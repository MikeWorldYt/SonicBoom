//! Provider-aware HTTP surface: registry wiring, status and discovery.
//!
//! Built per request from live configuration plus the current Supertonic
//! handle snapshot, so config hot reload applies without restarts and a
//! failed optional provider can never take down Supertonic.

use std::sync::Arc;

use sonicboom::tts::model::ModelHandle;
use sonicboom::tts::providers::TtsProviderRegistry;
use sonicboom::tts::providers::gptsovits::GptSovitsConfig;
use sonicboom::tts::providers::supertonic::SupertonicProvider;
use sonicboom::tts::providers::types::DevicePreference;

#[cfg(test)]
use crate::config::model::AppConfig;

fn parse_device(raw: &str) -> DevicePreference {
    DevicePreference::parse(raw).unwrap_or(DevicePreference::Auto)
}

#[cfg(feature = "qwen-gguf")]
pub fn map_qwen_gguf_config(
    gguf: &crate::config::model::Qwen3GgufConfig,
) -> sonicboom::tts::providers::qwen_gguf::provider::QwenGgufConfig {
    use sonicboom::tts::providers::qwen_gguf::worker::Device;
    sonicboom::tts::providers::qwen_gguf::provider::QwenGgufConfig {
        enabled: gguf.enabled,
        model_dir: gguf.model_dir.clone(),
        device: match gguf.device.trim().to_ascii_lowercase().as_str() {
            "cpu" => Device::Cpu,
            "vulkan" => Device::Vulkan,
            "cuda" => Device::Cuda,
            _ => Device::Auto,
        },
        gpu_layers: gguf.gpu_layers,
        threads: gguf.threads.max(1),
        max_tokens: gguf.max_tokens.max(1),
        cache_capacity: gguf.cache_entries.max(1),
    }
}

pub fn map_gpt_config(gpt: &crate::config::model::GptSovitsConfig) -> GptSovitsConfig {
    GptSovitsConfig {
        enabled: gpt.enabled,
        model_dir: gpt.model_dir.clone(),
        model_id: gpt.model_id.clone(),
        revision: gpt.revision.clone(),
        device: parse_device(&gpt.device),
        cache_capacity: gpt.cache_entries.max(1),
        top_k: gpt.top_k,
        top_p: gpt.top_p,
        temperature: gpt.temperature,
        repetition_penalty: gpt.repetition_penalty,
    }
}

/// Build a request-scoped registry from live config, the resident provider
/// pool, and the current Supertonic handle (if any). Singleton configs sync
/// from the live configuration first (hot reload without restarts), and an
/// enabled-but-unloaded provider kicks its background download + load.
/// Optional providers are always registered; when disabled or uninstalled
/// they report that state explicitly and reject synthesis with actionable
/// errors. One failing optional provider can never take down Supertonic.
#[cfg(test)]
pub fn build_registry(
    config: &AppConfig,
    supertonic_handle: Option<Arc<ModelHandle>>,
    pool: &sonicboom::tts::providers::ProviderPool,
) -> TtsProviderRegistry {
    pool.sync_from(map_gpt_config(&config.providers.gpt_sovits));
    #[cfg(feature = "qwen-gguf")]
    pool.sync_gguf_config(map_qwen_gguf_config(&config.providers.qwen3_gguf));
    // Hot-reload path: no HF token at request time (public bundles need
    // none; gated bundles report an explicit auth error in status).
    pool.kick_ensure_if_needed(None);

    let supertonic = Arc::new(SupertonicProvider::new(config.model.cache_dir.clone()));
    if let Some(handle) = supertonic_handle {
        supertonic.attach_ready(handle, &config.model.revision);
    }
    pool.registry(supertonic)
}

