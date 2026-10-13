//! Persistent optional-provider singletons.
//!
//! The Qwen and GPT-SoVITS models are large (gigabytes of weights); they
//! must load once and stay resident between requests — never reload per
//! request. This pool owns the two singletons and syncs their
//! configuration from the live `AppConfig` on every registry build, so
//! hot-reload applies without restarts and a failed reload keeps the last
//! healthy state.

use std::sync::Arc;

use super::TtsProviderRegistry;
use super::gptsovits::{GptSovitsConfig, GptSovitsProvider};
use super::qwen::{QwenConfig, QwenProvider};
use super::supertonic::SupertonicProvider;

/// Resident optional providers shared by all requests.
pub struct ProviderPool {
    /// Qwen3-TTS singleton (model loads once, prompts cached per profile).
    pub qwen: Arc<QwenProvider>,
    /// GPT-SoVITS singleton (model + reference state behind one lock).
    pub gpt: Arc<GptSovitsProvider>,
}

impl Default for ProviderPool {
    fn default() -> Self {
        Self {
            qwen: QwenProvider::new(QwenConfig::default()),
            gpt: GptSovitsProvider::new(GptSovitsConfig::default()),
        }
    }
}

impl ProviderPool {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sync singleton configuration from freshly mapped provider configs.
    /// Only genuinely changed providers are reconfigured (which drops
    /// their loaded model); unchanged providers keep serving.
    pub fn sync_from(&self, qwen: QwenConfig, gpt: GptSovitsConfig) {
        self.qwen.sync_config(qwen);
        self.gpt.sync_config(gpt);
    }

    /// Kick background download + load for enabled providers that have no
    /// loaded model and no recorded error (startup and hot-reload path).
    /// Never blocks. `hf_token` covers gated repositories; the pinned
    /// public bundles need none.
    pub fn kick_ensure_if_needed(&self, hf_token: Option<String>) {
        self.qwen.kick_ensure_if_needed(hf_token.clone());
        self.gpt.kick_ensure_if_needed(hf_token);
    }

    /// Build a request-scoped registry around the resident singletons plus
    /// a Supertonic adapter bound to the current handle (if any).
    pub fn registry(&self, supertonic: Arc<SupertonicProvider>) -> TtsProviderRegistry {
        let registry = TtsProviderRegistry::new();
        registry.register(supertonic);
        registry.register(Arc::clone(&self.qwen) as Arc<dyn super::TtsProvider>);
        registry.register(Arc::clone(&self.gpt) as Arc<dyn super::TtsProvider>);
        registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tts::providers::TtsProvider;

    #[test]
    fn pool_registers_all_three_providers() {
        let pool = ProviderPool::new();
        let supertonic = Arc::new(SupertonicProvider::new(std::path::PathBuf::from(
            "./models",
        )));
        let registry = pool.registry(supertonic);
        assert_eq!(
            registry.provider_ids(),
            vec!["gpt-sovits", "qwen3", "supertonic"]
        );
    }

    #[test]
    fn sync_keeps_healthy_singletons_when_config_is_unchanged() {
        let pool = ProviderPool::new();
        pool.sync_from(QwenConfig::default(), GptSovitsConfig::default());
        // No loaded model, no error, disabled by default: sync is a no-op
        // that must not record failures.
        assert!(!pool.qwen.status().enabled);
        assert!(!pool.gpt.status().enabled);
    }
}
