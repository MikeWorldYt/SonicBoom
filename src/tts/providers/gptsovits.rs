//! GPT-SoVITS ONNX voice-cloning provider.
//!
//! Upstream implementation under review:
//! `https://github.com/bailu-obj/gpt-sovits-onnx-rs` at
//! `204ae4abcd218113aad86f6f9a66bdba92adcfd7` (pinned dependency
//! revision, upstream HEAD at review time 2026-10-13). Rust ONNX runtime;
//! v2/v2Pro/v2ProPlus; CPU sessions; reference WAV plus transcript.
//!
//! This provider runs in-process with no Python and never shells out:
//! bundles load through `TTSModel::new`, references through
//! `process_reference_sync`, audio through `synthesize_sync`. The single
//! shared ONNX Runtime (identical `ort =2.0.0-rc.13` as the rest of
//! SonicBoom) serves Supertonic and GPT-SoVITS with one initialization.
//!
//! Constraints honored here: only v2/v2Pro/v2ProPlus ONNX bundles
//! (`vits.onnx`, `ssl.onnx`, `t2s_encoder.onnx`, `t2s_fs_decoder.onnx`,
//! `t2s_s_decoder.onnx`, optional `bert.onnx`/`g2pW.onnx`/`g2p_en/`/`sv.onnx`).
//! Raw `.pth` checkpoints are rejected. Only Chinese (`zh`) and English
//! (`en`) are supported — Japanese (`ja`) and Spanish (`es`) return explicit
//! unsupported-language errors (never Chinese phonemes, never a Supertonic
//! fallback). No V5 claim. CPU execution provider only: SonicBoom's `rocm`/
//! `cuda` ORT flags do not accelerate this provider, and AMD `gfx1013` is
//! CPU-only here.
//!
//! Concurrency: upstream `TTSModel` takes `&mut self` for reference and
//! synthesis, so this provider holds the model behind a mutex and performs
//! each `(prepare_reference + synthesize)` task as one atomic unit under a
//! single lock acquisition. No cross-user voice leakage.
//!
//! Without the `gpt-sovits` Cargo feature every synthesis path fails with
//! an explicit `model_missing` diagnostic (never silence, never fallback).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::TtsProvider;
use super::assets::PinnedFile;
use super::error::ProviderError;
use super::types::{
    CloneMode, DevicePreference, EffectiveDevice, PcmAudio, ProviderCapabilities, ProviderRequest,
    ProviderStatus, ProviderStatusState,
};

#[cfg(feature = "gpt-sovits")]
use gpt_sovits_onnx_rs::{LangId, PostprocessParams, SamplingParamsBuilder, TTSModel};

/// Reviewed upstream revision (pinned dependency revision).
pub const GPTSOVITS_UPSTREAM_REV: &str = "204ae4abcd218113aad86f6f9a66bdba92adcfd7";
/// Supported model IDs (exported ONNX bundles only). Only the v2Pro bundle
/// below has a verified manifest today; the other IDs route explicitly and
/// fail with guidance until their bundles are pinned.
pub const GPTSOVITS_MODELS: &[&str] = &[
    "gpt-sovits-v2-onnx",
    "gpt-sovits-v2pro-onnx",
    "gpt-sovits-v2proplus-onnx",
];
/// Only Chinese and English pipelines are implemented.
pub const GPTSOVITS_LANGUAGES: &[&str] = &["zh", "en"];
/// Required bundle members (validated before loading).
pub const GPTSOVITS_REQUIRED_FILES: &[&str] = &[
    "vits.onnx",
    "ssl.onnx",
    "t2s_encoder.onnx",
    "t2s_fs_decoder.onnx",
    "t2s_s_decoder.onnx",
];
/// Optional quality members: wired when present, reported when absent.
pub const GPTSOVITS_OPTIONAL_FILES: &[&str] = &[
    "bert.onnx",
    "g2pW.onnx",
    "g2p_en/encoder_model.onnx",
    "g2p_en/decoder_model.onnx",
    "sv.onnx",
];
/// Reference window recommended upstream (3–10 s enforced softly at 1–30 s).
pub const GPT_REF_MIN_SECS: f64 = 1.0;
pub const GPT_REF_MAX_SECS: f64 = 30.0;
/// Reference audio is staged to the upstream pipeline as 16 kHz mono WAV.
pub const GPT_REFERENCE_RATE: u32 = 16_000;

/// Verified v2Pro bundle source and pinned revision: the prebuilt package
/// recommended by the reviewed port
/// (`mikv39/gpt-sovits-onnx-custom`, prefix-free filenames).
pub const GPTSOVITS_V2PRO_REPO: &str = "mikv39/gpt-sovits-onnx-custom";
pub const GPTSOVITS_V2PRO_REVISION: &str = "69063da489c62f34fffc40acd10e2167623509aa";

/// Verified v2Pro manifest: (relative path, SHA-256 hex, size in bytes).
/// Hashes are computed from the downloaded bytes before the smoke test runs.
pub const GPTSOVITS_V2PRO_MANIFEST: &[(&str, &str, u64)] = &[
    (
        "vits.onnx",
        "bfafa7fe2a98cb90cd779deea5fe1dbcf5d480d8103b4fed8f42fd3cd147083b",
        262655362,
    ),
    (
        "ssl.onnx",
        "7abea7075478c11bd61f5353ad046e9ebb0224c82a49d7e77f7838d88687918e",
        377631708,
    ),
    (
        "t2s_encoder.onnx",
        "c4c3afd21d051ebc19f5db63122ba5468c70d756257000cad39f749e5ee20397",
        7876155,
    ),
    (
        "t2s_fs_decoder.onnx",
        "8727430411e2c681133823fc495a35a16176fe19ce1857a8331f71d5ed84b894",
        318911534,
    ),
    (
        "t2s_s_decoder.onnx",
        "f97a153a985fbe66361dcb7e14b10dd40bc9fb12a05978f0b9888309c5d78f4d",
        315283143,
    ),
    (
        "bert.onnx",
        "19c9d1bb52c6ba460cb282e7022624d69518d9f7e0223fa17452c58314440419",
        401805376,
    ),
    (
        "g2pW.onnx",
        "566db4c0805feabe2b39efccff0c6bea4c241feed98eb31749c9bb3728781382",
        391088627,
    ),
    (
        "sv.onnx",
        "afc235531204cf5bed34d84abcbb135b15509d31cd8f71036b863d20976f0bba",
        182622963,
    ),
    (
        "g2p_en/encoder_model.onnx",
        "5df81746fe1872b63aa120205ce267ed44163b7894a54e931a1d4b4b09568faa",
        6634844,
    ),
    (
        "g2p_en/decoder_model.onnx",
        "2c199ceaa241186259167a8e79c5ff3498609ee8fc01c28c8a3d76a351d33c3d",
        9999491,
    ),
];

