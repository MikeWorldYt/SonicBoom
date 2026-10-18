//! Restartable HTTP listener manager.
//!
//! The axum [`Router`] is rebuilt from the shared
//! [`AppState`](crate::AppState) on every restart, so
//! configuration that affects routing (body limits,
//! session cookies) is picked up without a process
//! restart.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::response::IntoResponse;
use tokio::sync::{Mutex, RwLock, oneshot};
use tokio::task::JoinHandle;
use tower_http::trace::{DefaultOnFailure, DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tower_sessions::{Expiry, MemoryStore, SessionManagerLayer};

use crate::admin::handlers::AdminState;
use crate::{AppState, admin, api, security, web};

/// Manages the HTTP listener lifecycle.
pub struct ApiServerManager {
    lifecycle: Mutex<()>,
    session_store: MemoryStore,
    state: AppState,
    admin_state: AdminState,
    current_addr: RwLock<Option<SocketAddr>>,
    listener: Mutex<Option<std::net::TcpListener>>,
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl ApiServerManager {
    pub fn new(state: AppState, admin_state: AdminState) -> Self {
        Self {
            lifecycle: Mutex::new(()),
            session_store: MemoryStore::default(),
            state,
            admin_state,
            current_addr: RwLock::new(None),
            listener: Mutex::new(None),
            shutdown: Mutex::new(None),
            task: Mutex::new(None),
        }
    }

    /// The address the listener is currently bound to.
    pub async fn current_addr(&self) -> Option<SocketAddr> {
        *self.current_addr.read().await
    }

    /// Build the axum router from the shared state.
    /// Reads body limits and session settings from
    /// the current configuration, so a restart
    /// picks up limit and cookie changes.
    async fn build_router(&self) -> Router {
        let config = self.state.config.get().await;

        let session_store = self.session_store.clone();
        let session_layer = SessionManagerLayer::new(session_store)
            .with_name("sonicboom_admin")
            .with_path("/")
            .with_http_only(true)
            .with_same_site(tower_sessions::cookie::SameSite::Strict)
            .with_secure(config.security.cookie_secure)
            .with_expiry(Expiry::OnInactivity(time::Duration::seconds(
                config.admin.session_expiry_secs,
            )));

        let request_timeout = config.server.request_timeout_secs;

        Router::new()
            .merge(web::router(self.state.clone()))
            .merge(api::router(self.state.clone()).await)
            .merge(admin::router(self.admin_state.clone()).await)
            .layer(axum::middleware::from_fn_with_state(
                Arc::clone(&self.state.config),
                security::security_headers,
            ))
            .layer(axum::middleware::from_fn_with_state(
                Arc::clone(&self.state.config),
                security::request_origin_guard,
            ))
            .layer(session_layer)
            .layer(axum::middleware::from_fn(move |request: axum::extract::Request, next: axum::middleware::Next| async move {
                let secs = synchronous_http_budget(request_timeout, request.uri().path(), request.uri().query());
                match tokio::time::timeout(Duration::from_secs(secs), next.run(request)).await {
                    Ok(response) => response,
                    Err(_) => (axum::http::StatusCode::REQUEST_TIMEOUT,
                        axum::Json(serde_json::json!({"error":"HTTP request deadline exceeded; use /api/tts/jobs for slow synthesis", "stage":"http_request_deadline"}))).into_response(),
                }
            }))
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(|request: &axum::http::Request<axum::body::Body>| tracing::info_span!("http_request", method = %request.method(), path = request.uri().path()))
                    .on_request(DefaultOnRequest::new())
                    .on_response(DefaultOnResponse::new())
                    .on_failure(DefaultOnFailure::new()),
            )
    }

    /// Start (or restart) the listener on the currently
    /// configured bind address and port.
    pub async fn start(&self) -> anyhow::Result<()> {
        let _lifecycle = self.lifecycle.lock().await;
        let config = self.state.config.get().await;
        let addr = SocketAddr::new(config.server.bind, config.server.port);

        let router = self.build_router().await;
        // Bind a changed address before shutting down the working listener.
        // A failed bind keeps the previous server available.
        let same_address = self.current_addr().await == Some(addr);
        let listener = async {
            if same_address {
                let held = self.listener.lock().await;
                let duplicate = held.as_ref().ok_or_else(|| std::io::Error::other("listener unavailable"))?.try_clone()?;
                tokio::net::TcpListener::from_std(duplicate)
            } else {
                tokio::net::TcpListener::bind(addr).await
            }
        }.await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AddrInUse {
                    anyhow::anyhow!(
                        "failed to bind {addr}: {e}. \
                         Another SonicBoom instance may be running, or another app uses port {}. \
                         Set a different port with server.port in config.toml \
                         (dev: `export PORT=...`; docker: `-e PORT=...` with a matching `-p` publish).",
                        config.server.port
                    )
                } else {
                    anyhow::anyhow!("failed to bind {addr}: {e}")
                }
            })?;

        let held_listener = listener.into_std()?;
        let bound_addr = held_listener.local_addr()?;
        let listener = tokio::net::TcpListener::from_std(held_listener.try_clone()?)?;
        self.stop_inner().await?;
        *self.listener.lock().await = Some(held_listener);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            if let Err(e) = axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(shutdown_signal(shutdown_rx))
            .await
            {
                tracing::error!("HTTP listener error: {e}");
            }
        });

        *self.shutdown.lock().await = Some(shutdown_tx);
        *self.task.lock().await = Some(task);
        *self.current_addr.write().await = Some(bound_addr);
        #[cfg(feature = "gui")]
        crate::update_gui_url(bound_addr);

        tracing::info!(%addr, "HTTP listener started");
        Ok(())
    }

    /// Stop the listener gracefully.
    pub async fn stop(&self) -> anyhow::Result<()> {
        let _lifecycle = self.lifecycle.lock().await;
        self.stop_inner().await
    }

    async fn stop_inner(&self) -> anyhow::Result<()> {
        if let Some(tx) = self.shutdown.lock().await.take() {
            // A closed receiver means the task already
            // finished; the send error is harmless.
            let _ = tx.send(());
        }
        if let Some(task) = self.task.lock().await.take() {
            // The serve task ends on the shutdown signal
            // or its own error; either way we await it.
            let mut task = task;
            if tokio::time::timeout(Duration::from_secs(5), &mut task)
                .await
                .is_err()
            {
                task.abort();
                let _ = task.await;
            }
        }
        self.listener.lock().await.take();
        *self.current_addr.write().await = None;
        Ok(())
    }

    /// Restart the listener with the current configuration.
    pub async fn restart(&self) -> anyhow::Result<()> {
        let old = self.current_addr().await;
        self.start().await?;
        let new = self.current_addr().await;
        tracing::info!(?old, ?new, "HTTP listener restarted");
        Ok(())
    }
}

