#[cfg(feature = "playback")]
pub mod audio;
pub mod config_api;
pub mod gate;
pub mod info;
pub mod jobs;
pub mod openai;
mod plugin_catalog;
pub mod providers;
#[cfg(feature = "playback")]
pub mod queue;
pub mod rate_limit;
pub mod sampling;
pub mod tts;
pub mod voices;

use crate::AppState;
use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{get, post},
};

/// Build the API router. Body limits are read live
/// from the configuration so limit changes apply
/// when the listener restarts (spec §23).
pub async fn router(state: AppState) -> Router {
    let tts_body_limit = state.config.get().await.tts.max_body_bytes;
    let openai_body_limit = state.config.get().await.tts.openai_max_body_bytes;
    let voices_body_limit = state.config.get().await.voices.max_upload_bytes;
    #[cfg(feature = "playback")]
    let queue_body_limit = state.config.get().await.tts.queue_max_body_bytes;

    #[allow(unused_mut)]
    let mut router = Router::new()
        // Server info (auth discovery for the GUI).
        .route("/api/info", get(info::get_info))
        // Configuration API (spec §33).
        .route(
            "/api/config",
            get(config_api::get_config).patch(config_api::patch_config),
        )
        .route("/api/config/schema", get(config_api::get_schema))
        .route("/api/config/status", get(config_api::get_status))
        .route("/api/config/reload", post(config_api::reload_config))
        .route("/api/config/open", post(config_api::open_config_file))
        .route("/api/config/effective", get(config_api::get_effective))
        .route(
            "/api/config/validate",
            post(config_api::validate_config_file),
        )
        .route("/api/tts/plugin/providers", get(plugin_catalog::providers))
        .route("/api/tts/plugin/voices", get(plugin_catalog::voices))
        // Original TTS API
        .route(
            "/api/tts",
            post(tts::post_tts).route_layer(DefaultBodyLimit::max(tts_body_limit)),
        )
        .route(
            "/api/tts/jobs",
            post(jobs::create).route_layer(DefaultBodyLimit::max(tts_body_limit)),
        )
        .route("/api/tts/jobs/{id}", get(jobs::status))
        .route("/api/tts/jobs/{id}/events", get(jobs::events))
        .route("/api/tts/jobs/{id}/cancel", post(jobs::cancel))
        .route("/api/tts/jobs/{id}/audio", get(jobs::audio))
        .route("/api/status", get(tts::get_status))
        // OpenAI-compatible endpoints
        .route(
            "/v1/audio/speech",
            post(openai::post_speech).route_layer(DefaultBodyLimit::max(openai_body_limit)),
        )
        .route("/v1/models", get(openai::get_models))
        .route("/v1/models/list", get(openai::get_models))
        .route("/v1/voices", get(openai::get_voices))
        // Native provider discovery and lifecycle status.
        .route("/api/providers", get(providers::get_providers))
        .route(
            "/api/providers/{id}/status",
            get(providers::get_provider_status),
        )
        // Managed cloning voice profiles (reference upload + preview).
        .route(
            "/api/voices/clones",
            post(voices::post_clones).route_layer(DefaultBodyLimit::max(voices_body_limit)),
        )
        .route("/api/voices", get(voices::get_voices))
        .route(
            "/api/voices/{id}",
            get(voices::get_voice).delete(voices::delete_voice),
        )
        .route(
            "/api/voices/{id}/preview",
            post(voices::post_preview).route_layer(DefaultBodyLimit::max(tts_body_limit)),
        );

    #[cfg(feature = "playback")]
    {
        // Audio playback and queue endpoints (only available when playback feature is enabled)
        router = router
            .route(
                "/api/tts/play",
                post(tts::post_tts_and_play).route_layer(DefaultBodyLimit::max(tts_body_limit)),
            )
            .route(
                "/api/queue",
                post(queue::queue_audio).route_layer(DefaultBodyLimit::max(queue_body_limit)),
            )
            .route("/api/queue/next", post(queue::play_next))
            .route("/api/queue/pause", post(queue::pause_audio))
            .route("/api/queue/resume", post(queue::resume_audio))
            .route("/api/queue/stop", post(queue::stop_audio))
            .route(
                "/api/queue/volume",
                post(queue::set_volume).route_layer(DefaultBodyLimit::max(queue_body_limit)),
            )
            .route("/api/queue/status", get(queue::get_queue_status))
            // Audio output device discovery/selection (playback only).
            .route("/api/audio/devices", get(audio::list_output_devices))
            .route(
                "/api/audio/output",
                get(audio::get_output_device)
                    .post(audio::set_output_device)
                    .route_layer(DefaultBodyLimit::max(queue_body_limit)),
            );
    }

    router.with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::store::TokenStore;
    use crate::config::{AppConfig, ConfigManager, secrets::MemorySecretStore};
    use crate::runtime::ModelService;
    use crate::runtime::subsystems::{InferenceGateManager, RateLimiterManager};
    use crate::tts::ModelStatus;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use tower::ServiceExt;

    pub(super) fn test_config() -> AppConfig {
        let mut config = AppConfig::default();
        config.server.auth_mode = crate::config::AuthMode::Token;
        config.tts.max_body_bytes = 65_536;
        config.tts.openai_max_body_bytes = 65_536;
        config.tts.queue_max_body_bytes = 16_384;
        config.paths.audio = Some("/tmp".to_string());
        config
    }

    pub(super) fn test_state(
        config: AppConfig,
        token_store: Arc<TokenStore>,
        audio_manager: Arc<Option<crate::AudioManager>>,
    ) -> AppState {
        AppState {
            model_status: Arc::new(RwLock::new(ModelStatus::Idle)),
            token_store,
            config: ConfigManager::in_memory(config),
            audio_manager,
            inference_gate: Arc::new(InferenceGateManager::new(gate::InferenceGate::new(1, 8))),
            rate_limiter: Arc::new(RateLimiterManager::new(rate_limit::RateLimiter::new(0, 60))),
            model_service: Arc::new(ModelService::new(Arc::new(RwLock::new(ModelStatus::Idle)))),
            providers: Arc::new(sonicboom::tts::providers::ProviderPool::new()),
            secrets: Arc::new(MemorySecretStore::default()),
        }
    }

    async fn test_app() -> (Router, String) {
        let path = std::env::temp_dir().join(format!(
            "sonicboom-auth-test-{}-{}.json",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let store = TokenStore::load(path.to_str().unwrap()).await.unwrap();
        let (_token, raw) = store.create(None).await.unwrap();
        let state = test_state(test_config(), Arc::new(store), Arc::new(None));
        let _ = std::fs::remove_file(&path);
        (router(state).await, raw)
    }

    async fn post_status(app: Router, uri: &str, token: Option<&str>, body: &str) -> StatusCode {
        let mut builder = Request::post(uri);
        if let Some(token) = token {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        let request = builder.body(Body::from(body.to_string())).unwrap();
        app.oneshot(request).await.unwrap().status()
    }

    #[cfg(feature = "playback")]
    async fn audio_test_parts() -> (
        Router,
        String,
        tokio::sync::mpsc::Receiver<crate::tts::queue::AudioCommand>,
    ) {
        let path = std::env::temp_dir().join(format!(
            "sonicboom-audio-test-{}-{}.json",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let store = TokenStore::load(path.to_str().unwrap()).await.unwrap();
        let (_token, raw) = store.create(None).await.unwrap();
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        let manager = crate::tts::queue::AudioManager::for_test(tx);
        let state = test_state(test_config(), Arc::new(store), Arc::new(Some(manager)));
        let _ = std::fs::remove_file(&path);
        (router(state).await, raw, rx)
    }

    #[cfg(feature = "playback")]
    fn authed(uri: &str, raw: &str, method: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", format!("Bearer {raw}"))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    async fn body_json(response: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn configuration_endpoints_require_token_authentication() {
        let (app, _) = test_app().await;
        for (method, path) in [
            ("GET", "/api/config"),
            ("GET", "/api/config/schema"),
            ("GET", "/api/config/status"),
            ("POST", "/api/config/reload"),
            ("PATCH", "/api/config"),
        ] {
            let request = Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"changes":{}}"#))
                .unwrap();
            assert_eq!(
                app.clone().oneshot(request).await.unwrap().status(),
                StatusCode::UNAUTHORIZED,
                "{path}"
            );
        }
    }

    #[tokio::test]
    async fn tts_requires_authentication() {
        let (app, valid) = test_app().await;
        assert_eq!(
            post_status(app.clone(), "/api/tts", None, "hello").await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            post_status(app.clone(), "/api/tts", Some("wrong-token"), "hello").await,
            StatusCode::UNAUTHORIZED
        );
        // Valid token passes auth (503 proves it reached the model check).
        assert_eq!(
            post_status(app, "/api/tts", Some(&valid), "hello").await,
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[tokio::test]
    async fn only_strict_bearer_syntax_is_accepted() {
        let (app, valid) = test_app().await;
        // Canonical form works (503 proves it passed authentication).
        assert_eq!(
            post_status(app.clone(), "/api/tts", Some(&valid), "hello").await,
            StatusCode::SERVICE_UNAVAILABLE
        );
        // Scheme is case-insensitive.
        let request = Request::post("/api/tts")
            .header("authorization", format!("bearer {valid}"))
            .body(Body::from("hello"))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(request).await.unwrap().status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        // Everything else is rejected.
        for header in [
            valid.clone(),
            "Basic dXNlcjpwYXNz".to_string(),
            "Token ".to_string() + &valid,
            "Bearer".to_string(),
            "Bearer ".to_string(),
            "Bearer  ".to_string() + &valid,
        ] {
            let request = Request::post("/api/tts")
                .header("authorization", header.clone())
                .body(Body::from("hello"))
                .unwrap();
            assert_eq!(
                app.clone().oneshot(request).await.unwrap().status(),
                StatusCode::UNAUTHORIZED,
                "accepted {header:?}"
            );
        }
    }

    #[tokio::test]
    async fn spoofed_referer_and_host_do_not_authenticate() {
        let (app, _) = test_app().await;
        let request = Request::post("/api/tts")
            .header("host", "localhost:17842")
            .header("referer", "http://localhost:17842/")
            .header("origin", "http://localhost:17842")
            .body(Body::from("hello"))
            .unwrap();
        assert_eq!(
            app.oneshot(request).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );

        let (app, _) = test_app().await;
        let request = Request::post("/v1/audio/speech")
            .header("host", "localhost:17842")
            .header("referer", "http://localhost:17842/")
            .body(Body::from(r#"{"input":"hello"}"#))
            .unwrap();
        assert_eq!(
            app.oneshot(request).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn openai_speech_requires_authentication() {
        let (app, valid) = test_app().await;
        assert_eq!(
            post_status(app.clone(), "/v1/audio/speech", None, r#"{"input":"hi"}"#).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            post_status(app, "/v1/audio/speech", Some(&valid), r#"{"input":"hi"}"#).await,
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[tokio::test]
    async fn oversized_body_is_rejected_before_handling() {
        let (app, valid) = test_app().await;
        let big = "x".repeat(70_000); // over the 64 KiB TTS body limit
        assert_eq!(
            post_status(app, "/api/tts", Some(&valid), &big).await,
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }

    #[cfg(feature = "playback")]
    #[tokio::test]
    async fn playback_endpoints_require_authentication() {
        for (method, uri, body) in [
            ("POST", "/api/tts/play", "hello"),
            ("POST", "/api/queue", r#"{"path":"x.wav"}"#),
            ("POST", "/api/queue/next", ""),
            ("POST", "/api/queue/pause", ""),
            ("POST", "/api/queue/resume", ""),
            ("POST", "/api/queue/stop", ""),
            ("POST", "/api/queue/volume", r#"{"volume":0.5}"#),
            ("POST", "/api/audio/output", r#"{"device":"default"}"#),
        ] {
            let (app, _) = test_app().await;
            let request = Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap();
            assert_eq!(
                app.oneshot(request).await.unwrap().status(),
                StatusCode::UNAUTHORIZED,
                "{method} {uri} must require authentication"
            );
        }

        for uri in [
            "/api/queue/status",
            "/api/audio/devices",
            "/api/audio/output",
        ] {
            let (app, _) = test_app().await;
            let request = Request::get(uri).body(Body::empty()).unwrap();
            assert_eq!(
                app.oneshot(request).await.unwrap().status(),
                StatusCode::UNAUTHORIZED,
                "GET {uri} must require authentication"
            );
        }
    }

    #[cfg(not(feature = "playback"))]
    #[tokio::test]
    async fn audio_routes_are_absent_without_playback() {
        for (method, uri) in [
            ("GET", "/api/audio/devices"),
            ("GET", "/api/audio/output"),
            ("POST", "/api/audio/output"),
        ] {
            let (app, valid) = test_app().await;
            let request = Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {valid}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"device":"default"}"#))
                .unwrap();
            assert_eq!(
                app.oneshot(request).await.unwrap().status(),
                StatusCode::NOT_FOUND,
                "{method} {uri} must not exist without the playback feature"
            );
        }
    }

    #[tokio::test]
    async fn rate_limited_tts_carries_retry_headers() {
        // Tiny bucket: 2 requests, then the 3rd is a 429 with live metadata.
        // The model stays Idle so allowed requests pass the limiter and then
        // fail at the model check (503), proving limiter-before-model order.
        let path = std::env::temp_dir().join(format!(
            "sonicboom-ratelimit-test-{}-{}.json",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let store = TokenStore::load(path.to_str().unwrap()).await.unwrap();
        let (_token, raw) = store.create(None).await.unwrap();
        let mut config = test_config();
        config.rate_limit.requests = 2;
        config.rate_limit.burst = 2;
        let state = AppState {
            model_status: Arc::new(RwLock::new(ModelStatus::Idle)),
            token_store: Arc::new(store),
            config: ConfigManager::in_memory(config),
            audio_manager: Arc::new(None),
            inference_gate: Arc::new(InferenceGateManager::new(gate::InferenceGate::new(1, 8))),
            rate_limiter: Arc::new(RateLimiterManager::new(rate_limit::RateLimiter::new(2, 60))),
            model_service: Arc::new(ModelService::new(Arc::new(RwLock::new(ModelStatus::Idle)))),
            providers: Arc::new(sonicboom::tts::providers::ProviderPool::new()),
            secrets: Arc::new(MemorySecretStore::default()),
        };
        let _ = std::fs::remove_file(&path);
        let app = router(state).await;

        for _ in 0..2 {
            assert_eq!(
                post_status(app.clone(), "/api/tts", Some(&raw), "hello").await,
                StatusCode::SERVICE_UNAVAILABLE
            );
        }
        let request = Request::post("/api/tts")
            .header("authorization", format!("Bearer {raw}"))
            .body(Body::from("hello"))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let headers = response.headers();
        assert_eq!(headers.get("retry-after").unwrap(), "30");
        assert_eq!(headers.get("x-ratelimit-limit").unwrap(), "2");
        assert_eq!(headers.get("x-ratelimit-remaining").unwrap(), "0");
        assert!(headers.get("x-ratelimit-reset").is_some());
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"], "too_many_requests");
        assert_eq!(body["status"], 429);
    }

    #[cfg(feature = "playback")]
    #[tokio::test]
    async fn audio_devices_reports_list_and_selection() {
        use crate::tts::devices::{OutputDeviceInfo, OutputDeviceList};
        use crate::tts::queue::AudioCommand;

        let (app, valid, mut rx) = audio_test_parts().await;
        tokio::spawn(async move {
            match rx.recv().await {
                Some(AudioCommand::GetOutputDevices { reply }) => {
                    let _ = reply.send(Ok(OutputDeviceList {
                        devices: vec![
                            OutputDeviceInfo {
                                id: "default".to_string(),
                                name: "System Default".to_string(),
                                is_default: true,
                                is_selected: false,
                            },
                            OutputDeviceInfo {
                                id: "Speakers".to_string(),
                                name: "Speakers".to_string(),
                                is_default: false,
                                is_selected: true,
                            },
                        ],
                        selected: "Speakers".to_string(),
                    }));
                }
                other => panic!("unexpected command: {other:?}"),
            }
        });
        let response = app
            .oneshot(authed("/api/audio/devices", &valid, "GET", ""))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        assert_eq!(body["selected"], "Speakers");
        assert_eq!(body["devices"][0]["id"], "default");
        assert_eq!(body["devices"][1]["id"], "Speakers");
        assert_eq!(body["devices"][1]["name"], "Speakers");
    }

    #[cfg(feature = "playback")]
    #[tokio::test]
    async fn audio_devices_enumeration_failure_is_500_not_empty_success() {
        use crate::tts::queue::{AudioCommand, AudioManagerError};

        let (app, valid, mut rx) = audio_test_parts().await;
        tokio::spawn(async move {
            if let Some(AudioCommand::GetOutputDevices { reply }) = rx.recv().await {
                let _ = reply.send(Err(AudioManagerError::DeviceError));
            }
        });
        let response = app
            .oneshot(authed("/api/audio/devices", &valid, "GET", ""))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[cfg(feature = "playback")]
    #[tokio::test]
    async fn audio_output_reports_configured_selection() {
        use crate::tts::devices::ActiveOutputDevice;
        use crate::tts::queue::AudioCommand;

        let (app, valid, mut rx) = audio_test_parts().await;
        tokio::spawn(async move {
            if let Some(AudioCommand::GetOutputDevice { reply }) = rx.recv().await {
                let _ = reply.send(ActiveOutputDevice {
                    device: "CABLE Input".to_string(),
                    resolved_name: Some("CABLE Input".to_string()),
                    available: true,
                });
            }
        });
        let response = app
            .oneshot(authed("/api/audio/output", &valid, "GET", ""))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        assert_eq!(body["device"], "CABLE Input");
        assert_eq!(body["resolved_name"], "CABLE Input");
        assert_eq!(body["available"], true);
    }

    #[cfg(feature = "playback")]
    #[tokio::test]
    async fn audio_output_selection_success_and_unknown_rejected() {
        use crate::tts::devices::{OutputDeviceInfo, OutputDeviceList};
        use crate::tts::queue::AudioCommand;
        for (device, expected) in [
            ("CABLE Input", StatusCode::OK),
            ("Nope", StatusCode::BAD_REQUEST),
        ] {
            let (app, valid, mut rx) = audio_test_parts().await;
            let task = tokio::spawn(async move {
                match rx.recv().await {
                    Some(AudioCommand::GetOutputDevices { reply }) => {
                        let _ = reply.send(Ok(OutputDeviceList {
                            devices: vec![OutputDeviceInfo {
                                id: "CABLE Input".into(),
                                name: "CABLE Input".into(),
                                is_default: false,
                                is_selected: false,
                            }],
                            selected: "default".into(),
                        }));
                    }
                    other => panic!("unexpected command: {other:?}"),
                }
                // The handler must not bypass the coordinator by switching hardware.
                assert!(rx.recv().await.is_none());
            });
            let body = serde_json::json!({"device": device}).to_string();
            let response = app
                .oneshot(authed("/api/audio/output", &valid, "POST", &body))
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
            if expected == StatusCode::OK {
                let body = body_json(response).await;
                assert_eq!(body["device"], device);
                assert_eq!(body["status"], "pending");
            }
            task.await.unwrap();
        }
    }

    #[cfg(feature = "playback")]
    #[tokio::test]
    async fn audio_output_rejects_empty_device_without_contacting_thread() {
        let (app, valid, rx) = audio_test_parts().await;
        drop(rx); // No command must be sent: validation happens first.
        let response = app
            .oneshot(authed(
                "/api/audio/output",
                &valid,
                "POST",
                r#"{"device":"  "}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    fn voices_test_config() -> AppConfig {
        let mut config = test_config();
        config.voices.store_dir = std::env::temp_dir()
            .join(format!("sonicboom-voices-test-{}", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .to_string();
        config
    }

    async fn voices_test_app() -> (Router, String, String) {
        let path = std::env::temp_dir().join(format!(
            "sonicboom-voices-auth-test-{}-{}.json",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let store = TokenStore::load(path.to_str().unwrap()).await.unwrap();
        let (_token, raw) = store.create(None).await.unwrap();
        let config = voices_test_config();
        let store_dir = config.voices.store_dir.clone();
        let state = test_state(config, Arc::new(store), Arc::new(None));
        let _ = std::fs::remove_file(&path);
        (router(state).await, raw, store_dir)
    }

    fn raw_request(
        method: &str,
        uri: &str,
        token: Option<&str>,
        content_type: &str,
        body: &[u8],
    ) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", content_type);
        if let Some(token) = token {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        builder.body(Body::from(body.to_vec())).unwrap()
    }

    #[tokio::test]
    async fn job_operations_require_authentication_and_foreign_jobs_are_hidden() {
        let (app, valid, store_dir) = voices_test_app().await;
        for (method, uri) in [
            ("POST", "/api/tts/jobs"),
            ("GET", "/api/tts/jobs/missing"),
            ("GET", "/api/tts/jobs/missing/events"),
            ("GET", "/api/tts/jobs/missing/audio"),
            ("POST", "/api/tts/jobs/missing/cancel"),
        ] {
            assert_eq!(
                app.clone()
                    .oneshot(raw_request(method, uri, None, "text/plain", b"hello"))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::UNAUTHORIZED
            );
        }
        let response = app
            .clone()
            .oneshot(raw_request(
                "POST",
                "/api/tts/jobs?format=wav",
                Some(&valid),
                "text/plain",
                b"hello",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let body = body_json(response).await;
        let id = body["id"].as_str().unwrap();
        // No model is attached: this accepted job must reach failed without
        // exposing successful audio or remaining queued indefinitely.
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let response = app
                    .clone()
                    .oneshot(raw_request(
                        "GET",
                        &format!("/api/tts/jobs/{id}"),
                        Some(&valid),
                        "text/plain",
                        b"",
                    ))
                    .await
                    .unwrap();
                let snapshot = body_json(response).await;
                if snapshot["stage"] == "failed" {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let audio = app
            .clone()
            .oneshot(raw_request(
                "GET",
                &format!("/api/tts/jobs/{id}/audio"),
                Some(&valid),
                "text/plain",
                b"",
            ))
            .await
            .unwrap();
        assert_eq!(audio.status(), StatusCode::SERVICE_UNAVAILABLE);
        for _ in 0..2 {
            let response = app
                .clone()
                .oneshot(raw_request(
                    "POST",
                    &format!("/api/tts/jobs/{id}/cancel"),
                    Some(&valid),
                    "text/plain",
                    b"",
                ))
                .await
                .unwrap();
            assert_eq!(
                body_json(response).await["stage"],
                "failed",
                "terminal cancellation is idempotent"
            );
        }
        let _ = std::fs::remove_dir_all(store_dir);
    }
    #[tokio::test]
    async fn concurrent_profile_creations_do_not_lose_manifest_updates() {
        let config = voices_test_config();
        let dir = config.voices.store_dir.clone();
        let state2 = test_state(
            config.clone(),
            Arc::new(TokenStore::empty()),
            Arc::new(None),
        );
        let state = test_state(config, Arc::new(TokenStore::empty()), Arc::new(None));
        let mut tasks = Vec::new();
        for i in 0..8 {
            // Independent caches exercise the interprocess-style file lock too.
            let state = if i % 2 == 0 {
                state.clone()
            } else {
                state2.clone()
            };
            tasks.push(tokio::spawn(async move {
                super::voices::with_store(&state, move |store| {
                    store
                        .create(sonicboom::tts::voices::NewVoiceProfile {
                            name: format!("test-{i}"),
                            provider: "qwen3-gguf".into(),
                            model: "qwen3-tts-1.7b-base".into(),
                            model_revision: "test".into(),
                            language: "en".into(),
                            clone_mode: "icl".into(),
                            reference_pcm: vec![0.1; 24000],
                            reference_sample_rate: 24000,
                            transcript: Some("hello".to_string()),
                            owner_token_hash: Some("owner".into()),
                            consent_basis: "owner".into(),
                            consent_note: String::new(),
                        })
                        .map_err(crate::error::AppError::BadRequest)
                })
                .await
                .unwrap();
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        let persisted =
            sonicboom::tts::voices::VoiceProfileStore::open(dir.clone().into()).unwrap();
        assert_eq!(persisted.list().len(), 8);
        let _ = std::fs::remove_dir_all(dir);
    }
    #[tokio::test]
    async fn provider_and_voice_endpoints_require_token_authentication() {
        let (app, _, _) = voices_test_app().await;
        for (method, uri, content_type, body) in [
            ("GET", "/api/providers", "application/json", ""),
            (
                "GET",
                "/api/providers/pocket-tts/status",
                "application/json",
                "",
            ),
            ("GET", "/api/voices", "application/json", ""),
            (
                "POST",
                "/api/voices/clones?name=t&provider=pocket-tts&model=pocket-tts-en&language=en&mode=audio_prompt&consent_basis=owner",
                "audio/wav",
                "",
            ),
            (
                "GET",
                "/api/voices/123e4567-e89b-12d3-a456-426614174000",
                "application/json",
                "",
            ),
            (
                "DELETE",
                "/api/voices/123e4567-e89b-12d3-a456-426614174000",
                "application/json",
                "",
            ),
            (
                "POST",
                "/api/voices/123e4567-e89b-12d3-a456-426614174000/preview",
                "text/plain",
                "hello",
            ),
        ] {
            let request = raw_request(method, uri, None, content_type, body.as_bytes());
            assert_eq!(
                app.clone().oneshot(request).await.unwrap().status(),
                StatusCode::UNAUTHORIZED,
                "{method} {uri} must require authentication"
            );
        }
    }

    #[tokio::test]
    async fn providers_list_reports_providers_without_candle() {
        let (app, valid, store_dir) = voices_test_app().await;
        let request = raw_request(
            "GET",
            "/api/providers",
            Some(&valid),
            "application/json",
            b"",
        );
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 256 * 1024)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let ids: Vec<_> = body["providers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["provider_id"].as_str().unwrap().to_string())
            .collect();
        assert!(!ids.contains(&"qwen3".to_string()));
        assert!(!ids.contains(&"gpt-sovits".to_string()));
        assert!(ids.contains(&"pocket-tts".to_string()));
        assert!(ids.contains(&"supertonic".to_string()));
        let _ = std::fs::remove_dir_all(&store_dir);
    }

    #[tokio::test]
    async fn unknown_provider_status_is_rejected_not_defaulted() {
        let (app, valid, store_dir) = voices_test_app().await;
        let request = raw_request(
            "GET",
            "/api/providers/nope/status",
            Some(&valid),
            "application/json",
            b"",
        );
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let _ = std::fs::remove_dir_all(&store_dir);
    }

    #[tokio::test]
    async fn pocket_voice_upload_rejects_cross_language_profiles() {
        let (app, valid, store_dir) = voices_test_app().await;
        let request = raw_request(
            "POST",
            "/api/voices/clones?name=t&provider=pocket-tts&model=pocket-tts-pt&language=ja&mode=audio_prompt&consent_basis=owner",
            Some(&valid),
            "audio/wav",
            b"not-needed-for-language-validation",
        );
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(response.into_body(), 8192)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("language"));
        let request = raw_request(
            "POST",
            "/api/voices/clones?name=t&provider=pocket-tts&model=pocket-tts-pt&language=de&mode=audio_prompt&consent_basis=owner",
            Some(&valid),
            "audio/wav",
            b"not-needed-for-language-validation",
        );
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(response.into_body(), 8192)
            .await
            .unwrap();
        let body = String::from_utf8_lossy(&bytes);
        assert!(body.contains("language") && body.contains("pt"), "{body}");
        let _ = std::fs::remove_dir_all(&store_dir);
    }

    #[tokio::test]
    async fn voice_upload_returns_non_destructive_reference_health_warnings() {
        let (app, valid, store_dir) = voices_test_app().await;
        let wav = sonicboom::tts::audio::encode_audio(
            &vec![0.0; 24000],
            24000,
            sonicboom::tts::audio::AudioFormat::Wav,
        )
        .unwrap();
        let response = app.oneshot(raw_request("POST", "/api/voices/clones?name=synthetic-silence&provider=pocket-tts&model=pocket-tts-en&language=en&mode=audio_prompt&consent_basis=owner", Some(&valid), "audio/wav", &wav)).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let result = body_json(response).await;
        assert_eq!(result["reference_health"]["source_channels"], 1);
        assert_eq!(result["reference_health"]["source_sample_rate"], 24000);
        assert_eq!(result["reference_health"]["duration_secs"], 1.0);
        assert_eq!(result["reference_health"]["peak"], 0.0);
        assert!(
            result["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|w| w == "reference_nearly_silent")
        );
        assert!(result["reference_health"].get("pcm").is_none());
        std::fs::remove_dir_all(store_dir).unwrap();
    }

    #[tokio::test]
    async fn voice_upload_with_wrong_media_is_415() {
        let (app, valid, store_dir) = voices_test_app().await;
        let request = raw_request(
            "POST",
            "/api/voices/clones?name=t&provider=pocket-tts&model=pocket-tts-en&language=en&mode=audio_prompt&consent_basis=owner",
            Some(&valid),
            "application/json",
            b"{}",
        );
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        let _ = std::fs::remove_dir_all(&store_dir);
    }

    #[tokio::test]
    async fn voice_upload_with_undecodable_bytes_is_400() {
        let (app, valid, store_dir) = voices_test_app().await;
        let request = raw_request(
            "POST",
            "/api/voices/clones?name=t&provider=pocket-tts&model=pocket-tts-en&language=en&mode=audio_prompt&consent_basis=owner",
            Some(&valid),
            "audio/wav",
            b"not-a-wav-file",
        );
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let _ = std::fs::remove_dir_all(&store_dir);
    }

    #[tokio::test]
    async fn missing_voice_profiles_are_404_without_disclosure() {
        let (app, valid, store_dir) = voices_test_app().await;
        let missing = format!("/api/voices/{}", uuid::Uuid::new_v4());
        for (method, uri, content_type, body) in [
            ("GET", missing.clone(), "application/json", ""),
            ("DELETE", missing.clone(), "application/json", ""),
            ("POST", format!("{missing}/preview"), "text/plain", "hello"),
        ] {
            let request = raw_request(method, &uri, Some(&valid), content_type, body.as_bytes());
            assert_eq!(
                app.clone().oneshot(request).await.unwrap().status(),
                StatusCode::NOT_FOUND,
                "{method} {uri} must be 404"
            );
        }
        let _ = std::fs::remove_dir_all(&store_dir);
    }

    #[tokio::test]
    async fn models_list_includes_cloning_models_with_owners() {
        let (app, _, store_dir) = voices_test_app().await;
        let request = Request::get("/v1/models").body(Body::empty()).unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 256 * 1024)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let ids: Vec<_> = body["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap().to_string())
            .collect();
        for expected in [
            "tts-1",
            "tts-1-hd",
            "supertonic-3",
            "qwen3-tts-1.7b-base",
            "pocket-tts-en",
            "pocket-tts-es",
        ] {
            assert!(
                ids.contains(&expected.to_string()),
                "models list missing {expected}"
            );
        }
        assert!(
            !ids.contains(&"qwen3-tts-0.6b-base".to_string()),
            "retired Candle model must not be listed"
        );
        let _ = std::fs::remove_dir_all(&store_dir);
    }

    #[cfg(feature = "playback")]
    #[tokio::test]
    async fn audio_apis_are_unavailable_without_manager() {
        let (app, valid) = test_app().await; // audio_manager: None
        for (method, uri, body) in [
            ("GET", "/api/audio/devices", ""),
            ("GET", "/api/audio/output", ""),
            ("POST", "/api/audio/output", r#"{"device":"default"}"#),
        ] {
            let response = app
                .clone()
                .oneshot(authed(uri, &valid, method, body))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::SERVICE_UNAVAILABLE,
                "{method} {uri} without a manager must be 503"
            );
        }
    }
    #[tokio::test]
    async fn plugin_catalog_requires_auth_and_reports_enabled_unavailable_provider() {
        let (app, raw) = test_app().await;
        for path in ["/api/tts/plugin/providers", "/api/tts/plugin/voices"] {
            let request = Request::get(path).body(Body::empty()).unwrap();
            assert_eq!(
                app.clone().oneshot(request).await.unwrap().status(),
                StatusCode::UNAUTHORIZED
            );
            let request = Request::get(path)
                .header("authorization", format!("Bearer {raw}"))
                .body(Body::empty())
                .unwrap();
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()["cache-control"], "private, no-store");
            let body = body_json(response).await;
            let items = body.as_array().unwrap();
            if path.ends_with("providers") {
                assert_eq!(items.len(), 1); // Optional providers disabled, Supertonic enabled but not loaded.
                assert_eq!(items[0]["id"], "supertonic");
                assert_eq!(items[0]["disabled"], true);
            } else {
                assert!(items.is_empty());
            }
        }
    }

    #[cfg(feature = "playback")]
    #[tokio::test]
    async fn plugin_play_rejects_invalid_identity_and_foreign_profiles_without_fallback() {
        let (app, raw) = test_app().await;
        for (query, status) in [
            ("provider=unknown&voice=M1", StatusCode::BAD_REQUEST),
            (
                "provider=pocket-tts&voice=qwen3-gguf:any",
                StatusCode::BAD_REQUEST,
            ),
            ("provider=pocket-tts&voice=", StatusCode::BAD_REQUEST),
            (
                "provider=pocket-tts&voice=123e4567-e89b-12d3-a456-426614174000",
                StatusCode::NOT_FOUND,
            ),
            (
                "provider=supertonic&voice=M1",
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            ("voice=M1", StatusCode::SERVICE_UNAVAILABLE),
            ("provider=&voice=M1", StatusCode::SERVICE_UNAVAILABLE),
        ] {
            assert_eq!(
                post_status(
                    app.clone(),
                    &format!("/api/tts/play?{query}"),
                    Some(&raw),
                    "hello"
                )
                .await,
                status,
                "{query}"
            );
        }
        assert_eq!(
            post_status(app, "/api/tts/play?provider=pocket-tts", Some(&raw), "").await,
            StatusCode::BAD_REQUEST
        );
    }
    #[cfg(feature = "playback")]
    #[tokio::test]
    async fn plugin_play_profile_ownership_revision_and_disabled_provider() {
        use sonicboom::tts::voices::{NewVoiceProfile, VoiceProfileStore};
        let dir = std::env::temp_dir().join(format!("plugin-play-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let tokens = TokenStore::load(dir.join("tokens.json").to_str().unwrap())
            .await
            .unwrap();
        let (token, raw) = tokens.create(None).await.unwrap();
        let mut config = test_config();
        config.voices.store_dir = dir.join("voices").to_string_lossy().into_owned();
        let revision = voices::config_revision_for(&config, "pocket-tts");
        let mut store = VoiceProfileStore::open(config.voices.store_dir.clone().into()).unwrap();
        let mut create = |owner: &str, revision: &str| {
            store
                .create(NewVoiceProfile {
                    name: "Synthetic test reference".into(),
                    provider: "pocket-tts".into(),
                    model: "pocket-tts-en".into(),
                    model_revision: revision.into(),
                    language: "en".into(),
                    clone_mode: "audio_prompt".into(),
                    reference_pcm: vec![0.1; 72_000],
                    reference_sample_rate: 24_000,
                    transcript: None,
                    owner_token_hash: Some(owner.into()),
                    consent_basis: "owner".into(),
                    consent_note: String::new(),
                })
                .unwrap()
        };
        let owned = create(&token.token_hash, &revision);
        let stale = create(&token.token_hash, "incompatible-test-revision");
        let foreign = create("different-token-hash", &revision);
        let app = router(test_state(config, Arc::new(tokens), Arc::new(None))).await;
        for (provider, id, status) in [
            (
                "pocket-tts",
                owned.id.as_str(),
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            ("pocket-tts", stale.id.as_str(), StatusCode::BAD_REQUEST),
            ("pocket-tts", foreign.id.as_str(), StatusCode::NOT_FOUND),
            #[cfg(feature = "qwen-gguf")]
            ("qwen3-gguf", owned.id.as_str(), StatusCode::BAD_REQUEST),
        ] {
            assert_eq!(
                post_status(
                    app.clone(),
                    &format!("/api/tts/play?provider={provider}&voice={id}&lang=en"),
                    Some(&raw),
                    "hello"
                )
                .await,
                status
            );
        }
        let response = app
            .clone()
            .oneshot(
                Request::get("/api/tts/plugin/voices")
                    .header("authorization", format!("Bearer {raw}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(body_json(response).await, serde_json::json!([])); // Disabled provider exposes no usable profiles.
        store.delete(&owned.id).unwrap();
        assert_eq!(
            post_status(
                app,
                &format!("/api/tts/play?provider=pocket-tts&voice={}", owned.id),
                Some(&raw),
                "hello"
            )
            .await,
            StatusCode::NOT_FOUND
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