/// Pinned download manifest for a supported model ID. Only the v2Pro
/// bundle is verified today; other variants fail with explicit guidance.
pub fn gptsovits_manifest(model_id: &str) -> Result<Vec<PinnedFile>, ProviderError> {
    match model_id {
        "gpt-sovits-v2pro-onnx" => Ok(GPTSOVITS_V2PRO_MANIFEST
            .iter()
            .map(|(path, sha256, size)| PinnedFile {
                path,
                url: PinnedFile::huggingface(GPTSOVITS_V2PRO_REPO, GPTSOVITS_V2PRO_REVISION, path),
                sha256,
                size: *size,
            })
            .collect()),
        "gpt-sovits-v2-onnx" | "gpt-sovits-v2proplus-onnx" => {
            Err(ProviderError::model_missing(format!(
                "no verified ONNX bundle manifest for '{model_id}' yet: only \
                 gpt-sovits-v2pro-onnx is pinned (see docs/providers.md). \
                 Raw .pth checkpoints are never loadable; export an ONNX bundle first"
            )))
        }
        other => Err(ProviderError::unsupported_model(
            "gpt-sovits",
            other,
            format!(
                "only v2/v2Pro/v2ProPlus ONNX bundles are supported; expected one of: {}",
                GPTSOVITS_MODELS.join(", ")
            ),
        )),
    }
}

/// GPT-SoVITS provider configuration (`[providers.gpt_sovits]`).
#[derive(Debug, Clone, PartialEq)]
pub struct GptSovitsConfig {
    pub enabled: bool,
    pub model_dir: PathBuf,
    pub model_id: String,
    pub revision: String,
    pub device: DevicePreference,
    pub cache_capacity: usize,
    pub top_k: usize,
    pub top_p: f32,
    pub temperature: f32,
    pub repetition_penalty: f32,
}

impl Default for GptSovitsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model_dir: PathBuf::from("./models/gpt-sovits-v2pro"),
            model_id: GPTSOVITS_MODELS[1].to_string(),
            revision: GPTSOVITS_V2PRO_REVISION.to_string(),
            device: DevicePreference::Auto,
            cache_capacity: 16,
            top_k: 4,
            top_p: 0.9,
            temperature: 1.0,
            repetition_penalty: 1.0,
        }
    }
}

struct GptState {
    loaded_model: Option<String>,
    load_error: Option<String>,
    ensuring: bool,
    download_progress: f32,
    /// A real network fetch started during this ensure pass. Hash
    /// verification alone advances `download_progress` too, so status
    /// lines consult this flag to say "verifying" instead of lying
    /// about "downloading" on warm boots.
    downloading: bool,
}

#[cfg(any(feature = "gpt-sovits", test))]
struct StagedReference(PathBuf);
#[cfg(any(feature = "gpt-sovits", test))]
impl Drop for StagedReference {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
#[cfg(any(feature = "gpt-sovits", test))]
impl std::ops::Deref for StagedReference {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}
#[cfg(any(feature = "gpt-sovits", test))]
impl AsRef<Path> for StagedReference {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

/// Native GPT-SoVITS ONNX provider.
pub struct GptSovitsProvider {
    config: Mutex<GptSovitsConfig>,
    pending_config: Mutex<Option<GptSovitsConfig>>,
    lifecycle: Mutex<()>,
    invalidated_reference: std::sync::atomic::AtomicBool,
    state: Mutex<GptState>,
    /// The native model. `TTSModel` requires `&mut self` for reference and
    /// synthesis, so every `(prepare_reference + synthesize)` unit runs
    /// under one acquisition of this mutex: request A's reference can never
    /// be replaced during request B's synthesis.
    #[cfg(feature = "gpt-sovits")]
    model: Mutex<Option<TTSModel>>,
    #[cfg(feature = "gpt-sovits")]
    last_reference: Mutex<Option<(String, String)>>,
}

#[cfg(feature = "gpt-sovits")]
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<TTSModel>();
};

impl GptSovitsProvider {
    pub fn new(config: GptSovitsConfig) -> Arc<Self> {
        Arc::new(Self {
            config: Mutex::new(config),
            pending_config: Mutex::new(None),
            lifecycle: Mutex::new(()),
            invalidated_reference: std::sync::atomic::AtomicBool::new(false),
            state: Mutex::new(GptState {
                loaded_model: None,
                load_error: None,
                ensuring: false,
                download_progress: 0.0,
                downloading: false,
            }),
            #[cfg(feature = "gpt-sovits")]
            model: Mutex::new(None),
            #[cfg(feature = "gpt-sovits")]
            last_reference: Mutex::new(None),
        })
    }