/// Versioned request registry: stale request snapshots cannot roll back a
/// resident provider after another request adopted a newer configuration.
pub async fn build_registry_for_state(state: &crate::AppState) -> TtsProviderRegistry {
    let (generation, config) = state.config.versioned_snapshot().await;
    state
        .providers
        .sync_versioned(generation, map_gpt_config(&config.providers.gpt_sovits));
    #[cfg(feature = "qwen-gguf")]
    state
        .providers
        .sync_gguf_config(map_qwen_gguf_config(&config.providers.qwen3_gguf));
    state.providers.kick_ensure_if_needed(None);
    let supertonic = Arc::new(SupertonicProvider::new(config.model.cache_dir.clone()));
    if let Some(handle) = supertonic_handle(state).await {
        supertonic.attach_ready(handle, &config.model.revision);
    }
    state.providers.registry(supertonic)
}

/// Snapshot the live Supertonic handle (if the model is ready).
pub async fn supertonic_handle(state: &crate::AppState) -> Option<Arc<ModelHandle>> {
    let status = state.model_status.read().await;
    match &*status {
        sonicboom::tts::ModelStatus::Ready(handle) => Some(Arc::clone(handle)),
        _ => None,
    }
}

/// `GET /api/providers`: authenticated capability + status discovery.
pub async fn get_providers(
    _token: crate::auth::AuthenticatedToken,
    axum::extract::State(state): axum::extract::State<crate::AppState>,
) -> impl axum::response::IntoResponse {
    use axum::http::{StatusCode, header};
    let registry = build_registry_for_state(&state).await;
    let capabilities = [
        registry.get("supertonic").map(|p| p.capabilities()),
        registry.get("gpt-sovits").map(|p| p.capabilities()),
    ]
    .into_iter()
    .chain(
        cfg!(feature = "qwen-gguf").then(|| registry.get("qwen3-gguf").map(|p| p.capabilities())),
    )
    .collect::<Vec<_>>();
    let body = serde_json::json!({
        "providers": registry.statuses(),
        "capabilities": capabilities,
    })
    .to_string();
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        body,
    )
}

