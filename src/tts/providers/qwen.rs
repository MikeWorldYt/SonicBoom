//! Qwen3-TTS (Candle) voice-cloning provider.
//!
//! Upstream implementation under review:
//! `https://github.com/TrevorS/qwen3-tts-rs` at
//! `711ceee07cad92673f86de8997bdf54c30caa49f` (pinned dependency
//! revision, upstream HEAD at review time 2026-10-13). Candle, native
//! Rust; Base variants support reference-audio ICL and x-vector cloning;
//! CPU/CUDA/Metal features.
//!
//! This provider runs in-process with no Python/PyTorch and never calls
//! the upstream CLI: weights load through `Qwen3TTS::from_pretrained` on
//! a verified local bundle, prompts through `create_voice_clone_prompt`,
//! audio through `synthesize_voice_clone`.
//!
//! Ownership honesty: the upstream model handle is `Send` but not `Sync`
//! (interior mutability in the speech encoder), so it lives on a dedicated
//! in-process Rust worker thread that owns it for its whole lifetime —
//! never a subprocess, never shared `&` access. Requests cross the thread
//! boundary as plain data (`String`, `Vec<f32>`, sampling options); voice
//! prompts are cached inside the worker under the conditioning key.
//!
//! `device=cpu|auto` works everywhere; `device=cuda` requires the
//! `qwen-cuda` Cargo feature (NVIDIA builds only) and is rejected
//! explicitly otherwise. AMD BC-250 (`gfx1013`) is CPU-only for the
//! acceptance criteria: no ROCm/MIGraphX claim is made here.
//!
//! Without the `qwen` Cargo feature every synthesis path fails with an
//! explicit `model_missing` diagnostic (never silence, never a Supertonic
//! fallback).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::TtsProvider;
use super::assets::PinnedFile;
use super::error::ProviderError;
use super::types::{
    CloneMode, DevicePreference, EffectiveDevice, PcmAudio, ProviderCapabilities, ProviderRequest,
    ProviderStatus, ProviderStatusState,
};

/// Reviewed upstream revision (pinned dependency revision).
pub const QWEN_UPSTREAM_REV: &str = "711ceee07cad92673f86de8997bdf54c30caa49f";
/// Supported models: Base variants (ICL/x-vector cloning). CustomVoice and
/// VoiceDesign are rejected: they do not support reference cloning.
/// The 1.7B Base identifier is accepted for routing but load is refused
/// until a verified manifest exists (weights exceed the verified CPU path).
pub const QWEN_MODELS: &[&str] = &["qwen3-tts-0.6b-base", "qwen3-tts-1.7b-base"];
/// Languages implemented upstream and supported here (BCP-47 tags).
/// Every entry parses via the upstream `Language` grammar.
pub const QWEN_LANGUAGES: &[&str] = &["en", "ja", "es", "zh", "ko", "fr", "de"];
/// Reference audio is resampled to 24 kHz by the upstream prompt builder.
/// Minimum/maximum reference duration (seconds).
pub const QWEN_REFERENCE_RATE: u32 = 24_000;
pub const QWEN_REF_MIN_SECS: f64 = 1.0;
pub const QWEN_REF_MAX_SECS: f64 = 30.0;

/// Required model asset files for a verified local/offline import.
/// `from_pretrained` needs the talker weights + tokenizer pieces plus the
/// bundled 12 Hz speech tokenizer (`speech_tokenizer/model.safetensors`).
pub const QWEN_REQUIRED_FILES: &[&str] = &[
    "config.json",
    "vocab.json",
    "merges.txt",
    "model.safetensors",
    "speech_tokenizer/model.safetensors",
];

/// 0.6B-Base source repository and pinned revision.
pub const QWEN_06B_REPO: &str = "Qwen/Qwen3-TTS-12Hz-0.6B-Base";
pub const QWEN_06B_REVISION: &str = "5d83992436eae1d760afd27aff78a71d676296fc";

/// Verified 0.6B-Base manifest: (relative path, SHA-256 hex, size in bytes).
/// Sizes confirmed 2026-10-13 via registry headers; hashes are computed
/// from the downloaded bytes before the smoke test runs.
pub const QWEN_06B_MANIFEST: &[(&str, &str, u64)] = &[
    (
        "config.json",
        "2e714c787c8edb98b05432685cddb634add2de4d4e645f653d68251ef72ba011",
        4494,
    ),
    (
        "vocab.json",
        "ca10d7e9fb3ed18575dd1e277a2579c16d108e32f27439684afa0e10b1440910",
        2776833,
    ),
    (
        "merges.txt",
        "599bab54075088774b1733fde865d5bd747cbcc7a547c5bc12610e874e26f5e3",
        1671839,
    ),
    (
        "model.safetensors",
        "180b3b10eb1c9f1b4db7806d5475bae3071c0243c299d49926bab1da3b6946f6",
        1829344272,
    ),
    (
        "speech_tokenizer/model.safetensors",
        "836b7b357f5ea43e889936a3709af68dfe3751881acefe4ecf0dbd30ba571258",
        682293092,
    ),
];

/// Pinned download manifest for a supported model ID, or an explicit error
/// for variants without a verified manifest (1.7B: weights exceed the
/// verified CPU path and no local bundle has been hashed yet).
pub fn qwen_manifest(model_id: &str) -> Result<Vec<PinnedFile>, ProviderError> {
    match model_id {
        "qwen3-tts-0.6b-base" => Ok(QWEN_06B_MANIFEST
            .iter()
            .map(|(path, sha256, size)| PinnedFile {
                path,
                url: PinnedFile::huggingface(QWEN_06B_REPO, QWEN_06B_REVISION, path),
                sha256,
                size: *size,
            })
            .collect()),
        "qwen3-tts-1.7b-base" => Err(ProviderError::model_missing(
            "qwen3-tts-1.7b-base has no verified download manifest: its weights \
             (~7 GB in F32) exceed the verified CPU memory path on constrained hosts. \
             Use qwen3-tts-0.6b-base; see docs/providers.md",
        )),
        other => Err(ProviderError::unsupported_model(
            "qwen3",
            other,
            format!(
                "Base variants support reference cloning; expected one of: {}. \
                 CustomVoice/VoiceDesign are rejected for cloning requests",
                QWEN_MODELS.join(", ")
            ),
        )),
    }
}

/// Qwen provider configuration (mirrors `[providers.qwen3]` in config.toml).
#[derive(Debug, Clone, PartialEq)]
pub struct QwenConfig {
    pub enabled: bool,
    pub model_dir: PathBuf,
    pub model_id: String,
    pub revision: String,
    pub device: DevicePreference,
    pub max_tokens: usize,
    pub cache_capacity: usize,
}

impl Default for QwenConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model_dir: PathBuf::from("./models/qwen3-tts"),
            model_id: QWEN_MODELS[0].to_string(),
            revision: QWEN_06B_REVISION.to_string(),
            device: DevicePreference::Auto,
            max_tokens: 1024,
            cache_capacity: 16,
        }
    }
}

