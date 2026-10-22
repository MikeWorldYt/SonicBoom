pub mod index;
pub mod static_files;

use crate::AppState;
use axum::routing::post;
use axum::{Router, routing::get};

pub fn router(state: AppState) -> Router {
    let router = Router::new()
        .route("/health", get(index::get_health))
        .route("/ready", get(index::get_ready))
        .route("/setup/status", get(crate::setup::get_setup_status))
        .route(
            "/setup/audio-devices",
            get(crate::setup::get_setup_audio_devices),
        )
        .route("/setup/complete", post(crate::setup::post_setup_complete));
    #[cfg(feature = "web-ui")]
    let router = router
        .route("/", get(index::get_index))
        .route("/assets/{*file}", get(static_files::get_static))
        .route("/setup", get(crate::setup::get_setup));
    router.with_state(state)
}

#[cfg(all(test, not(feature = "web-ui")))]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use tower::ServiceExt;

    #[tokio::test]
    async fn api_only_keeps_health_and_readiness_without_pages() {
        let state = AppState {
            model_status: Arc::new(RwLock::new(crate::tts::ModelStatus::Idle)),
            token_store: Arc::new(crate::auth::store::TokenStore::empty()),
            config: crate::config::ConfigManager::in_memory(crate::config::AppConfig::default()),
            audio_manager: Arc::new(None),
            inference_gate: Arc::new(crate::runtime::subsystems::InferenceGateManager::new(
                crate::api::gate::InferenceGate::new(1, 8),
            )),
            rate_limiter: Arc::new(crate::runtime::subsystems::RateLimiterManager::new(
                crate::api::rate_limit::RateLimiter::new(0, 60),
            )),
            model_service: Arc::new(crate::runtime::ModelService::new(Arc::new(RwLock::new(
                crate::tts::ModelStatus::Idle,
            )))),
            providers: Arc::new(crate::tts::providers::ProviderPool::new()),
            secrets: Arc::new(crate::config::secrets::MemorySecretStore::default()),
        };
        let app = router(state.clone());
        for (path, expected) in [
            ("/health", StatusCode::OK),
            ("/setup/status", StatusCode::UNAUTHORIZED),
            ("/ready", StatusCode::SERVICE_UNAVAILABLE),
            ("/", StatusCode::NOT_FOUND),
            ("/setup", StatusCode::NOT_FOUND),
            ("/assets/app.js", StatusCode::NOT_FOUND),
        ] {
            let response = app
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), expected, "{path}");
        }
        *state.model_status.write().await =
            crate::tts::ModelStatus::Failed("incompatible model".into());
        assert_eq!(
            app.oneshot(Request::get("/ready").body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
}
