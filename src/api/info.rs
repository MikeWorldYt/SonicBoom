//! Public server info endpoint (spec §31).
//!
//! `GET /api/info` describes the server without
//! exposing secrets: authentication mode, feature
//! flags, configuration revision, and model
//! status. The browser GUI uses it to discover
//! whether an API token is required (spec §32).

use axum::{Json, extract::State};
use serde::Serialize;

use crate::{AppState, config::AuthMode, tts::ModelStatus};

/// Response for `GET /api/info`.
#[derive(Serialize)]
pub struct InfoResponse {
    pub version: String,
    /// `desktop` for GUI builds, `server` for
    /// headless builds.
    pub mode: &'static str,
    pub setup_complete: bool,
    pub auth: AuthInfo,
    pub features: Features,
    pub config_revision: u64,
    pub model: ModelInfo,
    pub synthesis: serde_json::Value,
}

#[derive(Serialize)]
pub struct AuthInfo {
    pub mode: String,
    /// Whether API requests must present a
    /// bearer token.
    pub required: bool,
}

#[derive(Serialize)]
pub struct Features {
    pub playback: bool,
    pub audio_devices: bool,
    pub hot_reload: bool,
}

#[derive(Serialize)]
pub struct ModelInfo {
    pub status: String,
}

/// `GET /api/info`
pub async fn get_info(State(state): State<AppState>) -> Json<InfoResponse> {
    let config = state.config.get().await;
    let model_status = state.model_status.read().await;
    let model_status = match &*model_status {
        ModelStatus::Idle => "idle",
        ModelStatus::Downloading { .. } => "downloading",
        ModelStatus::Loading => "loading",
        ModelStatus::Ready(_) => "ready",
        ModelStatus::Failed(_) => "failed",
    };
    Json(InfoResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        mode: if cfg!(feature = "gui") {
            "desktop"
        } else {
            "server"
        },
        setup_complete: config.setup_complete,
        auth: AuthInfo {
            mode: match config.server.auth_mode {
                AuthMode::Local => "local",
                AuthMode::Token => "token",
                AuthMode::None => "none",
            }
            .to_string(),
            required: matches!(config.server.auth_mode, AuthMode::Token),
        },
        features: Features {
            playback: cfg!(feature = "playback"),
            audio_devices: cfg!(feature = "playback"),
            hot_reload: true,
        },
        config_revision: config.revision,
        synthesis: {
            use sonicboom::tts::providers::types::SamplingParams as Bounds;
            serde_json::json!({
                "max_text_chars": config.tts.max_text_length,
                "max_body_bytes": config.tts.max_body_bytes,
                "sampling": {
                    "provider": "qwen3-gguf",
                    "seed_max": u64::MAX.to_string(),
                    "temperature": { "min": Bounds::TEMPERATURE_MIN, "max": Bounds::TEMPERATURE_MAX },
                    "top_k": { "min": 0, "max": Bounds::TOP_K_MAX, "integer": true },
                    "top_p": { "min": 0, "max": Bounds::TOP_P_MAX, "exclusive_min": true },
                    "repetition_penalty": { "min": Bounds::REPETITION_MIN, "max": Bounds::REPETITION_MAX }
                }
            })
        },
        model: ModelInfo {
            status: model_status.to_string(),
        },
    })
}