#[derive(Default)]
struct QwenMetrics {
    loads: std::sync::atomic::AtomicU64,
    hits: std::sync::atomic::AtomicU64,
    misses: std::sync::atomic::AtomicU64,
    evictions: std::sync::atomic::AtomicU64,
}

struct QwenState {
    loaded_model: Option<String>,
    selected_device: Option<EffectiveDevice>,
    load_error: Option<String>,
    ensuring: bool,
    download_progress: f32,
    /// A real network fetch started during this ensure pass. Hash
    /// verification alone advances `download_progress` too, so status
    /// lines consult this flag to say "verifying" instead of lying
    /// about "downloading" on warm boots.
    downloading: bool,
}

/// Dedicated in-process worker owning the non-`Sync` native model.
///
/// The worker thread is the only place the `Qwen3TTS` handle exists; jobs
/// cross as plain data and replies return plain data. Prompt conditioning
/// is cached inside the worker under the provider's conditioning key, so
/// repeat syntheses with one profile skip reconditioning. This is a Rust
/// thread in this process — never a subprocess.
#[cfg(feature = "qwen")]
mod worker {
    use std::collections::{HashMap, VecDeque};
    use std::sync::mpsc;

    use qwen3_tts::{AudioBuffer, Language, ModelType, Qwen3TTS, SynthesisOptions};

    use super::CloneMode;

    /// Load outcome returned to the provider for variant enforcement.
    pub struct ModelInfo {
        pub model_type: Option<ModelType>,
        pub supports_cloning: bool,
        pub selected_device: super::EffectiveDevice,
    }

    enum Job {
        Load {
            dir: String,
            device: String,
            reply: mpsc::SyncSender<Result<ModelInfo, String>>,
        },
        Prompt {
            key: String,
            pcm: Vec<f32>,
            rate: u32,
            transcript: Option<String>,
            mode: CloneMode,
            capacity: usize,
            control: std::sync::Arc<crate::tts::control::InferenceControl>,
            reply: mpsc::SyncSender<Result<(), String>>,
        },
        Synthesize {
            text: String,
            key: String,
            language: String,
            max_tokens: usize,
            seed: Option<u64>,
            control: std::sync::Arc<crate::tts::control::InferenceControl>,
            reply: mpsc::SyncSender<Result<(Vec<f32>, u32), String>>,
        },
        Unload,
        Shutdown,
        Invalidate,
    }

    pub struct WorkerHandle {
        tx: mpsc::SyncSender<Job>,
        thread: Option<std::thread::JoinHandle<()>>,
    }
    impl Drop for WorkerHandle {
        fn drop(&mut self) {
            let _ = self.tx.send(Job::Shutdown);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    impl WorkerHandle {
        pub fn spawn(
            metrics: std::sync::Arc<super::QwenMetrics>,
            invalidations: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
        ) -> Result<Self, String> {
            let (tx, rx) = mpsc::sync_channel::<Job>(2);
            let thread = std::thread::Builder::new()
                .name("qwen3-inference".to_owned())
                .spawn(move || run(rx, metrics, invalidations))
                .map_err(|e| format!("could not start qwen3 worker thread: {e}"))?;
            Ok(Self {
                tx,
                thread: Some(thread),
            })
        }

        fn call<T: Send + 'static>(
            &self,
            make: impl FnOnce(mpsc::SyncSender<T>) -> Job,
        ) -> Result<T, String> {
            let (reply_tx, reply_rx) = mpsc::sync_channel::<T>(1);
            self.tx
                .try_send(make(reply_tx))
                .map_err(|error| match error {
                    mpsc::TrySendError::Full(_) => "qwen3 worker queue is full".to_string(),
                    mpsc::TrySendError::Disconnected(_) => {
                        "qwen3 worker thread is gone".to_string()
                    }
                })?;
            loop {
                match reply_rx.recv_timeout(std::time::Duration::from_secs(30)) {
                    Ok(result) => return Ok(result),
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        return Err("qwen3 worker reply lost".into());
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if let Some(control) = crate::tts::control::current() {
                            tracing::warn!(
                                deadline_exceeded = control.stopped(),
                                stage = control.snapshot().stage,
                                "Qwen worker still executing; retaining inference permit"
                            );
                        }
                    }
                }
            }
        }

        pub fn alive(&self) -> bool {
            self.thread.as_ref().is_some_and(|t| !t.is_finished())
        }
        pub fn load(&self, dir: String, device: String) -> Result<ModelInfo, String> {
            self.call(|reply| Job::Load { dir, device, reply })?
        }

        pub fn prompt(
            &self,
            key: String,
            pcm: Vec<f32>,
            rate: u32,
            transcript: Option<String>,
            mode: CloneMode,
            capacity: usize,
        ) -> Result<(), String> {
            self.call(|reply| Job::Prompt {
                key,
                pcm,
                rate,
                transcript,
                mode,
                capacity,
                control: crate::tts::control::current().unwrap_or_else(|| {
                    crate::tts::control::InferenceControl::new(std::time::Duration::from_secs(1800))
                }),
                reply,
            })?
        }

        pub fn synthesize(
            &self,
            text: String,
            key: String,
            language: String,
            max_tokens: usize,
            seed: Option<u64>,
        ) -> Result<(Vec<f32>, u32), String> {
            self.call(|reply| Job::Synthesize {
                text,
                key,
                language,
                max_tokens,
                seed,
                control: crate::tts::control::current().unwrap_or_else(|| {
                    crate::tts::control::InferenceControl::new(std::time::Duration::from_secs(1800))
                }),
                reply,
            })?
        }

        pub fn invalidate(&self) {
            let _ = self.tx.try_send(Job::Invalidate);
        }
        pub fn unload(&self) {
            let _ = self.tx.send(Job::Unload);
        }
    }

