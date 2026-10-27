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
    /// Text-normalization capabilities. Coverage of pronunciation rules is
    /// reported separately from provider speech-language support.
    pub normalization: NormalizationInfo,
}

/// `normalization` block of `GET /api/info`.
#[derive(Serialize)]
pub struct NormalizationInfo {
    pub ruleset_version: &'static str,
    /// Effective configured default mode (`auto` or `off`).
    pub default_mode: sonicboom::tts::normalization::NormalizationMode,
    pub languages: Vec<sonicboom::tts::normalization::LanguageCoverage>,
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
        normalization: NormalizationInfo {
            ruleset_version: sonicboom::tts::normalization::RULESET_VERSION,
            default_mode: config.tts.normalization.effective_mode(),
            languages: sonicboom::tts::normalization::language_coverage(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn info_reports_normalization_coverage_and_effective_mode() {
        let mut config = crate::api::tests::test_config();
        let state = crate::api::tests::test_state(
            config.clone(),
            Arc::new(crate::auth::store::TokenStore::empty()),
            Arc::new(None),
        );
        let body = serde_json::to_value(get_info(State(state)).await.0).unwrap();
        let block = &body["normalization"];
        assert_eq!(block["ruleset_version"], "tn-v1");
        assert_eq!(block["default_mode"], "auto");
        let spanish = block["languages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["language"] == "es")
            .unwrap();
        assert_eq!(
            spanish["locales"],
            serde_json::json!(["es-ES", "es-MX", "es-PE"])
        );
        assert_eq!(spanish["default_locale"], "es-ES");
        assert!(spanish["rules"].is_string());
        assert!(spanish["features"]["cardinal_numbers"].is_string());

        config.tts.normalization.enabled = false;
        let state = crate::api::tests::test_state(
            config,
            Arc::new(crate::auth::store::TokenStore::empty()),
            Arc::new(None),
        );
        let body = serde_json::to_value(get_info(State(state)).await.0).unwrap();
        assert_eq!(body["normalization"]["default_mode"], "off");
    }
}