    #[cfg(feature = "gpt-sovits")]
    fn recover_poisoned(&self) {
        if let Err(std::sync::TryLockError::Poisoned(guard)) = self.lifecycle.try_lock() {
            let _lifecycle = guard.into_inner();
            *self.model.lock().unwrap_or_else(|e| e.into_inner()) = None;
            *self
                .last_reference
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = None;
            self.model.clear_poison();
            self.last_reference.clear_poison();
            self.lifecycle.clear_poison();
            if let Ok(mut state) = self.state.lock() {
                state.loaded_model = None;
                state.load_error = None;
            }
            tracing::error!("GPT model retired after panic; next ensure can safely reload");
        }
    }
    pub fn reconfigure(&self, config: GptSovitsConfig) {
        #[cfg(feature = "gpt-sovits")]
        self.recover_poisoned();
        let Ok(_lifecycle) = self.lifecycle.try_lock() else {
            if let Ok(mut pending) = self.pending_config.lock() {
                *pending = Some(config);
            }
            return;
        };
        if let Ok(mut pending) = self.pending_config.lock() {
            *pending = None;
        }
        let old = self.config_snapshot();
        let reload = old.enabled != config.enabled
            || old.model_dir != config.model_dir
            || old.model_id != config.model_id
            || old.revision != config.revision
            || old.device != config.device;
        if !reload {
            if let Ok(mut current) = self.config.lock() {
                *current = config;
            }
            return;
        }
        if let Ok(mut current) = self.config.lock() {
            *current = config;
        }
        #[cfg(feature = "gpt-sovits")]
        if let Ok(mut model) = self.model.lock() {
            *model = None;
            if let Ok(mut last) = self.last_reference.lock() {
                *last = None;
            }
        }
        if let Ok(mut state) = self.state.lock() {
            state.loaded_model = None;
            state.load_error = None;
        }
    }

    fn model_change_pending(&self) -> bool {
        let current = self.config_snapshot();
        self.pending_config
            .lock()
            .map(|p| {
                p.as_ref()
                    .is_some_and(|next| !Self::same_model(&current, next))
            })
            .unwrap_or(true)
    }
    pub fn reload_pending(&self) -> bool {
        self.pending_config
            .lock()
            .map(|p| p.is_some())
            .unwrap_or(true)
    }
    fn same_model(a: &GptSovitsConfig, b: &GptSovitsConfig) -> bool {
        a.enabled == b.enabled
            && a.model_dir == b.model_dir
            && a.model_id == b.model_id
            && a.revision == b.revision
            && a.device == b.device
    }
    fn config_snapshot(&self) -> GptSovitsConfig {
        self.config.lock().map(|c| c.clone()).unwrap_or_default()
    }

    /// Adopt `next` only when it differs (reconfigure drops the loaded
    /// model, so spurious resets would interrupt serving providers).
    pub fn sync_config(&self, next: GptSovitsConfig) {
        let changed = self
            .config
            .lock()
            .map(|current| *current != next)
            .unwrap_or(true);
        if changed || self.reload_pending() {
            self.reconfigure(next);
        }
    }

    fn check_model(model: &str) -> Result<(), ProviderError> {
        if GPTSOVITS_MODELS.contains(&model) {
            Ok(())
        } else if model.ends_with(".pth") {
            Err(ProviderError::unsupported_model(
                "gpt-sovits",
                model,
                "raw PyTorch checkpoints (.pth) are not loadable; \
                 export an ONNX bundle first (see docs/providers.md)",
            ))
        } else {
            Err(ProviderError::unsupported_model(
                "gpt-sovits",
                model,
                format!(
                    "only v2/v2Pro/v2ProPlus ONNX bundles are supported; expected one of: {}",
                    GPTSOVITS_MODELS.join(", ")
                ),
            ))
        }
    }

    fn check_language(language: &str) -> Result<(), ProviderError> {
        if GPTSOVITS_LANGUAGES.contains(&language) {
            Ok(())
        } else {
            // Japanese and Spanish are explicitly unsupported: no phonemizer
            // pipeline exists in the reviewed port (Lang::{Zh,En} only).
            Err(ProviderError::unsupported_language(
                "gpt-sovits",
                language,
                GPTSOVITS_LANGUAGES,
            ))
        }
    }

    /// Validate an exported ONNX bundle directory.
    pub fn validate_bundle(model_dir: &Path) -> Result<(), ProviderError> {
        let mut missing = Vec::new();
        for file in GPTSOVITS_REQUIRED_FILES {
            if !model_dir.join(file).is_file() {
                missing.push(file.to_string());
            }
        }
        if !missing.is_empty() {
            return Err(ProviderError::model_missing(format!(
                "gpt-sovits ONNX bundle incomplete in '{}': missing {} \
                 (see docs/providers.md for the export checklist)",
                model_dir.display(),
                missing.join(", ")
            )));
        }
        Ok(())
    }

    /// Optional quality members absent from the bundle (reported, not fatal:
    /// the pipeline falls back to zero-BERT/dictionary/CMUdict paths).
    pub fn missing_optionals(model_dir: &Path) -> Vec<String> {
        GPTSOVITS_OPTIONAL_FILES
            .iter()
            .filter(|f| !model_dir.join(f).is_file())
            .map(|s| s.to_string())
            .collect()
    }

    pub fn validate_reference(pcm: &[f32], sample_rate: u32) -> Result<(), ProviderError> {
        if pcm.iter().any(|s| !s.is_finite()) {
            return Err(ProviderError::invalid_voice(
                "reference samples must be finite",
            ));
        }
        if pcm.is_empty() {
            return Err(ProviderError::invalid_voice(
                "reference audio is empty; provide a 3-10 s authorized recording",
            ));
        }
        if !(8000..=192000).contains(&sample_rate) {
            return Err(ProviderError::invalid_voice(
                "reference sample rate must be 8000..=192000 Hz",
            ));
        }
        let secs = pcm.len() as f64 / f64::from(sample_rate);
        if secs < GPT_REF_MIN_SECS {
            return Err(ProviderError::invalid_voice(format!(
                "reference audio is too short ({secs:.1}s); provide 3-10 s (minimum {:.0}s)",
                GPT_REF_MIN_SECS
            )));
        }
        if secs > GPT_REF_MAX_SECS {
            return Err(ProviderError::invalid_voice(format!(
                "reference audio is too long ({secs:.1}s); keep it under {:.0}s",
                GPT_REF_MAX_SECS
            )));
        }
        Ok(())
    }