    fn run(
        rx: mpsc::Receiver<Job>,
        metrics: std::sync::Arc<super::QwenMetrics>,
        invalidations: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    ) {
        let mut model: Option<Qwen3TTS> = None;
        let mut prompts: HashMap<String, qwen3_tts::VoiceClonePrompt> = HashMap::new();
        let mut order: VecDeque<String> = VecDeque::new();
        while let Ok(job) = rx.recv() {
            let stale = invalidations
                .lock()
                .map(|mut set| std::mem::take(&mut *set))
                .unwrap_or_default();
            if stale.contains("*") {
                prompts.clear();
                order.clear();
            } else {
                prompts.retain(|key, _| !stale.iter().any(|prefix| key.starts_with(prefix)));
                order.retain(|key| !stale.iter().any(|prefix| key.starts_with(prefix)));
            }
            match job {
                Job::Load { dir, device, reply } => {
                    model = None;
                    prompts.clear();
                    order.clear();
                    let result = (|| {
                        let parsed = qwen3_tts::parse_device(&device).map_err(|e| e.to_string())?;
                        let loaded =
                            Qwen3TTS::from_pretrained(&dir, parsed).map_err(|e| e.to_string())?;
                        let info = ModelInfo {
                            model_type: loaded.model_type().copied(),
                            supports_cloning: loaded.supports_voice_cloning(),
                            selected_device: if loaded.device().is_cuda() {
                                super::EffectiveDevice::Cuda
                            } else {
                                super::EffectiveDevice::Cpu
                            },
                        };
                        metrics
                            .loads
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        model = Some(loaded);
                        prompts.clear();
                        order.clear();
                        Ok(info)
                    })();
                    let _ = reply.send(result);
                }
                Job::Prompt {
                    key,
                    pcm,
                    rate,
                    transcript,
                    mode,
                    capacity: requested_capacity,
                    control,
                    reply,
                } => {
                    let capacity = requested_capacity.max(1);
                    let result = (|| {
                        let active = model.as_ref().ok_or("qwen3 model is not loaded")?;
                        if prompts.contains_key(&key) {
                            metrics
                                .hits
                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            order.retain(|k| k != &key);
                            order.push_back(key.clone());
                            while prompts.len() > capacity {
                                if let Some(old) = order.pop_front() {
                                    prompts.remove(&old);
                                    metrics
                                        .evictions
                                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                }
                            }
                            tracing::debug!(cache_hit = true, "Qwen conditioning");
                            return Ok(());
                        }
                        let prepared = key
                            .split_once('|')
                            .filter(|(prefix, _)| prefix.len() == 64)
                            .and_then(|(_, base)| prompts.get(base))
                            .map(|p| qwen3_tts::VoiceClonePrompt {
                                speaker_embedding: p.speaker_embedding.clone(),
                                ref_codes: p.ref_codes.clone(),
                                ref_text_ids: p.ref_text_ids.clone(),
                            });
                        if prepared.is_some() {
                            metrics
                                .hits
                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        } else {
                            metrics
                                .misses
                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                        let preparation = std::time::Instant::now();
                        let reference = AudioBuffer::new(pcm, rate);
                        let ref_text = match mode {
                            CloneMode::Icl => transcript.as_deref(),
                            CloneMode::XVector => None,
                        };
                        let prompt = if let Some(prepared) = prepared {
                            prepared
                        } else {
                            qwen3_tts::control::scope(
                                std::sync::Arc::new(move |stage, frames| {
                                    control.update(stage, frames)
                                }),
                                || active.create_voice_clone_prompt(&reference, ref_text),
                            )
                            .map_err(|e| e.to_string())?
                        };
                        let prompt_bytes = |p: &qwen3_tts::VoiceClonePrompt| {
                            p.speaker_embedding.elem_count()
                                * p.speaker_embedding.dtype().size_in_bytes()
                                + p.ref_codes
                                    .as_ref()
                                    .map(|t| t.elem_count() * t.dtype().size_in_bytes())
                                    .unwrap_or(0)
                                + p.ref_text_ids.as_ref().map(|v| v.len() * 4).unwrap_or(0)
                        };
                        let new_bytes = prompt_bytes(&prompt);
                        if new_bytes > 64 * 1024 * 1024 {
                            return Err("conditioning exceeds 64 MiB budget".into());
                        }
                        while prompts.len() >= capacity
                            || prompts.values().map(prompt_bytes).sum::<usize>() + new_bytes
                                > 64 * 1024 * 1024
                        {
                            match order.pop_front() {
                                Some(old) => {
                                    prompts.remove(&old);
                                    metrics
                                        .evictions
                                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                }
                                None => break,
                            }
                        }
                        order.push_back(key.clone());
                        prompts.insert(key, prompt);
                        tracing::debug!(
                            cache_hit = false,
                            preparation_secs = preparation.elapsed().as_secs_f64(),
                            "Qwen conditioning"
                        );
                        Ok(())
                    })();
                    let _ = reply.send(result);
                }
                Job::Synthesize {
                    text,
                    key,
                    language,
                    max_tokens,
                    seed,
                    control,
                    reply,
                } => {
                    let span = tracing::info_span!("qwen_job", job_id = %control.id);
                    let _span = span.enter();
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        if control.stopped() {
                            return Err("inference cancelled or deadline exceeded".into());
                        }
                        order.retain(|k| k != &key);
                        order.push_back(key.clone());
                        let active = model.as_ref().ok_or("qwen3 model is not loaded")?;
                        let prompt = prompts.get(&key).ok_or(
                            "voice conditioning expired before synthesis; retry the request",
                        )?;
                        let parsed: Language = language.parse().map_err(|e| format!("{e}"))?;
                        let options = SynthesisOptions {
                            max_length: max_tokens,
                            seed,
                            ..SynthesisOptions::default()
                        };
                        let started = std::time::Instant::now();
                        let hook_control = control.clone();
                        let audio = qwen3_tts::control::scope(
                            std::sync::Arc::new(move |stage, frames| {
                                hook_control.update(stage, frames)
                            }),
                            || active.synthesize_voice_clone(&text, prompt, parsed, Some(options)),
                        )
                        .map_err(|e| e.to_string())?;
                        let duration = audio.samples.len() as f64 / f64::from(audio.sample_rate);
                        tracing::info!(
                            elapsed_secs = started.elapsed().as_secs_f64(),
                            audio_secs = duration,
                            rtf = started.elapsed().as_secs_f64() / duration.max(0.001),
                            frames = control.snapshot().frames,
                            "Qwen synthesis"
                        );
                        Ok((audio.samples, audio.sample_rate))
                    }))
                    .unwrap_or_else(|_| {
                        model = None;
                        prompts.clear();
                        order.clear();
                        Err("qwen3 worker panicked; model retired".into())
                    });
                    let _ = reply.send(result);
                }
                Job::Shutdown => break,
                Job::Invalidate => {}
                Job::Unload => {
                    model = None;
                    prompts.clear();
                    order.clear();
                }
            }
        }
    }
}

/// Native Qwen3-TTS provider.
///
/// Model weights execute through the upstream Candle pipeline on the
/// worker thread once a verified bundle is installed; until then every
/// synthesis path returns an explicit `model_missing` error naming the
/// absent assets — never silence, never a Supertonic fallback.
pub struct QwenProvider {
    config: Mutex<QwenConfig>,
    pending_config: Mutex<Option<QwenConfig>>,
    metrics: Arc<QwenMetrics>,
    invalidations: Arc<Mutex<std::collections::HashSet<String>>>,
    state: Mutex<QwenState>,
    /// Worker owning the native model (`Send`, not `Sync`: only the
    /// channel handle lives here, so this provider stays `Sync`).
    /// Dropped on unload/reconfigure; the thread exits after draining.
    #[cfg(feature = "qwen")]
    worker: Mutex<Option<worker::WorkerHandle>>,
    /// Serializes synthesis so cached conditioning is reused safely and
    /// concurrent voices never interleave. (Only read when the native
    /// backend is compiled in.)
    #[cfg_attr(not(feature = "qwen"), allow(dead_code))]
    inference_lock: Mutex<()>,
}