/// `GET /api/providers/{id}/status`: one provider's lifecycle status.
pub async fn get_provider_status(
    _token: crate::auth::AuthenticatedToken,
    axum::extract::State(state): axum::extract::State<crate::AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<impl axum::response::IntoResponse, crate::error::AppError> {
    use axum::http::{StatusCode, header};
    let registry = build_registry_for_state(&state).await;
    match registry.get(id.trim()) {
        Some(provider) => {
            let body = serde_json::to_string(&provider.status())
                .map_err(|e| crate::error::AppError::internal(e.to_string()))?;
            Ok((
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json")],
                body,
            ))
        }
        None => Err(crate::error::AppError::BadRequest(format!(
            "unknown provider '{id}' (expected one of: supertonic, gpt-sovits{})",
            if cfg!(feature = "qwen-gguf") {
                ", qwen3-gguf"
            } else {
                ""
            }
        ))),
    }
}

/// Map a typed provider error onto the existing HTTP error policy.
pub fn provider_error_to_app(
    error: sonicboom::tts::providers::ProviderError,
) -> crate::error::AppError {
    use sonicboom::tts::providers::ProviderErrorKind;
    match error.kind() {
        ProviderErrorKind::UnsupportedModel
        | ProviderErrorKind::UnsupportedLanguage
        | ProviderErrorKind::InvalidVoice => {
            crate::error::AppError::BadRequest(error.message().to_string())
        }
        ProviderErrorKind::Busy => {
            crate::error::AppError::TooManyRequests(error.message().to_string())
        }
        ProviderErrorKind::ModelMissing
        | ProviderErrorKind::Loading
        | ProviderErrorKind::DeviceUnavailable => {
            crate::error::AppError::ServiceUnavailable(error.message().to_string())
        }
        ProviderErrorKind::InferenceFailed if error.message().contains("maximum frame limit without EOS") => {
            crate::error::AppError::ServiceUnavailable("Generation reached the configured frame limit without EOS; audio was withheld. Use shorter text or increase providers.qwen3_gguf.max_tokens within its validated range.".into())
        }
        ProviderErrorKind::InferenceFailed => {
            crate::error::AppError::internal(error.message().to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_lists_providers_with_explicit_devices() {
        let config = AppConfig::default();
        let pool = sonicboom::tts::providers::ProviderPool::new();
        let registry = build_registry(&config, None, &pool);
        // The Candle provider is gone: no "qwen3" id may appear.
        assert!(!registry.provider_ids().contains(&"qwen3".to_string()));
        #[cfg(feature = "qwen-gguf")]
        assert_eq!(
            registry.provider_ids(),
            vec!["gpt-sovits", "qwen3-gguf", "supertonic"]
        );
        #[cfg(not(feature = "qwen-gguf"))]
        assert_eq!(registry.provider_ids(), vec!["gpt-sovits", "supertonic"]);
        for status in registry.statuses() {
            if status.provider_id == "supertonic" {
                assert_eq!(status.effective_device.as_deref(), Some("cpu"));
            } else {
                // Optional providers are disabled by default.
                assert!(!status.enabled);
            }
        }
    }

    #[test]
    fn unknown_provider_status_is_rejected() {
        let config = AppConfig::default();
        let pool = sonicboom::tts::providers::ProviderPool::new();
        let registry = build_registry(&config, None, &pool);
        assert!(registry.get("nope").is_none());
    }

    #[test]
    fn enabling_a_provider_without_weights_reports_not_installed() {
        // Hot reload path: flipping `enabled` takes effect on the next
        // request-scoped registry build; without installed weights the
        // provider reports `not_installed` (never a silent fallback) and
        // Supertonic keeps serving.
        // Uses an isolated empty dir: the workspace `./models` may hold
        // real downloaded weights, which must not flip this expectation.
        let dir = std::env::temp_dir().join(format!("gpt-no-weights-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut config = AppConfig::default();
        config.providers.gpt_sovits.enabled = true;
        config.providers.gpt_sovits.model_dir = dir.clone();
        // Built without the background kick on purpose: kicking would
        // start a real download in this test.
        let pool = sonicboom::tts::providers::ProviderPool::new();
        pool.sync_from(map_gpt_config(&config.providers.gpt_sovits));
        let supertonic =
            std::sync::Arc::new(SupertonicProvider::new(config.model.cache_dir.clone()));
        let registry = pool.registry(supertonic);
        let status = registry.get("gpt-sovits").unwrap().status();
        assert!(status.enabled);
        assert_eq!(
            status.state,
            sonicboom::tts::providers::types::ProviderStatusState::NotInstalled
        );
        assert_eq!(status.effective_device.as_deref(), Some("cpu"));
        assert!(registry.get("supertonic").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(feature = "qwen-gguf")]
    #[test]
    fn enabling_gguf_without_weights_reports_not_installed() {
        // Same hot-reload contract as the other native providers: enabled
        // but no verified bundle means `not_installed`, and the 1.7B model
        // stays routable to its owning provider while Supertonic keeps serving.
        let dir =
            std::env::temp_dir().join(format!("qwen-gguf-no-weights-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut config = AppConfig::default();
        config.providers.qwen3_gguf.enabled = true;
        config.providers.qwen3_gguf.model_dir = dir.clone();
        let pool = sonicboom::tts::providers::ProviderPool::new();
        pool.sync_gguf_config(map_qwen_gguf_config(&config.providers.qwen3_gguf));
        let supertonic =
            std::sync::Arc::new(SupertonicProvider::new(config.model.cache_dir.clone()));
        let registry = pool.registry(supertonic);
        let status = registry.get("qwen3-gguf").unwrap().status();
        assert!(status.enabled);
        assert_eq!(
            status.state,
            sonicboom::tts::providers::types::ProviderStatusState::NotInstalled
        );
        assert_eq!(
            registry.resolve_model("qwen3-tts-1.7b-base").unwrap().id(),
            "qwen3-gguf"
        );
        assert!(registry.get("supertonic").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