/// Waits for the shutdown signal (or task abort).
/// `oneshot::Receiver` implements `Future` directly.
async fn shutdown_signal(rx: oneshot::Receiver<()>) {
    let _ = rx.await;
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_server() -> Arc<ApiServerManager> {
        let port_probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = port_probe.local_addr().unwrap().port();
        drop(port_probe);
        let mut config = crate::config::AppConfig {
            setup_complete: true,
            ..Default::default()
        };
        config.server.port = port;
        config.paths.audio = Some("/tmp".into());
        let config = crate::config::ConfigManager::in_memory(config);
        let store = Arc::new(crate::auth::store::TokenStore::empty());
        let secrets = Arc::new(crate::config::secrets::MemorySecretStore::default());
        let status = Arc::new(RwLock::new(crate::tts::ModelStatus::Idle));
        let state = AppState {
            config: config.clone(),
            token_store: store.clone(),
            secrets: secrets.clone(),
            model_status: status.clone(),
            audio_manager: Arc::new(None),
            model_service: Arc::new(crate::runtime::ModelService::new(status)),
            providers: Arc::new(sonicboom::tts::providers::ProviderPool::new()),
            rate_limiter: Arc::new(crate::runtime::subsystems::RateLimiterManager::new(
                crate::api::rate_limit::RateLimiter::new(0, 60),
            )),
            inference_gate: Arc::new(crate::runtime::subsystems::InferenceGateManager::new(
                crate::api::gate::InferenceGate::new(1, 8),
            )),
        };
        let admin = AdminState {
            config,
            token_store: store,
            secrets,
            lockout: Arc::new(crate::admin::lockout::LoginAttemptTracker::default()),
        };
        Arc::new(ApiServerManager::new(state, admin))
    }

    #[tokio::test]
    async fn changed_port_restarts_only_listener_and_failed_bind_keeps_old_listener() {
        let server = test_server().await;
        server.start().await.unwrap();
        let old = server.current_addr().await.unwrap();
        let model = server.state.model_status.clone();
        server.restart().await.unwrap();
        assert_eq!(server.current_addr().await, Some(old));
        let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = occupied.local_addr().unwrap().port();
        server
            .state
            .config
            .update(None, |c| c.server.port = port)
            .await
            .unwrap();
        assert!(server.restart().await.is_err());
        assert_eq!(server.current_addr().await, Some(old));
        let response = reqwest::get(format!("http://{old}/health")).await.unwrap();
        assert!(response.status().is_success());
        drop(occupied);
        server.restart().await.unwrap();
        let new = server.current_addr().await.unwrap();
        assert_eq!(new.port(), port);
        assert!(
            reqwest::get(format!("http://{new}/health"))
                .await
                .unwrap()
                .status()
                .is_success()
        );
        assert!(Arc::ptr_eq(&model, &server.state.model_status));
        assert!(matches!(*model.read().await, crate::tts::ModelStatus::Idle));
        server.stop().await.unwrap();
    }

    #[test]
    fn shutdown_signal_resolves_on_channel_drop() {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        drop(tx);
        // Completing the future proves the receiver
        // resolves when the sender is dropped.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async { shutdown_signal(rx).await });
    }
}

/// Only the legacy synchronous playback route gets a provider budget. Job
/// submission/polling retain the ordinary short HTTP contract; inference has
/// its own deadline and admission controls. Explicit non-default server
/// limits remain upper bounds.
fn synchronous_http_budget(configured: u64, path: &str, query: Option<&str>) -> u64 {
    if path != "/api/tts/play" {
        return configured.max(1);
    }
    let provider = query
        .unwrap_or("")
        .split('&')
        .find_map(|part| part.strip_prefix("provider="))
        .unwrap_or("supertonic");
    let budget = match provider {
        "qwen3-gguf" => 180,
        "pocket-tts" => 60,
        _ => 15,
    };
    if configured == 120 {
        budget
    } else {
        budget.min(configured.max(1))
    }
}

#[cfg(test)]
mod synchronous_budget_tests {
    #[test]
    fn provider_budgets_do_not_expand_jobs_or_other_requests() {
        use super::synchronous_http_budget as budget;
        assert_eq!(
            budget(120, "/api/tts/play", Some("provider=qwen3-gguf")),
            180
        );
        assert_eq!(
            budget(120, "/api/tts/play", Some("provider=pocket-tts")),
            60
        );
        assert_eq!(
            budget(120, "/api/tts/play", Some("provider=supertonic")),
            15
        );
        assert_eq!(budget(20, "/api/tts/play", Some("provider=qwen3-gguf")), 20);
        assert_eq!(
            budget(120, "/api/tts/jobs", Some("provider=qwen3-gguf")),
            120
        );
    }
}