impl QwenProvider {
    pub fn diagnostics(&self) -> serde_json::Value {
        use std::sync::atomic::Ordering::Relaxed;
        serde_json::json!({ "model_load_count": self.metrics.loads.load(Relaxed),
            "resident": self.state.lock().map(|s| s.loaded_model.is_some()).unwrap_or(false),
            "conditioning_hits": self.metrics.hits.load(Relaxed), "conditioning_misses": self.metrics.misses.load(Relaxed),
            "conditioning_evictions": self.metrics.evictions.load(Relaxed), "conditioning_memory_limit_bytes": 64 * 1024 * 1024,
            "worker_capacity": 2, "effective_concurrency": 1, "cuda_compiled": cfg!(feature = "qwen-cuda") })
    }
    pub fn new(config: QwenConfig) -> Arc<Self> {
        Arc::new(Self {
            config: Mutex::new(config),
            pending_config: Mutex::new(None),
            metrics: Arc::new(QwenMetrics::default()),
            invalidations: Arc::new(Mutex::new(std::collections::HashSet::new())),
            state: Mutex::new(QwenState {
                loaded_model: None,
                selected_device: None,
                load_error: None,
                ensuring: false,
                download_progress: 0.0,
                downloading: false,
            }),
            #[cfg(feature = "qwen")]
            worker: Mutex::new(None),
            inference_lock: Mutex::new(()),
        })
    }