    fn reference_hash(pcm: &[f32]) -> String {
        use sha2::Digest;
        let mut hasher = sha2::Sha256::new();
        for sample in pcm {
            hasher.update(sample.to_le_bytes());
        }
        hex::encode(hasher.finalize())
    }

    /// Synchronous load: validates the bundle, then constructs all native
    /// ONNX sessions on the calling (blocking) thread. Downloading is never
    /// performed here. Without the `gpt-sovits` Cargo feature this reports
    /// the rebuild diagnostic instead of touching weights.
    fn load_native(&self, model_id: &str) -> Result<(), ProviderError> {
        let config = self.config_snapshot();
        self.load_expected(model_id, &config)
    }
    fn load_expected(
        &self,
        model_id: &str,
        expected: &GptSovitsConfig,
    ) -> Result<(), ProviderError> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| ProviderError::inference_failed("GPT lifecycle lock poisoned"))?;
        if self.model_change_pending() || !Self::same_model(&self.config_snapshot(), expected) {
            return Err(ProviderError::loading("obsolete GPT load discarded"));
        }
        if self
            .state
            .lock()
            .map(|s| s.loaded_model.is_some())
            .unwrap_or(false)
        {
            return Ok(());
        }
        Self::check_model(model_id)?;
        gptsovits_manifest(model_id)?;
        let config = self.config_snapshot();
        if !matches!(
            config.device,
            DevicePreference::Cpu | DevicePreference::Auto
        ) {
            return Err(ProviderError::device_unavailable(
                "provider 'gpt-sovits' supports CPU only ('cpu'/'auto'); \
                 GPU execution providers are experimental and unsupported",
            ));
        }
        Self::validate_bundle(&config.model_dir)?;
        #[cfg(feature = "gpt-sovits")]
        {
            super::assets::verify_local_files(&config.model_dir, &gptsovits_manifest(model_id)?)?;
            let _load_resource = super::worker::model_load_guard();
            let dir = &config.model_dir;
            let opt = |name: &str| {
                let path = dir.join(name);
                if path.is_file() { Some(path) } else { None }
            };
            let g2p_en = {
                let path = dir.join("g2p_en");
                if path.is_dir() { Some(path) } else { None }
            };
            let model = TTSModel::new(
                dir.join("vits.onnx"),
                dir.join("ssl.onnx"),
                dir.join("t2s_encoder.onnx"),
                dir.join("t2s_fs_decoder.onnx"),
                dir.join("t2s_s_decoder.onnx"),
                opt("bert.onnx"),
                opt("g2pW.onnx"),
                g2p_en,
                opt("sv.onnx"),
            )
            .map_err(|e| {
                ProviderError::inference_failed(format!("gpt-sovits native load failed: {e}"))
            })?;
            if self.model_change_pending() {
                return Err(ProviderError::loading("obsolete GPT native load discarded"));
            }
            if let Ok(mut slot) = self.model.lock() {
                *slot = Some(model);
                if let Ok(mut last) = self.last_reference.lock() {
                    *last = None;
                }
            }
        }
        #[cfg(feature = "gpt-sovits")]
        {
            if let Ok(mut state) = self.state.lock() {
                state.loaded_model = Some(model_id.to_string());
                state.load_error = None;
            }
            Ok(())
        }
        #[cfg(not(feature = "gpt-sovits"))]
        {
            let _ = (config, model_id);
            Err(ProviderError::model_missing(
                "gpt-sovits bundle validated but this build omits the native ONNX backend; \
                 rebuild with `--features gpt-sovits` (CPU) for voice cloning",
            ))
        }
    }

    /// Start download + load in the background (deduplicated). Returns
    /// immediately; progress surfaces through [`TtsProvider::status`].
    pub fn ensure_background(self: &Arc<Self>, hf_token: Option<String>) {
        {
            let mut state = match self.state.lock() {
                Ok(state) => state,
                Err(_) => return,
            };
            if state.ensuring {
                return;
            }
            #[cfg(feature = "gpt-sovits")]
            if state.loaded_model.is_some() {
                return;
            }
            state.ensuring = true;
            state.download_progress = 0.0;
            state.downloading = false;
        }
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let retry_token = hf_token.clone();
            let result = this.ensure_async(hf_token).await;
            if let Ok(mut state) = this.state.lock() {
                state.ensuring = false;
                state.downloading = false;
                match &result {
                    Ok(()) => state.load_error = None,
                    Err(error) if error.message().contains("obsolete") => state.load_error = None,
                    Err(error) => state.load_error = Some(error.message().to_string()),
                }
            }
            if result
                .as_ref()
                .is_err_and(|error| error.message().contains("obsolete"))
            {
                this.kick_ensure_if_needed(retry_token);
            }
            if let Err(error) = &result {
                tracing::error!("gpt-sovits background prepare failed: {error}");
            }
        });
    }

    async fn ensure_async(self: &Arc<Self>, hf_token: Option<String>) -> Result<(), ProviderError> {
        if !cfg!(feature = "gpt-sovits") {
            return Err(ProviderError::model_missing(
                "native backend is not compiled; rebuild with --features gpt-sovits",
            ));
        }
        let config = self.config_snapshot();
        if !config.enabled {
            return Err(ProviderError::model_missing(
                "gpt-sovits provider is disabled ([providers.gpt_sovits] enabled=false)",
            ));
        }
        let manifest = gptsovits_manifest(&config.model_id)?;
        let progress_self = Arc::clone(self);
        let download_self = Arc::clone(self);
        super::assets::ensure_pinned_files(
            "gpt-sovits",
            &config.model_dir,
            &manifest,
            hf_token.as_deref(),
            move |p| {
                if let Ok(mut state) = progress_self.state.lock() {
                    state.download_progress = p;
                }
            },
            move || {
                if let Ok(mut state) = download_self.state.lock() {
                    state.downloading = true;
                }
            },
            || false,
        )
        .await?;
        let loader = Arc::clone(self);
        let model_id = config.model_id.clone();
        tokio::task::spawn_blocking(move || loader.load_expected(&model_id, &config))
            .await
            .map_err(|e| {
                ProviderError::inference_failed(format!("gpt-sovits load task failed: {e}"))
            })?
    }

    /// Whether a native model is currently resident (recorded state plus
    /// a live handle; a dropped handle counts as unloaded so the next kick
    /// re-ensures instead of wedging on a stale flag).
    #[cfg(feature = "gpt-sovits")]
    fn is_resident(&self) -> bool {
        self.state
            .lock()
            .map(|s| s.loaded_model.is_some())
            .unwrap_or(false)
    }

    /// Trigger a background ensure when the provider is enabled but has no
    /// loaded model and no error yet (hot-reload path). Never blocks.
    pub fn kick_ensure_if_needed(self: &Arc<Self>, hf_token: Option<String>) {
        if self.reload_pending() {
            return;
        }
        #[cfg(feature = "gpt-sovits")]
        self.recover_poisoned();
        #[cfg(feature = "gpt-sovits")]
        let resident = self.is_resident();
        #[cfg(not(feature = "gpt-sovits"))]
        let resident = self
            .state
            .lock()
            .map(|s| s.loaded_model.is_some())
            .unwrap_or(false);
        let needs = {
            let config = self.config_snapshot();
            let state = match self.state.lock() {
                Ok(state) => state,
                Err(_) => return,
            };
            config.enabled && !resident && state.load_error.is_none() && !state.ensuring
        };
        if needs {
            self.ensure_background(hf_token);
        }
    }

    /// Stage request reference PCM as a 16 kHz mono WAV file for the
    /// upstream pipeline (which reads references from a file path).
    /// The file is owner-only and removed after use by the caller.
    /// (Also exercised by unit tests without the native backend.)
    #[cfg(any(feature = "gpt-sovits", test))]
    fn stage_reference_wav(
        pcm: &[f32],
        sample_rate: u32,
    ) -> Result<StagedReference, ProviderError> {
        let resampled =
            crate::tts::audio::resample_pcm_linear(pcm, sample_rate, GPT_REFERENCE_RATE);
        let dir = std::env::temp_dir().join("sonicboom-gpt-refs");
        std::fs::create_dir_all(&dir).map_err(|e| {
            ProviderError::inference_failed(format!("reference staging dir unavailable: {e}"))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
        #[cfg(target_os = "linux")]
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.take(1024).flatten() {
                let name = entry.file_name();
                let Some(name) = name.to_str() else {
                    continue;
                };
                let Some((pid, id)) = name
                    .strip_prefix("ref-")
                    .and_then(|s| s.strip_suffix(".wav"))
                    .and_then(|s| s.split_once('-'))
                else {
                    continue;
                };
                if let Ok(pid) = pid.parse::<u32>()
                    && uuid::Uuid::parse_str(id).is_ok()
                    && !Path::new("/proc").join(pid.to_string()).exists()
                {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        let path = StagedReference(dir.join(format!(
            "ref-{}-{}.wav",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        )));
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: GPT_REFERENCE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).map_err(|e| {
            ProviderError::inference_failed(format!("reference staging failed: {e}"))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        for sample in &resampled {
            writer
                .write_sample((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                .map_err(|e| {
                    ProviderError::inference_failed(format!("reference staging failed: {e}"))
                })?;
        }
        writer.finalize().map_err(|e| {
            ProviderError::inference_failed(format!("reference staging failed: {e}"))
        })?;
        Ok(path)
    }

    #[cfg(feature = "gpt-sovits")]
    fn require_loaded(&self) -> Result<(), ProviderError> {
        let loaded = self.model.lock().ok().is_some_and(|m| m.is_some());
        if loaded {
            return Ok(());
        }
        let state = self.state.lock().ok();
        match state {
            Some(state) if state.ensuring && state.download_progress >= 1.0 => Err(
                ProviderError::loading("GPT assets verified; native model is loading"),
            ),
            Some(state) if state.ensuring && state.downloading => {
                Err(ProviderError::loading(format!(
                    "gpt-sovits bundle is downloading ({:.0}% complete)",
                    state.download_progress * 100.0
                )))
            }
            Some(state) if state.ensuring => Err(ProviderError::loading(format!(
                "gpt-sovits bundle is verifying cache ({:.0}% complete)",
                state.download_progress * 100.0
            ))),
            Some(state) => match &state.load_error {
                Some(detail) => Err(ProviderError::inference_failed(format!(
                    "gpt-sovits model unavailable: {detail}"
                ))),
                None => Err(ProviderError::loading("gpt-sovits model is not loaded yet")),
            },
            _ => Err(ProviderError::loading("gpt-sovits model is not loaded yet")),
        }
    }
}

impl TtsProvider for GptSovitsProvider {
    fn id(&self) -> &'static str {
        "gpt-sovits"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider_id: "gpt-sovits".to_string(),
            models: GPTSOVITS_MODELS.iter().map(|s| s.to_string()).collect(),
            languages: GPTSOVITS_LANGUAGES.iter().map(|s| s.to_string()).collect(),
            clone_modes: vec!["icl".to_string()],
            devices: vec!["cpu".to_string(), "auto".to_string()],
            requires_reference_audio: true,
            requires_reference_transcript: true,
            sample_rate: Some(32_000),
        }
    }

    fn status(&self) -> ProviderStatus {
        let config = self.config_snapshot();
        if !config.enabled {
            return ProviderStatus {
                provider_id: "gpt-sovits".to_string(),
                enabled: false,
                state: ProviderStatusState::Disabled,
                active_model: None,
                effective_device: None,
                model_revision: Some(config.revision),
                detail: Some(
                    "provider disabled in configuration ([providers.gpt_sovits] enabled=false)"
                        .to_string(),
                ),
            };
        }
        let state = self.state.lock().ok();
        let loaded = state.as_ref().and_then(|s| s.loaded_model.clone());
        let error = state.as_ref().and_then(|s| s.load_error.clone());
        let ensuring = state.as_ref().is_some_and(|s| s.ensuring);
        let downloading = state.as_ref().is_some_and(|s| s.downloading);
        let progress = state.as_ref().map(|s| s.download_progress).unwrap_or(0.0);
        drop(state);
        #[cfg(not(feature = "gpt-sovits"))]
        let native = false;
        #[cfg(feature = "gpt-sovits")]
        let native = true;
        let installed = Self::validate_bundle(&config.model_dir).is_ok();
        let state = if loaded.is_some() {
            ProviderStatusState::Ready
        } else if error.is_some() {
            ProviderStatusState::Error
        } else if ensuring || installed {
            ProviderStatusState::Loading
        } else {
            ProviderStatusState::NotInstalled
        };
        let mut detail = error.or_else(|| {
            if loaded.is_some() {
                None
            } else if ensuring && progress >= 1.0 {
                Some("GPT assets verified; loading native model".into())
            } else if ensuring && downloading {
                Some(format!(
                    "gpt-sovits bundle downloading ({:.0}% complete)",
                    progress * 100.0
                ))
            } else if ensuring {
                Some(format!(
                    "gpt-sovits bundle verifying cache ({:.0}% complete)",
                    progress * 100.0
                ))
            } else if !native {
                Some(
                    "native ONNX backend not compiled; rebuild with `--features gpt-sovits`"
                        .to_string(),
                )
            } else if installed {
                Some("gpt-sovits bundle present; loading on first use".to_string())
            } else {
                Some(format!(
                    "gpt-sovits ONNX bundle not installed in '{}' (see docs/providers.md)",
                    config.model_dir.display()
                ))
            }
        });
        // Surface degraded-but-usable bundles honestly.
        if detail.is_none() {
            let missing = Self::missing_optionals(&config.model_dir);
            if !missing.is_empty() {
                detail = Some(format!(
                    "ready without optional quality members: {} (fallback phonemization active)",
                    missing.join(", ")
                ));
            }
        }
        ProviderStatus {
            provider_id: "gpt-sovits".to_string(),
            enabled: true,
            state,
            active_model: loaded.or(Some(config.model_id)),
            effective_device: Some(EffectiveDevice::Cpu.as_str().to_string()),
            model_revision: Some(config.revision),
            detail,
        }
    }

    fn load(&self, model_id: &str) -> Result<(), ProviderError> {
        Self::check_model(model_id)?;
        gptsovits_manifest(model_id)?;
        let config = self.config_snapshot();
        if !matches!(
            config.device,
            DevicePreference::Cpu | DevicePreference::Auto
        ) {
            return Err(ProviderError::device_unavailable(
                "provider 'gpt-sovits' supports CPU only ('cpu'/'auto'); \
                 GPU execution providers are experimental and unsupported",
            ));
        }
        Self::validate_bundle(&config.model_dir)?;
        self.load_native(model_id)
    }

    fn unload(&self) {
        let Ok(_lifecycle) = self.lifecycle.lock() else {
            return;
        };
        #[cfg(feature = "gpt-sovits")]
        if let Ok(mut model) = self.model.lock() {
            *model = None;
            if let Ok(mut last) = self.last_reference.lock() {
                *last = None;
            }
        }
        if let Ok(mut state) = self.state.lock() {
            state.loaded_model = None;
        }
    }

    fn invalidate_voice(&self, voice: &str) {
        self.invalidated_reference
            .store(true, std::sync::atomic::Ordering::Release);
        #[cfg(feature = "gpt-sovits")]
        if let Ok(mut last) = self.last_reference.try_lock()
            && last.as_ref().is_some_and(|(id, _)| id == voice)
        {
            *last = None;
        }
        #[cfg(not(feature = "gpt-sovits"))]
        let _ = voice;
    }

    fn prepare_reference(
        &self,
        model_id: &str,
        reference_pcm: &[f32],
        reference_rate: u32,
        transcript: Option<&str>,
        _mode: CloneMode,
    ) -> Result<String, ProviderError> {
        Self::check_model(model_id)?;
        Self::validate_reference(reference_pcm, reference_rate)?;
        match transcript.map(str::trim) {
            Some(t) if !t.is_empty() => {}
            _ => {
                return Err(ProviderError::invalid_voice(
                    "gpt-sovits requires the exact reference transcript \
                     (reference WAV alone is not sufficient)",
                ));
            }
        }
        let hash = Self::reference_hash(reference_pcm);
        let key = super::worker::ConditioningCache::cache_key(
            "gpt-sovits",
            &self.config_snapshot().revision,
            &hash,
            transcript,
            "icl",
        );
        // Eagerly warm the native reference state when a model is loaded;
        // otherwise the key is derived now and materializes at synthesis.
        #[cfg(feature = "gpt-sovits")]
        if self.is_resident() {
            let _lifecycle = crate::tts::control::lock_cancellable(&self.lifecycle)
                .map_err(ProviderError::inference_failed)?;
            let staged = Self::stage_reference_wav(reference_pcm, reference_rate)?;
            let result = (|| {
                let mut model = self.model.lock().map_err(|_| {
                    ProviderError::inference_failed("gpt-sovits model lock poisoned")
                })?;
                let slot = model
                    .as_mut()
                    .ok_or_else(|| ProviderError::loading("gpt-sovits model is not loaded"))?;
                if let Ok(mut last) = self.last_reference.lock() {
                    *last = None;
                }
                slot.process_reference_sync(&staged, transcript.unwrap_or(""), LangId::Auto)
                    .map_err(|e| {
                        ProviderError::inference_failed(format!(
                            "gpt-sovits reference preparation failed: {e}"
                        ))
                    })
            })();
            let _ = std::fs::remove_file(&staged);
            result?;
        }
        Ok(key)
    }

    fn synthesize(&self, request: &ProviderRequest) -> Result<PcmAudio, ProviderError> {
        Self::check_model(&request.model)?;
        Self::check_language(&request.language)?;
        if !matches!(
            request.device,
            DevicePreference::Cpu | DevicePreference::Auto
        ) {
            return Err(ProviderError::device_unavailable(
                "provider 'gpt-sovits' supports CPU only ('cpu'/'auto'); \
                 GPU execution providers are experimental and unsupported",
            ));
        }
        if request.text.trim().is_empty() {
            return Err(ProviderError::invalid_voice("text cannot be empty"));
        }
        let transcript = match request.reference_transcript.as_deref().map(str::trim) {
            Some(t) if !t.is_empty() => t.to_string(),
            _ => {
                return Err(ProviderError::invalid_voice(
                    "gpt-sovits requires the exact reference transcript \
                     (missing transcript is rejected; voice-profile synthesis \
                     must carry one)",
                ));
            }
        };
        // Reference audio travels with the request: concurrent voices can
        // never observe each other's data through a shared mutable slot.
        let reference = match request.reference_audio.as_ref() {
            Some(reference) => {
                Self::validate_reference(&reference.pcm, reference.sample_rate)?;
                reference
            }
            None => {
                return Err(ProviderError::invalid_voice(
                    "gpt-sovits cloning requires reference audio; synthesize through \
                     an authorized voice profile (POST /api/voices/clones)",
                ));
            }
        };
        let config = self.config_snapshot();
        if !config.enabled {
            return Err(ProviderError::model_missing(
                "gpt-sovits provider is disabled ([providers.gpt_sovits] enabled=false)",
            ));
        }
        if let Err(missing) = Self::validate_bundle(&config.model_dir) {
            if let Ok(mut state) = self.state.lock() {
                state.load_error = Some(missing.message().to_string());
            }
            return Err(missing);
        }
        #[cfg(not(feature = "gpt-sovits"))]
        {
            let _ = (transcript, reference, config);
            Err(ProviderError::model_missing(
                "gpt-sovits bundle validated but this build omits the native ONNX backend; \
                 rebuild with `--features gpt-sovits` (CPU) for voice cloning",
            ))
        }
        #[cfg(feature = "gpt-sovits")]
        {
            let _lifecycle = crate::tts::control::lock_cancellable(&self.lifecycle)
                .map_err(ProviderError::inference_failed)?;
            self.require_loaded()?;
            if let Some(control) = crate::tts::control::current() {
                control.set_context("gpt-sovits", &request.model, &request.voice, "cpu");
            }
            if self.reload_pending() {
                return Err(ProviderError::loading(
                    "model reconfiguration pending; retry after the provider is ready",
                ));
            }
            let config = self.config_snapshot();
            if !config.enabled {
                return Err(ProviderError::model_missing("GPT provider disabled"));
            }
            // One atomic unit under a single acquisition: this request's
            // reference is prepared and synthesized before any other
            // request can touch the model. Staged audio is owner-only and
            // always removed afterwards.
            let mut model = self
                .model
                .lock()
                .map_err(|_| ProviderError::inference_failed("gpt-sovits model lock poisoned"))?;
            let slot = model
                .as_mut()
                .ok_or_else(|| ProviderError::loading("gpt-sovits model is not loaded"))?;
            let reference_hash = format!(
                "{}:{}",
                reference.sample_rate,
                Self::reference_hash(&reference.pcm)
            );
            let key = super::worker::ConditioningCache::cache_key(
                "gpt-sovits",
                &config.revision,
                &reference_hash,
                Some(&transcript),
                "icl",
            );
            let mut last = self.last_reference.lock().map_err(|_| {
                ProviderError::inference_failed("GPT reference cache lock poisoned")
            })?;
            if self
                .invalidated_reference
                .swap(false, std::sync::atomic::Ordering::AcqRel)
            {
                *last = None;
            }
            let outcome = (|| {
                if crate::tts::control::current().is_some_and(|c| c.stopped()) {
                    return Err(ProviderError::inference_failed(
                        "inference cancelled or deadline exceeded",
                    ));
                }
                if last.as_ref() != Some(&(request.voice.clone(), key.clone())) {
                    *last = None;
                    let staged = Self::stage_reference_wav(&reference.pcm, reference.sample_rate)?;
                    let result = slot.process_reference_sync(&staged, &transcript, LangId::Auto);
                    let _ = std::fs::remove_file(&staged);
                    result.map_err(|e| {
                        ProviderError::inference_failed(format!(
                            "gpt-sovits reference preparation failed: {e}"
                        ))
                    })?;
                    *last = Some((request.voice.clone(), key));
                    tracing::debug!(cache_hit = false, "GPT reference conditioning");
                } else {
                    tracing::debug!(cache_hit = true, "GPT reference conditioning");
                }
                let mut sampling = SamplingParamsBuilder::new()
                    .temperature(config.temperature)
                    .repetition_penalty(config.repetition_penalty);
                if config.top_k > 0 {
                    sampling = sampling.top_k(config.top_k);
                }
                if config.top_p > 0.0 {
                    sampling = sampling.top_p(config.top_p);
                }
                if let Some(seed) = request.seed {
                    sampling = sampling.seed(seed);
                }
                let control = crate::tts::control::current();
                let mut samples = Vec::new();
                let mut rate = 0;
                if let Some(control) = &control {
                    control.update("generating", 0);
                }
                slot.synthesize_sentences_sync(
                    request.text.trim(),
                    sampling.build(),
                    LangId::Auto,
                    PostprocessParams::default(),
                    |spec, chunk| {
                        rate = spec.sample_rate;
                        if control.as_ref().is_some_and(|c| !c.update("generating", 0)) {
                            return false;
                        }
                        samples.extend(chunk);
                        samples.len() <= 32_000 * 600
                    },
                )
                .map_err(|e| {
                    ProviderError::inference_failed(format!("gpt-sovits synthesis failed: {e}"))
                })?;
                if control.as_ref().is_some_and(|c| c.stopped()) {
                    return Err(ProviderError::inference_failed(
                        "inference cancelled or deadline exceeded",
                    ));
                }
                if samples.len() > 32_000 * 600 {
                    return Err(ProviderError::inference_failed(
                        "GPT output exceeds ten-minute bound",
                    ));
                }
                Ok::<_, ProviderError>((rate, samples))
            })();
            let (sample_rate, samples) = outcome?;
            if samples.is_empty()
                || sample_rate == 0
                || samples.iter().any(|s| !s.is_finite())
                || !samples.iter().any(|s| s.abs() > 1e-4)
            {
                return Err(ProviderError::inference_failed(
                    "gpt-sovits synthesis produced no audio",
                ));
            }
            Ok(PcmAudio {
                samples,
                sample_rate,
                channels: 1,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tts::providers::types::ReferenceAudio;

    fn provider() -> Arc<GptSovitsProvider> {
        GptSovitsProvider::new(GptSovitsConfig {
            enabled: true,
            ..GptSovitsConfig::default()
        })
    }

    fn with_reference(mut request: ProviderRequest) -> ProviderRequest {
        request.reference_audio = Some(ReferenceAudio {
            pcm: vec![0.1; 48000],
            sample_rate: 16000,
        });
        request
    }

    #[test]
    fn manifest_gate_pins_only_the_verified_bundle() {
        assert!(gptsovits_manifest("gpt-sovits-v2pro-onnx").is_ok());
        let error = gptsovits_manifest("gpt-sovits-v2-onnx").unwrap_err();
        assert_eq!(error.code(), "model_missing");
        let error = gptsovits_manifest("gpt-sovits-v2proplus-onnx").unwrap_err();
        assert_eq!(error.code(), "model_missing");
    }

    #[test]
    fn japanese_and_spanish_are_explicitly_unsupported() {
        let p = provider();
        for lang in ["ja", "es", "yue"] {
            let mut request = ProviderRequest::new(GPTSOVITS_MODELS[1], "profile", "hello", lang);
            request.reference_transcript = Some("hello".to_string());
            let error = p.synthesize(&with_reference(request)).unwrap_err();
            assert_eq!(error.code(), "unsupported_language", "{lang}");
            assert_eq!(error.http_status(), 400);
        }
        // English passes language validation (then fails on the missing
        // bundle or unloaded model — both explicit, never a fallback).
        let mut request = ProviderRequest::new(GPTSOVITS_MODELS[1], "profile", "hello", "en");
        request.reference_transcript = Some("hello".to_string());
        let error = p.synthesize(&with_reference(request)).unwrap_err();
        assert!(
            matches!(error.code(), "model_missing" | "loading"),
            "unexpected: {}",
            error
        );
    }

    #[test]
    fn missing_transcript_is_rejected() {
        let p = provider();
        let request = ProviderRequest::new(GPTSOVITS_MODELS[1], "profile", "hello", "en");
        let error = p.synthesize(&request).unwrap_err();
        assert_eq!(error.code(), "invalid_voice");
        assert!(error.message().contains("transcript"));
    }

    #[test]
    fn cloning_without_request_reference_audio_is_rejected() {
        let p = provider();
        let mut request = ProviderRequest::new(GPTSOVITS_MODELS[1], "profile", "hello", "en");
        request.reference_transcript = Some("hello".to_string());
        let error = p.synthesize(&request).unwrap_err();
        assert_eq!(error.code(), "invalid_voice");
        assert!(error.message().contains("reference audio"));
    }

    #[test]
    fn raw_pth_checkpoints_are_rejected_with_export_guidance() {
        let p = provider();
        let error = p.load("model-v2.pth").unwrap_err();
        assert_eq!(error.code(), "unsupported_model");
        assert!(error.message().contains("ONNX"));
    }

    #[test]
    fn gpu_requests_are_explicitly_unsupported() {
        let p = provider();
        let mut request = ProviderRequest::new(GPTSOVITS_MODELS[1], "profile", "hello", "en");
        request.reference_transcript = Some("hello".to_string());
        request.device = DevicePreference::Cuda;
        let error = p.synthesize(&with_reference(request)).unwrap_err();
        assert_eq!(error.code(), "device_unavailable");
    }

    #[test]
    fn malformed_bundle_names_missing_members() {
        let dir = std::env::temp_dir().join(format!("gptsovits-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let error = GptSovitsProvider::validate_bundle(&dir).unwrap_err();
        assert_eq!(error.code(), "model_missing");
        assert!(error.message().contains("vits.onnx"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn concurrent_references_use_distinct_cache_keys() {
        let p = provider();
        let a = p
            .prepare_reference(
                GPTSOVITS_MODELS[1],
                &vec![0.1; 48000],
                16000,
                Some("hello"),
                CloneMode::Icl,
            )
            .unwrap();
        let b = p
            .prepare_reference(
                GPTSOVITS_MODELS[1],
                &vec![0.2; 48000],
                16000,
                Some("hello"),
                CloneMode::Icl,
            )
            .unwrap();
        assert_ne!(a, b, "distinct references must never share conditioning");
    }

    #[test]
    fn reference_staging_produces_16k_mono_wav() {
        let path = GptSovitsProvider::stage_reference_wav(&vec![0.25; 24000], 24000).unwrap();
        let reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().sample_rate, GPT_REFERENCE_RATE);
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.duration(), 16000);
        let _ = std::fs::remove_file(&path);
    }
}
