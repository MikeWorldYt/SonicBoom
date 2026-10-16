//! Persistent optional-provider singletons.
//!
//! The Qwen, GPT-SoVITS and GGUF/ONNX models are large (gigabytes of
//! weights); they must load once and stay resident between requests —
//! never reload per request. This pool owns the singletons and syncs their
//! configuration from the live `AppConfig` on every registry build, so
//! hot-reload applies without restarts and a failed reload keeps the last
//! healthy state.

use std::sync::Arc;

use super::TtsProviderRegistry;
use super::gptsovits::{GptSovitsConfig, GptSovitsProvider};
use super::qwen::{QwenConfig, QwenProvider};
#[cfg(feature = "qwen-gguf")]
use super::qwen_gguf::provider::{QwenGgufConfig, QwenGgufProvider};
use super::supertonic::SupertonicProvider;

type DesiredProviders = Option<(u64, QwenConfig, GptSovitsConfig)>;

/// Resident optional providers shared by all requests.
pub struct ProviderPool {
    desired: Arc<std::sync::Mutex<DesiredProviders>>,
    retrying: Arc<std::sync::atomic::AtomicBool>,
    /// Qwen3-TTS singleton (model loads once, prompts cached per profile).
    pub qwen: Arc<QwenProvider>,
    /// GPT-SoVITS singleton (model + reference state behind one lock).
    pub gpt: Arc<GptSovitsProvider>,
    /// Qwen3-TTS 1.7B GGUF/ONNX singleton (native worker, cached
    /// reference conditioning). Compiled only with `qwen-gguf`.
    #[cfg(feature = "qwen-gguf")]
    pub qwen_gguf: Arc<QwenGgufProvider>,
}

impl Default for ProviderPool {
    fn default() -> Self {
        Self {
            desired: Arc::new(std::sync::Mutex::new(None)),
            retrying: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            qwen: QwenProvider::new(QwenConfig::default()),
            gpt: GptSovitsProvider::new(GptSovitsConfig::default()),
            #[cfg(feature = "qwen-gguf")]
            qwen_gguf: QwenGgufProvider::new(QwenGgufConfig::default()),
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

    /// Sync the GGUF singleton configuration (reconfiguration drops its
    /// loaded worker; unchanged configs keep serving). Kept separate from
    /// [`Self::sync_from`] so builds without `qwen-gguf` are untouched.
    /// There is no background downloader yet: an enabled provider with a
    /// verified bundle loads explicitly via `load`, never implicitly here.
    #[cfg(feature = "qwen-gguf")]
    pub fn sync_gguf_config(&self, gguf: QwenGgufConfig) {
        self.qwen_gguf.sync_config(gguf);
    }

    /// Reject request-scoped snapshots older than the most recently adopted
    /// server configuration. Equal versions retry deferred busy-model changes.
    pub fn sync_versioned(&self, generation: u64, qwen: QwenConfig, gpt: GptSovitsConfig) {
        let mut current = self.desired.lock().unwrap_or_else(|e| e.into_inner());
        if current
            .as_ref()
            .is_some_and(|(current, _, _)| generation < *current)
        {
            return;
        }
        *current = Some((generation, qwen.clone(), gpt.clone()));
        self.sync_from(qwen, gpt);
        drop(current);
        self.retry_deferred();
    }
    fn retry_deferred(&self) {
        use std::sync::atomic::Ordering;
        if !self.qwen.reload_pending() && !self.gpt.reload_pending() {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        if self.retrying.swap(true, Ordering::AcqRel) {
            return;
        }
        let desired = self.desired.clone();
        let retrying = self.retrying.clone();
        let qwen = self.qwen.clone();
        let gpt = self.gpt.clone();
        runtime.spawn(async move {
            struct Reset(Arc<std::sync::atomic::AtomicBool>, bool);
            impl Drop for Reset {
                fn drop(&mut self) {
                    if self.1 {
                        self.0.store(false, Ordering::Release);
                    }
                }
            }
            let mut reset = Reset(retrying, true);
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                let current = desired.lock().unwrap_or_else(|e| e.into_inner());
                if let Some((_, q, g)) = current.as_ref() {
                    qwen.sync_config(q.clone());
                    gpt.sync_config(g.clone());
                }
                qwen.kick_ensure_if_needed(None);
                gpt.kick_ensure_if_needed(None);
                if !qwen.reload_pending() && !gpt.reload_pending() {
                    // Publish idle while holding the same lock used by new
                    // versions; a concurrent update cannot lose its retry.
                    reset.0.store(false, Ordering::Release);
                    reset.1 = false;
                    break;
                }
            }
        });
    }

    /// Kick background download + load for enabled providers that have no
    /// loaded model and no recorded error (startup and hot-reload path).
    /// Never blocks. `hf_token` covers gated repositories; the pinned
    /// public bundles need none.
    pub fn kick_ensure_if_needed(&self, hf_token: Option<String>) {
        self.qwen.kick_ensure_if_needed(hf_token.clone());
        self.gpt.kick_ensure_if_needed(hf_token.clone());
        #[cfg(feature = "qwen-gguf")]
        self.qwen_gguf.kick_ensure_if_needed(hf_token);
    }

    /// Build a request-scoped registry around the resident singletons plus
    /// a Supertonic adapter bound to the current handle (if any).
    pub fn registry(&self, supertonic: Arc<SupertonicProvider>) -> TtsProviderRegistry {
        let registry = TtsProviderRegistry::new();
        registry.register(supertonic);
        registry.register(Arc::clone(&self.qwen) as Arc<dyn super::TtsProvider>);
        registry.register(Arc::clone(&self.gpt) as Arc<dyn super::TtsProvider>);
        #[cfg(feature = "qwen-gguf")]
        registry.register(Arc::clone(&self.qwen_gguf) as Arc<dyn super::TtsProvider>);
        registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tts::providers::TtsProvider;

    #[test]
    fn older_configuration_cannot_restore_a_retired_provider() {
        let pool = ProviderPool::new();
        let enabled = QwenConfig {
            enabled: true,
            ..Default::default()
        };
        pool.sync_versioned(10, enabled.clone(), GptSovitsConfig::default());
        pool.sync_versioned(11, QwenConfig::default(), GptSovitsConfig::default());
        pool.sync_versioned(10, enabled, GptSovitsConfig::default());
        assert!(!pool.qwen.status().enabled);
    }
    #[test]
    fn pool_registers_all_three_providers() {
        let pool = ProviderPool::new();
        let supertonic = Arc::new(SupertonicProvider::new(std::path::PathBuf::from(
            "./models",
        )));
        let registry = pool.registry(supertonic);
        #[cfg(feature = "qwen-gguf")]
        assert_eq!(
            registry.provider_ids(),
            vec!["gpt-sovits", "qwen3", "qwen3-gguf", "supertonic"]
        );
        #[cfg(not(feature = "qwen-gguf"))]
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
