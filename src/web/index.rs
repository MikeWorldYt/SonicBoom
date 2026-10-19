use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::{AppState, tts::ModelStatus};

/// Liveness: the process is alive. Always 200 when the server can respond.
/// Used by container `HEALTHCHECK` / load-balancer liveness probes.
pub async fn get_health() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

/// Readiness: the model is loaded and the server can synthesize.
/// Used by load-balancer readiness probes and orchestrators.
pub async fn get_ready(State(state): State<AppState>) -> impl IntoResponse {
    let status = state.model_status.read().await;
    match &*status {
        ModelStatus::Ready(_) => (StatusCode::OK, "ready"),
        ModelStatus::Downloading { .. } => (StatusCode::SERVICE_UNAVAILABLE, "model downloading"),
        ModelStatus::Loading => (StatusCode::SERVICE_UNAVAILABLE, "model loading"),
        ModelStatus::Idle => (StatusCode::SERVICE_UNAVAILABLE, "model idle"),
        ModelStatus::Failed(_) => (StatusCode::SERVICE_UNAVAILABLE, "model failed to load"),
    }
}

pub async fn get_index(
    State(state): State<AppState>,
    axum::extract::Extension(nonce): axum::extract::Extension<crate::security::CspNonce>,
) -> Response {
    if !state.config.get().await.setup_complete {
        return axum::response::Redirect::to("/setup").into_response();
    }
    let status = state.model_status.read().await;
    let (voices, message): (Vec<String>, String) = match &*status {
        ModelStatus::Ready(handle) => {
            let mut voices: Vec<_> = handle.voice_styles.keys().cloned().collect();
            voices.sort();
            (voices, String::new())
        }
        ModelStatus::Idle => (vec![], "Preparing model...".into()),
        ModelStatus::Downloading { progress } => (
            vec![],
            format!("Downloading model... ({:.0}%)", progress * 100.0),
        ),
        ModelStatus::Loading => (vec![], "Loading model...".into()),
        ModelStatus::Failed(_) => (vec![], "Model load failed. Check server logs.".into()),
    };
    drop(status);
    let registry = crate::api::providers::build_registry_for_state(&state).await;
    let providers: Vec<_> = registry
        .statuses()
        .into_iter()
        .map(|status| {
            let models = registry
                .get(&status.provider_id)
                .map(|provider| provider.capabilities().models.join(", "))
                .unwrap_or_default();
            serde_json::json!({"status": status, "models": models})
        })
        .collect();
    axum::response::Html(super::static_files::page(
        serde_json::json!({
            "page": "home", "modelReady": !voices.is_empty(), "voices": voices, "status": message, "providers": providers,
        }),
        &nonce,
    ))
    .into_response()
}