    /// Apply fresh configuration: drops the worker (and its model/prompts)
    /// without touching the async download flag. The next request re-syncs
    /// and reloads; the last healthy state is reported until then.
    pub fn reconfigure(&self, config: QwenConfig) {
        // Request-scoped config sync must never block the Tokio executor on
        // active native computation. Adopt a busy model's change on the next sync.
        let Ok(_lifecycle) = self.inference_lock.try_lock() else {
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
        #[cfg(feature = "qwen")]
        if let Ok(mut worker) = self.worker.lock()
            && let Some(handle) = worker.take()
        {
            handle.unload();
        }
        if let Ok(mut state) = self.state.lock() {
            state.loaded_model = None;
            state.selected_device = None;
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
    fn same_model(a: &QwenConfig, b: &QwenConfig) -> bool {
        a.enabled == b.enabled
            && a.model_dir == b.model_dir
            && a.model_id == b.model_id
            && a.revision == b.revision
            && a.device == b.device
    }
    fn config_snapshot(&self) -> QwenConfig {
        self.config.lock().map(|c| c.clone()).unwrap_or_default()
    }

    /// Adopt `next` only when it differs (reconfigure drops the loaded
    /// model, so spurious resets would interrupt serving providers).
    pub fn sync_config(&self, next: QwenConfig) {
        let changed = self
            .config
            .lock()
            .map(|current| *current != next)
            .unwrap_or(true);
        if changed || self.reload_pending() {
            self.reconfigure(next);
        }
    }

    /// Validate model assets without moving untrusted paths into a loader.
    pub fn validate_assets(model_dir: &std::path::Path) -> Result<(), ProviderError> {
        let mut missing = Vec::new();
        for file in QWEN_REQUIRED_FILES {
            if !model_dir.join(file).is_file() {
                missing.push(file.to_string());
            }
        }
        if missing.is_empty() {
            Ok(())
        } else {
            Err(ProviderError::model_missing(format!(
                "qwen3 model bundle incomplete in '{}': missing {} \
                 (see docs/providers.md for the pinned manifest and offline import)",
                model_dir.display(),
                missing.join(", ")
            )))
        }
    }

    fn resolve_device(requested: DevicePreference) -> Result<EffectiveDevice, ProviderError> {
        match requested {
            DevicePreference::Cpu => Ok(EffectiveDevice::Cpu),
            DevicePreference::Auto => {
                #[cfg(feature = "qwen-cuda")]
                if qwen3_tts::auto_device().is_ok_and(|d| d.is_cuda()) {
                    return Ok(EffectiveDevice::Cuda);
                }
                Ok(EffectiveDevice::Cpu)
            }
            DevicePreference::Cuda => {
                if cfg!(feature = "qwen-cuda") {
                    #[cfg(feature = "qwen-cuda")]
                    qwen3_tts::parse_device("cuda").map_err(|e| {
                        ProviderError::device_unavailable(format!(
                            "CUDA was compiled but device initialization failed: {e}"
                        ))
                    })?;
                    Ok(EffectiveDevice::Cuda)
                } else {
                    Err(ProviderError::device_unavailable(
                        "qwen3 device 'cuda' requires the `qwen-cuda` build feature \
                         (NVIDIA only); this build supports 'cpu' and 'auto'",
                    ))
                }
            }
        }
    }

    fn check_language(language: &str) -> Result<(), ProviderError> {
        if QWEN_LANGUAGES.contains(&language) {
            Ok(())
        } else {
            Err(ProviderError::unsupported_language(
                "qwen3",
                language,
                QWEN_LANGUAGES,
            ))
        }
    }

    fn check_model(model: &str) -> Result<(), ProviderError> {
        if QWEN_MODELS.contains(&model) {
            Ok(())
        } else {
            Err(ProviderError::unsupported_model(
                "qwen3",
                model,
                format!(
                    "Base variants support reference cloning; expected one of: {}. \
                     CustomVoice/VoiceDesign are rejected for cloning requests",
                    QWEN_MODELS.join(", ")
                ),
            ))
        }
    }

    /// Validate a decoded reference buffer (duration, sample count).
    pub fn validate_reference(pcm: &[f32], sample_rate: u32) -> Result<(), ProviderError> {
        if pcm.iter().any(|s| !s.is_finite()) {
            return Err(ProviderError::invalid_voice(
                "reference audio must contain finite samples",
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
        if secs < QWEN_REF_MIN_SECS {
            return Err(ProviderError::invalid_voice(format!(
                "reference audio is too short ({secs:.1}s); provide at least {:.0}s",
                QWEN_REF_MIN_SECS
            )));
        }
        if secs > QWEN_REF_MAX_SECS {
            return Err(ProviderError::invalid_voice(format!(
                "reference audio is too long ({secs:.1}s); keep it under {:.0}s",
                QWEN_REF_MAX_SECS
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

    fn voice_prefix(voice: &str) -> String {
        use sha2::Digest;
        format!("{}|", hex::encode(sha2::Sha256::digest(voice.as_bytes())))
    }
    fn conditioning_key(
        &self,
        reference_hash: &str,
        transcript: Option<&str>,
        mode: CloneMode,
    ) -> String {
        super::worker::ConditioningCache::cache_key(
            "qwen3",
            &format!(
                "{}:{}:{}",
                QWEN_UPSTREAM_REV,
                self.config_snapshot().model_id,
                self.config_snapshot().revision
            ),
            reference_hash,
            transcript,
            mode.as_str(),
        )
    }

    /// Synchronous load: validates identifiers, assets and device, then
    /// loads the native model on the worker thread from the calling
    /// (blocking) thread. Downloading is never performed here; missing
    /// assets are an explicit `model_missing` error.
    fn load_native(&self, model_id: &str) -> Result<(), ProviderError> {
        let config = self.config_snapshot();
        self.load_expected(model_id, &config)
    }
    fn load_expected(&self, model_id: &str, expected: &QwenConfig) -> Result<(), ProviderError> {
        let _lifecycle = self
            .inference_lock
            .lock()
            .map_err(|_| ProviderError::inference_failed("qwen lifecycle lock poisoned"))?;
        if self.model_change_pending() || !Self::same_model(&self.config_snapshot(), expected) {
            return Err(ProviderError::loading("obsolete Qwen load discarded"));
        }
        if self.is_loaded() {
            return Ok(());
        }
        Self::check_model(model_id)?;
        // Variants without a verified manifest fail here with guidance.
        qwen_manifest(model_id)?;
        let config = self.config_snapshot();
        let effective = Self::resolve_device(config.device)?;
        Self::validate_assets(&config.model_dir)?;
        #[cfg(feature = "qwen")]
        {
            if config.revision != QWEN_06B_REVISION {
                return Err(ProviderError::model_missing(
                    "Qwen requires the reviewed pinned model revision",
                ));
            }
            super::assets::verify_local_files(&config.model_dir, &qwen_manifest(model_id)?)?;
            let _load_resource = super::worker::model_load_guard();
            let device_name = match effective {
                EffectiveDevice::Cpu => "cpu",
                EffectiveDevice::Cuda => "cuda",
            };
            // Take or spawn the worker; a dead worker is replaced, never reused.
            let spawn = || {
                worker::WorkerHandle::spawn(self.metrics.clone(), self.invalidations.clone())
                    .map_err(ProviderError::inference_failed)
            };
            let mut slot = self
                .worker
                .lock()
                .map_err(|_| ProviderError::inference_failed("qwen3 worker lock poisoned"))?;
            if slot.as_ref().is_none_or(|worker| !worker.alive()) {
                *slot = Some(spawn()?);
            }
            let info = match slot.as_ref().map(|w| {
                w.load(
                    config.model_dir.to_string_lossy().into_owned(),
                    device_name.to_string(),
                )
            }) {
                Some(Ok(info)) => info,
                Some(Err(error)) => {
                    *slot = None;
                    return Err(ProviderError::inference_failed(format!(
                        "qwen3 native load failed: {error}"
                    )));
                }
                None => {
                    *slot = None;
                    return Err(ProviderError::inference_failed("qwen3 worker is gone"));
                }
            };
            drop(slot);
            if self.model_change_pending() {
                if let Ok(mut slot) = self.worker.lock() {
                    *slot = None;
                }
                return Err(ProviderError::loading(
                    "obsolete Qwen native load discarded",
                ));
            }
            // Reject incompatible variants explicitly: only Base weights
            // carry the speaker encoder required for cloning.
            use qwen3_tts::ModelType;
            match info.model_type {
                Some(ModelType::CustomVoice) => {
                    return Err(ProviderError::unsupported_model(
                        "qwen3",
                        model_id,
                        "CustomVoice models use preset speakers, not voice cloning. \
                         Use a Base model for voice cloning",
                    ));
                }
                Some(ModelType::VoiceDesign) => {
                    return Err(ProviderError::unsupported_model(
                        "qwen3",
                        model_id,
                        "VoiceDesign models use text-described voices, not voice cloning. \
                         Use a Base model for voice cloning",
                    ));
                }
                _ => {}
            }
            if !info.supports_cloning {
                return Err(ProviderError::unsupported_model(
                    "qwen3",
                    model_id,
                    "loaded weights lack a speaker encoder; only Base models \
                     support reference cloning",
                ));
            }
            if let Ok(mut state) = self.state.lock() {
                state.loaded_model = Some(model_id.to_string());
                state.selected_device = Some(info.selected_device);
                state.load_error = None;
            }
            Ok(())
        }
        #[cfg(not(feature = "qwen"))]
        {
            let _ = (config, model_id, effective);
            Err(ProviderError::model_missing(
                "qwen3 weights validated but this build omits the native Candle backend; \
                 rebuild with `--features qwen` (CPU) for voice cloning",
            ))
        }
    }

    /// Start download + load in the background (deduplicated). Returns
    /// immediately; progress surfaces through [`TtsProvider::status`].
    /// Used at startup and when hot-reload enables the provider.
    pub fn ensure_background(self: &Arc<Self>, hf_token: Option<String>) {
        {
            let mut state = match self.state.lock() {
                Ok(state) => state,
                Err(_) => return,
            };
            if state.ensuring {
                return;
            }
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
                tracing::error!("qwen3 background prepare failed: {error}");
            }
        });
    }

    async fn ensure_async(self: &Arc<Self>, hf_token: Option<String>) -> Result<(), ProviderError> {
        if !cfg!(feature = "qwen") {
            return Err(ProviderError::model_missing(
                "native backend is not compiled; rebuild with --features qwen",
            ));
        }
        let config = self.config_snapshot();
        if !config.enabled {
            return Err(ProviderError::model_missing(
                "qwen3 provider is disabled ([providers.qwen3] enabled=false)",
            ));
        }
        let manifest = qwen_manifest(&config.model_id)?;
        let progress_self = Arc::clone(self);
        let download_self = Arc::clone(self);
        super::assets::ensure_pinned_files(
            "qwen3",
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
        if let Ok(mut state) = self.state.lock() {
            state.downloading = false;
        }
        // Heavy native load runs on a blocking thread; the async task only
        // records the outcome. Reload failure keeps the last working model.
        let loader = Arc::clone(self);
        let model_id = config.model_id.clone();
        tokio::task::spawn_blocking(move || loader.load_expected(&model_id, &config))
            .await
            .map_err(|e| ProviderError::inference_failed(format!("qwen3 load task failed: {e}")))?
    }

    /// Whether a background ensure is currently running.
    pub fn is_ensuring(&self) -> bool {
        self.state.lock().map(|s| s.ensuring).unwrap_or(false)
    }

    /// Trigger a background ensure when the provider is enabled but has no
    /// loaded model and no error yet (hot-reload path). Called during
    /// request-scoped registry sync; never blocks.
    pub fn kick_ensure_if_needed(self: &Arc<Self>, hf_token: Option<String>) {
        if self.reload_pending() {
            return;
        }
        let resident = self.is_loaded();
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

    /// Whether a native model is currently resident (recorded state plus
    /// a live worker; a dead worker counts as unloaded so the next kick
    /// re-ensures instead of wedging on a stale flag).
    fn is_loaded(&self) -> bool {
        #[cfg(feature = "qwen")]
        {
            self.state
                .lock()
                .map(|s| s.loaded_model.is_some())
                .unwrap_or(false)
        }
        #[cfg(not(feature = "qwen"))]
        {
            false
        }
    }

    /// Error describing why no model is resident (only meaningful with the
    /// native backend compiled in).
    #[cfg(feature = "qwen")]
    fn not_loaded_error(&self) -> ProviderError {
        let state = self.state.lock().ok();
        match state {
            Some(state) if state.ensuring && state.download_progress >= 1.0 => {
                ProviderError::loading("Qwen assets verified; native model is loading")
            }
            Some(state) if state.ensuring && state.downloading => ProviderError::loading(format!(
                "qwen3 model is downloading ({:.0}% complete)",
                state.download_progress * 100.0
            )),
            Some(state) if state.ensuring => ProviderError::loading(format!(
                "qwen3 model is verifying cache ({:.0}% complete)",
                state.download_progress * 100.0
            )),
            Some(state) if let Some(detail) = state.load_error.clone() => {
                ProviderError::inference_failed(format!("qwen3 model unavailable: {detail}"))
            }
            _ => ProviderError::loading("qwen3 model is not loaded yet"),
        }
    }

    /// Ensure the worker holds conditioning for this request (idempotent:
    /// cache hits skip recomputation). Blocks the calling thread; callers
    /// run on blocking threads already.
    #[cfg(feature = "qwen")]
    fn ensure_prompt(
        &self,
        reference_pcm: &[f32],
        reference_rate: u32,
        transcript: Option<&str>,
        mode: CloneMode,
        voice: Option<&str>,
    ) -> Result<String, ProviderError> {
        let hash = format!("{reference_rate}:{}", Self::reference_hash(reference_pcm));
        let base_key = self.conditioning_key(&hash, transcript, mode);
        let key = voice
            .map(|voice| format!("{}{base_key}", Self::voice_prefix(voice)))
            .unwrap_or(base_key);
        let worker = self
            .worker
            .lock()
            .map_err(|_| ProviderError::inference_failed("qwen3 worker lock poisoned"))?;
        let handle = worker.as_ref().ok_or_else(|| self.not_loaded_error())?;
        handle
            .prompt(
                key.clone(),
                reference_pcm.to_vec(),
                reference_rate,
                transcript.map(str::to_string),
                mode,
                self.config_snapshot().cache_capacity,
            )
            .map_err(|error| {
                if (error.contains("worker reply lost") || error.contains("worker thread is gone"))
                    && let Ok(mut state) = self.state.lock()
                {
                    state.loaded_model = None;
                    state.selected_device = None;
                    state.load_error = None;
                }
                if error.contains("queue is full") {
                    ProviderError::busy(error)
                } else {
                    ProviderError::inference_failed(error)
                }
            })?;
        Ok(key)
    }
}

impl TtsProvider for QwenProvider {
    fn id(&self) -> &'static str {
        "qwen3"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider_id: "qwen3".to_string(),
            models: QWEN_MODELS.iter().map(|s| s.to_string()).collect(),
            languages: QWEN_LANGUAGES.iter().map(|s| s.to_string()).collect(),
            clone_modes: vec!["icl".to_string(), "x_vector".to_string()],
            devices: vec!["cpu".to_string(), "auto".to_string(), "cuda".to_string()],
            requires_reference_audio: true,
            requires_reference_transcript: false,
            sample_rate: Some(QWEN_REFERENCE_RATE),
        }
    }

    fn status(&self) -> ProviderStatus {
        let config = self.config_snapshot();
        if !config.enabled {
            return ProviderStatus {
                provider_id: "qwen3".to_string(),
                enabled: false,
                state: ProviderStatusState::Disabled,
                active_model: None,
                effective_device: None,
                model_revision: Some(config.revision),
                detail: Some(
                    "provider disabled in configuration ([providers.qwen3] enabled=false)"
                        .to_string(),
                ),
            };
        }
        let state = self.state.lock().ok();
        let loaded = state.as_ref().and_then(|s| s.loaded_model.clone());
        let selected_device = state.as_ref().and_then(|s| s.selected_device);
        let error = state.as_ref().and_then(|s| s.load_error.clone());
        let ensuring = state.as_ref().is_some_and(|s| s.ensuring);
        let downloading = state.as_ref().is_some_and(|s| s.downloading);
        let progress = state.as_ref().map(|s| s.download_progress).unwrap_or(0.0);
        drop(state);
        #[cfg(not(feature = "qwen"))]
        let native = false;
        #[cfg(feature = "qwen")]
        let native = true;
        let installed = Self::validate_assets(&config.model_dir).is_ok();
        let ready = loaded.is_some() && (!native || self.is_loaded());
        let state = if self.reload_pending() {
            ProviderStatusState::Loading
        } else if ready {
            ProviderStatusState::Ready
        } else if error.is_some() {
            ProviderStatusState::Error
        } else if ensuring || installed {
            ProviderStatusState::Loading
        } else {
            ProviderStatusState::NotInstalled
        };
        let device = selected_device
            .or_else(|| Self::resolve_device(config.device).ok())
            .map(|d| d.as_str().to_string());
        let detail = error.or_else(|| {
            if self.reload_pending() {
                Some("Reconfiguration waiting for active model work to stop".into())
            } else if ready {
                None
            } else if ensuring && progress >= 1.0 {
                Some("Qwen assets verified; loading native model".into())
            } else if ensuring && downloading {
                Some(format!(
                    "qwen3 weights downloading ({:.0}% complete)",
                    progress * 100.0
                ))
            } else if ensuring {
                Some(format!(
                    "qwen3 weights verifying cache ({:.0}% complete)",
                    progress * 100.0
                ))
            } else if !native {
                Some(
                    "native Candle backend not compiled; rebuild with `--features qwen`"
                        .to_string(),
                )
            } else if installed {
                Some("qwen3 weights present; loading on first use".to_string())
            } else {
                Some(format!(
                    "qwen3 weights not installed in '{}' (see docs/providers.md)",
                    config.model_dir.display()
                ))
            }
        });
        ProviderStatus {
            provider_id: "qwen3".to_string(),
            enabled: true,
            state,
            active_model: loaded.or(Some(config.model_id)),
            effective_device: device,
            model_revision: Some(config.revision),
            detail,
        }
    }

    fn load(&self, model_id: &str) -> Result<(), ProviderError> {
        Self::check_model(model_id)?;
        // Manifest-gated variants fail fast with guidance (see qwen_manifest).
        qwen_manifest(model_id)?;
        let config = self.config_snapshot();
        Self::resolve_device(config.device)?;
        Self::validate_assets(&config.model_dir)?;
        self.load_native(model_id)
    }

    fn unload(&self) {
        let Ok(_lifecycle) = self.inference_lock.lock() else {
            return;
        };
        #[cfg(feature = "qwen")]
        if let Ok(mut worker) = self.worker.lock()
            && let Some(handle) = worker.take()
        {
            handle.unload();
        }
        if let Ok(mut state) = self.state.lock() {
            state.loaded_model = None;
            state.selected_device = None;
        }
    }

    fn invalidate_voice(&self, voice: &str) {
        if let Ok(mut pending) = self.invalidations.lock() {
            if pending.len() >= 1024 {
                pending.clear();
                pending.insert("*".into());
            } else {
                pending.insert(Self::voice_prefix(voice));
            }
        }
        #[cfg(feature = "qwen")]
        if let Ok(worker) = self.worker.try_lock()
            && let Some(worker) = worker.as_ref()
        {
            worker.invalidate();
        }
    }

    fn prepare_reference(
        &self,
        model_id: &str,
        reference_pcm: &[f32],
        reference_rate: u32,
        transcript: Option<&str>,
        mode: CloneMode,
    ) -> Result<String, ProviderError> {
        Self::check_model(model_id)?;
        Self::validate_reference(reference_pcm, reference_rate)?;
        if mode == CloneMode::Icl {
            match transcript.map(str::trim) {
                Some(t) if !t.is_empty() => {}
                _ => {
                    return Err(ProviderError::invalid_voice(
                        "qwen3 icl mode requires the exact reference transcript",
                    ));
                }
            }
        }
        let hash = format!("{reference_rate}:{}", Self::reference_hash(reference_pcm));
        let key = self.conditioning_key(&hash, transcript, mode);
        // Eagerly materialize the prompt when a model is resident so
        // invalid references fail here, not at synthesis time. Without a
        // loaded model the key is still derived (lazy path in synthesize).
        // Note: blocks the calling thread on the worker; async callers must
        // use `spawn_blocking` (all HTTP synthesis paths already do).
        #[cfg(feature = "qwen")]
        if self.is_loaded() {
            let _guard = crate::tts::control::lock_cancellable(&self.inference_lock)
                .map_err(ProviderError::inference_failed)?;
            let _ = self.ensure_prompt(reference_pcm, reference_rate, transcript, mode, None)?;
        }
        Ok(key)
    }

    fn synthesize(&self, request: &ProviderRequest) -> Result<PcmAudio, ProviderError> {
        Self::check_model(&request.model)?;
        Self::check_language(&request.language)?;
        let effective = if request.device == DevicePreference::Auto {
            self.state
                .lock()
                .ok()
                .and_then(|s| s.selected_device)
                .unwrap_or(EffectiveDevice::Cpu)
        } else {
            Self::resolve_device(request.device)?
        };
        if request.text.trim().is_empty() {
            return Err(ProviderError::invalid_voice("text cannot be empty"));
        }
        let mode = request.clone_mode.unwrap_or(CloneMode::Icl);
        if mode == CloneMode::Icl
            && request
                .reference_transcript
                .as_deref()
                .map(str::trim)
                .is_none_or(|t| t.is_empty())
        {
            return Err(ProviderError::invalid_voice(
                "qwen3 icl mode requires the exact reference transcript \
                 (use x_vector mode for transcript-free cloning)",
            ));
        }
        // Reference audio travels with the request: concurrent voices can
        // never observe each other's data through a shared mutable slot.
        let reference = match request.reference_audio.as_ref() {
            Some(reference) => {
                Self::validate_reference(&reference.pcm, reference.sample_rate)?;
                reference
            }
            None => {
                return Err(ProviderError::invalid_voice(
                    "qwen3 cloning requires reference audio; synthesize through \
                     an authorized voice profile (POST /api/voices/clones)",
                ));
            }
        };
        let config = self.config_snapshot();
        if !config.enabled {
            return Err(ProviderError::model_missing(
                "qwen3 provider is disabled ([providers.qwen3] enabled=false)",
            ));
        }
        // Stage check: assets must validate before any inference claim.
        if let Err(missing) = Self::validate_assets(&config.model_dir) {
            if let Ok(mut state) = self.state.lock() {
                state.load_error = Some(missing.message().to_string());
            }
            return Err(missing);
        }
        #[cfg(not(feature = "qwen"))]
        {
            let _ = (effective, mode, reference, config);
            Err(ProviderError::model_missing(
                "qwen3 weights validated but this build omits the native Candle backend; \
                 rebuild with `--features qwen` (CPU) for voice cloning",
            ))
        }
        #[cfg(feature = "qwen")]
        {
            if !self.is_loaded() {
                return Err(self.not_loaded_error());
            }
            let _guard = crate::tts::control::lock_cancellable(&self.inference_lock)
                .map_err(ProviderError::inference_failed)?;
            if self.reload_pending() {
                return Err(ProviderError::loading(
                    "model reconfiguration pending; retry after the provider is ready",
                ));
            }
            let config = self.config_snapshot();
            if let Some(control) = crate::tts::control::current() {
                let device = self
                    .state
                    .lock()
                    .ok()
                    .and_then(|s| s.selected_device)
                    .map(|d| d.as_str())
                    .unwrap_or("unknown");
                control.set_context("qwen3", &request.model, &request.voice, device);
            }
            if request.device != DevicePreference::Auto
                && self
                    .state
                    .lock()
                    .ok()
                    .and_then(|s| s.selected_device)
                    .is_some_and(|d| d != effective)
            {
                return Err(ProviderError::device_unavailable(
                    "requested device differs from the resident model; change providers.qwen3.device and wait for reload",
                ));
            }
            if !config.enabled || config.model_id != request.model || !self.is_loaded() {
                return Err(self.not_loaded_error());
            }
            let key = self.ensure_prompt(
                &reference.pcm,
                reference.sample_rate,
                request.reference_transcript.as_deref(),
                mode,
                Some(&request.voice),
            )?;
            let worker = self
                .worker
                .lock()
                .map_err(|_| ProviderError::inference_failed("qwen3 worker lock poisoned"))?;
            let handle = worker.as_ref().ok_or_else(|| self.not_loaded_error())?;
            let (samples, sample_rate) = handle
                .synthesize(
                    request.text.trim().to_string(),
                    key,
                    request.language.clone(),
                    config.max_tokens,
                    request.seed,
                )
                .map_err(|error| {
                    if (error.contains("worker panicked")
                        || error.contains("worker reply lost")
                        || error.contains("worker thread is gone"))
                        && let Ok(mut state) = self.state.lock()
                    {
                        state.loaded_model = None;
                        state.selected_device = None;
                        state.load_error = None;
                    }
                    if error.contains("queue is full") {
                        ProviderError::busy(error)
                    } else {
                        ProviderError::inference_failed(error)
                    }
                })?;
            let _ = effective;
            if samples.is_empty()
                || sample_rate == 0
                || samples.iter().any(|s| !s.is_finite())
                || !samples.iter().any(|s| s.abs() > 1e-4)
            {
                return Err(ProviderError::inference_failed(
                    "qwen3 synthesis produced no audio",
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

    fn provider() -> Arc<QwenProvider> {
        QwenProvider::new(QwenConfig {
            enabled: true,
            ..QwenConfig::default()
        })
    }

    fn with_reference(mut request: ProviderRequest) -> ProviderRequest {
        request.reference_audio = Some(ReferenceAudio {
            pcm: vec![0.1; 24000],
            sample_rate: 24000,
        });
        request
    }

    #[tokio::test]
    async fn busy_reconfiguration_is_retried_without_another_request() {
        let pool = super::super::ProviderPool::new();
        pool.qwen.reconfigure(QwenConfig {
            enabled: true,
            ..Default::default()
        });
        let guard = pool.qwen.inference_lock.lock().unwrap();
        pool.sync_versioned(
            1,
            QwenConfig::default(),
            super::super::gptsovits::GptSovitsConfig::default(),
        );
        assert!(pool.qwen.reload_pending());
        drop(guard);
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while pool.qwen.reload_pending() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(!pool.qwen.status().enabled);
    }
    #[test]
    fn inference_options_do_not_retire_a_model() {
        let p = provider();
        p.state.lock().unwrap().loaded_model = Some(QWEN_MODELS[0].into());
        let mut next = p.config_snapshot();
        next.max_tokens += 1;
        p.sync_config(next);
        assert!(p.state.lock().unwrap().loaded_model.is_some());
    }
    #[test]
    fn stale_load_is_rejected_before_loading_weights() {
        let p = provider();
        let old = p.config_snapshot();
        let mut next = old.clone();
        next.revision = "changed".into();
        p.reconfigure(next);
        assert!(
            p.load_expected(QWEN_MODELS[0], &old)
                .unwrap_err()
                .message()
                .contains("obsolete")
        );
    }
    #[test]
    fn incompatible_models_are_rejected() {
        let p = provider();
        for model in ["tts-1", "supertonic-3", "qwen3-tts-customvoice", ""] {
            let request = ProviderRequest::new(model, "profile", "hello", "en");
            let error = p.synthesize(&request).unwrap_err();
            assert_eq!(error.code(), "unsupported_model", "{model}");
        }
    }

    #[test]
    fn manifest_gate_rejects_unverified_variant() {
        let error = qwen_manifest("qwen3-tts-1.7b-base").unwrap_err();
        assert_eq!(error.code(), "model_missing");
        assert!(qwen_manifest("qwen3-tts-0.6b-base").is_ok());
    }

    #[test]
    fn warm_cache_reports_verifying_not_downloading() {
        // An ensure pass that verified cache without fetching must never
        // claim a download: warm boots show "verifying", not "downloading".
        let p = QwenProvider::new(QwenConfig {
            enabled: true,
            ..QwenConfig::default()
        });
        if let Ok(mut state) = p.state.lock() {
            state.ensuring = true;
            state.downloading = false;
            state.download_progress = 0.5;
        }
        let status = p.status();
        assert!(status.enabled);
        let detail = status.detail.unwrap_or_default();
        assert!(detail.contains("verifying"), "{detail}");
        assert!(!detail.contains("downloading"), "{detail}");
        if let Ok(mut state) = p.state.lock() {
            state.ensuring = false;
        }
    }

    #[test]
    fn disabled_provider_never_starts_a_download() {
        // Only Supertonic weights download by default: a disabled
        // provider ignores kicks and stays `disabled`, never `loading`.
        let p = QwenProvider::new(QwenConfig {
            enabled: false,
            ..QwenConfig::default()
        });
        p.kick_ensure_if_needed(None);
        assert!(!p.is_ensuring());
        let status = p.status();
        assert!(!status.enabled);
        assert_eq!(status.state, ProviderStatusState::Disabled);
    }

    #[test]
    fn required_assets_include_speech_tokenizer() {
        assert!(QWEN_REQUIRED_FILES.contains(&"speech_tokenizer/model.safetensors"));
        assert!(QWEN_REQUIRED_FILES.contains(&"model.safetensors"));
        assert!(QWEN_REQUIRED_FILES.contains(&"vocab.json"));
    }

    #[test]
    fn japanese_is_supported_but_unknown_languages_are_not() {
        let p = provider();
        // Language check runs before the asset check: use check path via
        // synthesize and assert the language error surfaces for `xx`.
        let mut request = ProviderRequest::new(QWEN_MODELS[0], "profile", "hello", "xx");
        request.clone_mode = Some(CloneMode::XVector);
        let error = p.synthesize(&with_reference(request)).unwrap_err();
        assert_eq!(error.code(), "unsupported_language");
        // `ja` passes language validation (then fails on missing assets or
        // an unloaded model — both explicit, never a fallback).
        let mut request = ProviderRequest::new(QWEN_MODELS[0], "profile", "こんにちは", "ja");
        request.clone_mode = Some(CloneMode::XVector);
        let error = p.synthesize(&with_reference(request)).unwrap_err();
        assert!(
            matches!(error.code(), "model_missing" | "loading"),
            "unexpected: {}",
            error
        );
    }

    #[test]
    fn icl_without_transcript_is_rejected() {
        let p = provider();
        let request = ProviderRequest::new(QWEN_MODELS[0], "profile", "hello", "en");
        let error = p.synthesize(&request).unwrap_err();
        assert_eq!(error.code(), "invalid_voice");
        assert!(error.message().contains("transcript"));
    }

    #[test]
    fn cloning_without_request_reference_audio_is_rejected() {
        let p = provider();
        let mut request = ProviderRequest::new(QWEN_MODELS[0], "profile", "hello", "en");
        request.clone_mode = Some(CloneMode::XVector);
        let error = p.synthesize(&request).unwrap_err();
        assert_eq!(error.code(), "invalid_voice");
        assert!(error.message().contains("reference audio"));
    }

    #[test]
    fn cuda_without_feature_is_an_explicit_device_error() {
        let p = provider();
        let mut request = ProviderRequest::new(QWEN_MODELS[0], "profile", "hello", "en");
        request.clone_mode = Some(CloneMode::XVector);
        request.device = DevicePreference::Cuda;
        let request = with_reference(request);
        let error = p.synthesize(&request).unwrap_err();
        if cfg!(feature = "qwen-cuda") {
            assert!(matches!(error.code(), "model_missing" | "loading"));
        } else {
            assert_eq!(error.code(), "device_unavailable");
            assert_eq!(error.http_status(), 503);
        }
    }

    #[test]
    fn reference_duration_bounds_are_enforced() {
        assert!(QwenProvider::validate_reference(&vec![0.1; 24000], 24000).is_ok());
        assert!(QwenProvider::validate_reference(&vec![0.1; 100], 24000).is_err());
        assert!(QwenProvider::validate_reference(&vec![0.1; 24000 * 60], 24000).is_err());
        assert!(QwenProvider::validate_reference(&[], 24000).is_err());
    }

    #[test]
    fn missing_bundle_names_the_absent_files() {
        let dir = std::env::temp_dir().join(format!("qwen-missing-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let error = QwenProvider::validate_assets(&dir).unwrap_err();
        assert_eq!(error.code(), "model_missing");
        assert!(error.message().contains("config.json"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prepare_reference_derives_stable_keys_without_a_model() {
        let p = provider();
        let pcm = vec![0.1; 48000];
        let a = p
            .prepare_reference(QWEN_MODELS[0], &pcm, 16000, Some("hello"), CloneMode::Icl)
            .unwrap();
        let b = p
            .prepare_reference(QWEN_MODELS[0], &pcm, 16000, Some("hello"), CloneMode::Icl)
            .unwrap();
        assert_eq!(a, b);
        let c = p
            .prepare_reference(
                QWEN_MODELS[0],
                &vec![0.2; 48000],
                16000,
                Some("hello"),
                CloneMode::Icl,
            )
            .unwrap();
        assert_ne!(a, c);
    }
}
