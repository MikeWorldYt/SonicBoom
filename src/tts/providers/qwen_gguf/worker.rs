//! One owner thread for all native state, bounded admission and synchronous replies.
//! A cancelled caller retains the worker/model until the native call returns.
use super::{
    cache::ReferenceCache,
    codec::{Codec, check_cancelled},
    conditioning::{self, MelExtractor, Reference},
    generation::{self, Sampling},
    metrics::Metrics,
    models::{self, Embeddings},
    runtime::Runtime,
    sampling,
    tokenizer::TextTokenizer,
};
use crate::tts::{
    control::InferenceControl,
    providers::types::{CloneMode, PcmAudio, ProviderRequest, SamplingRequest},
    quality::{self, QualityPolicy},
};
use anyhow::{Context, Result, anyhow, ensure};
use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    Auto,
    Cpu,
    Vulkan,
    Cuda,
}
#[derive(Debug, Clone)]
pub struct Settings {
    pub dir: PathBuf,
    pub device: Device,
    pub gpu_layers: i32,
    pub threads: usize,
    pub max_tokens: usize,
    pub cache_capacity: usize,
    /// Permit a CPU fallback when an explicit `device` fails to initialize.
    pub allow_cpu_fallback: bool,
    /// Default sampling used when a request carries no override.
    pub sampling: SamplingRequest,
    /// Quality detection/regeneration policy.
    pub quality: QualityPolicy,
}

/// Ordered native device ids to attempt and whether a CPU fallback is
/// permitted when an earlier candidate fails.
///
/// `Auto` prefers each compiled GPU backend, then CPU, and always permits the
/// fallback (a reason is reported when a GPU was attempted first). An explicit
/// device selects exactly that backend and only falls back to CPU when the
/// operator opts in via `allow_cpu_fallback`; otherwise the failure is
/// surfaced instead of silently masking a broken GPU backend.
fn device_plan(
    requested: Device,
    allow_cpu_fallback: bool,
    vulkan: bool,
    cuda: bool,
) -> (Vec<i32>, bool) {
    match requested {
        Device::Cpu => (vec![0], false),
        Device::Vulkan if allow_cpu_fallback => (vec![1, 0], true),
        Device::Cuda if allow_cpu_fallback => (vec![2, 0], true),
        Device::Vulkan => (vec![1], false),
        Device::Cuda => (vec![2], false),
        Device::Auto => {
            let mut candidates = Vec::with_capacity(3);
            if vulkan {
                candidates.push(1);
            }
            if cuda {
                candidates.push(2);
            }
            candidates.push(0);
            (candidates, true)
        }
    }
}

fn device_name(id: i32) -> &'static str {
    match id {
        1 => "vulkan",
        2 => "cuda",
        _ => "cpu",
    }
}
#[derive(Debug, Clone)]
pub struct Ready {
    pub effective_device: String,
    pub fallback_reason: Option<String>,
    pub load_secs: f64,
}
#[derive(Debug)]
pub struct Synthesis {
    pub audio: PcmAudio,
    pub metrics: Metrics,
}
enum Job {
    Synthesize(
        Box<ProviderRequest>,
        Arc<InferenceControl>,
        mpsc::SyncSender<Result<Synthesis, String>>,
    ),
    Invalidate,
    Shutdown,
}
pub struct Worker {
    tx: mpsc::SyncSender<Job>,
    thread: Option<JoinHandle<()>>,
    pub ready: Ready,
}
impl Worker {
    pub fn spawn(settings: Settings) -> Result<Self> {
        ensure!(
            (1..=1024).contains(&settings.max_tokens) && (1..=256).contains(&settings.threads),
            "invalid Qwen generation/thread limits"
        );
        let (tx, rx) = mpsc::sync_channel::<Job>(2);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("qwen3-gguf-inference".into())
            .spawn(move || {
                let loaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    State::load(settings)
                }));
                let mut state = match loaded {
                    Ok(Ok(state)) => state,
                    Ok(Err(error)) => {
                        let _ = ready_tx.send(Err(error.to_string()));
                        return;
                    }
                    Err(_) => {
                        let _ =
                            ready_tx.send(Err("Qwen native worker panicked during load".into()));
                        return;
                    }
                };
                if ready_tx.send(Ok(state.ready.clone())).is_err() {
                    return;
                }
                while let Ok(job) = rx.recv() {
                    match job {
                        Job::Shutdown => break,
                        Job::Invalidate => state.cache.clear(),
                        Job::Synthesize(request, control, reply) => {
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    crate::tts::control::scope(control, || {
                                        state.synthesize(*request)
                                    })
                                }));
                            match result {
                                Ok(result) => {
                                    let _ = reply.send(result.map_err(|e| e.to_string()));
                                }
                                Err(_) => {
                                    let _ = reply.send(Err(
                                        "Qwen native worker panicked; reload required".into(),
                                    ));
                                    break;
                                }
                            }
                        }
                    }
                }
                // State and all native objects are dropped here, on their owner thread.
            })?;
        match ready_rx.recv() {
            Ok(Ok(ready)) => Ok(Self {
                tx,
                thread: Some(thread),
                ready,
            }),
            response => {
                let _ = thread.join();
                Err(anyhow!(match response {
                    Ok(Err(error)) => error,
                    _ => "Qwen worker lost load reply".into(),
                }))
            }
        }
    }
    pub fn synthesize(&self, request: ProviderRequest) -> Result<Synthesis> {
        let control = crate::tts::control::current()
            .unwrap_or_else(|| InferenceControl::new(Duration::from_secs(600)));
        let (tx, rx) = mpsc::sync_channel(1);
        self.tx
            .try_send(Job::Synthesize(Box::new(request), control, tx))
            .map_err(|e| anyhow!("Qwen worker queue unavailable: {e}"))?;
        // Cooperative cancellation is evaluated on the owner thread. Never detach
        // an executing native job by returning early from this blocking wait.
        rx.recv()
            .context("Qwen worker reply lost; reload required")?
            .map_err(|e| anyhow!(e))
    }
    pub fn invalidate(&self) -> Result<()> {
        self.tx
            .try_send(Job::Invalidate)
            .map_err(|e| anyhow!("Qwen invalidation queue unavailable: {e}"))
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.tx.send(Job::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct State {
    settings: Settings,
    ready: Ready,
    runtime: Runtime,
    codec: Codec,
    assets: Embeddings,
    tokenizer: TextTokenizer,
    mel: MelExtractor,
    cache: ReferenceCache,
}
impl State {
    fn load(settings: Settings) -> Result<Self> {
        let _load = crate::tts::providers::worker::model_load_guard();
        crate::tts::providers::assets::verify_local_files(&settings.dir, &models::manifest())?;
        let started = Instant::now();
        let load = |device| -> Result<Runtime> {
            let mut runtime =
                Runtime::load(&settings.dir, device, settings.gpu_layers, settings.threads)?;
            // Real backend computation is part of initialization, before any request.
            runtime
                .talker
                .decode(&vec![0.0; 2048], 0)
                .map_err(|e| anyhow!("talker initialization test: {e}"))?;
            runtime
                .predictor
                .decode(&vec![0.0; 2048], 0)
                .map_err(|e| anyhow!("predictor initialization test: {e}"))?;
            runtime.reset()?;
            Ok(runtime)
        };
        // Try each permitted device in order. An explicit device that fails
        // without `allow_cpu_fallback` returns an actionable error instead of
        // quietly switching to CPU.
        let (candidates, allow_cpu_fallback) = device_plan(
            settings.device,
            settings.allow_cpu_fallback,
            cfg!(feature = "qwen-gguf-vulkan"),
            cfg!(feature = "qwen-gguf-cuda"),
        );
        let mut last_gpu_error: Option<String> = None;
        let mut selected: Option<(Runtime, i32, Option<String>)> = None;
        for candidate in candidates {
            match load(candidate) {
                Ok(runtime) => {
                    // Report a fallback only when a GPU was attempted first.
                    let fallback_reason = (candidate == 0)
                        .then(|| last_gpu_error.take())
                        .flatten()
                        .map(|error| {
                            format!(
                                "GPU initialization failed; CPU selected before synthesis: {error}"
                            )
                        });
                    selected = Some((runtime, candidate, fallback_reason));
                    break;
                }
                Err(error) => {
                    let text = format!("{error:#}");
                    if candidate == 0 {
                        // CPU is the last resort: its failure is terminal.
                        return Err(error);
                    }
                    if !allow_cpu_fallback {
                        return Err(anyhow!(
                            "requested device `{}` could not be initialized: {text}; \
                             set providers.qwen3_gguf.allow_cpu_fallback = true to run on \
                             CPU instead, or use device = \"auto\"",
                            device_name(candidate)
                        ));
                    }
                    last_gpu_error = Some(text);
                }
            }
        }
        let (runtime, device, fallback_reason) = selected.ok_or_else(|| {
            anyhow!(
                "no usable native device could be initialized: {}",
                last_gpu_error.unwrap_or_else(|| "unknown native failure".into())
            )
        })?;
        let codec = Codec::load(&settings.dir)?;
        let assets = Embeddings::load(&settings.dir)?;
        let tokenizer = TextTokenizer::load(&settings.dir)?;
        let ready = Ready {
            effective_device: match device {
                1 => "vulkan",
                2 => "cuda",
                _ => "cpu",
            }
            .into(),
            fallback_reason,
            load_secs: started.elapsed().as_secs_f64(),
        };
        let cache = ReferenceCache::new(settings.cache_capacity);
        Ok(Self {
            settings,
            ready,
            runtime,
            codec,
            assets,
            tokenizer,
            mel: MelExtractor::default(),
            cache,
        })
    }
    fn synthesize(&mut self, request: ProviderRequest) -> Result<Synthesis> {
        check_cancelled()?;
        ensure!(
            request.model == "qwen3-tts-1.7b-base",
            "GGUF/ONNX supports only qwen3-tts-1.7b-base"
        );
        ensure!(
            request.clone_mode.unwrap_or(CloneMode::Icl) == CloneMode::Icl,
            "GGUF/ONNX supports validated ICL only; x-vector is not validated"
        );
        let transcript = request
            .reference_transcript
            .as_deref()
            .filter(|t| !t.trim().is_empty())
            .context("ICL requires the exact reference transcript")?;
        let reference = request
            .reference_audio
            .as_ref()
            .context("ICL requires reference audio")?;
        ensure!(
            reference.sample_rate > 0 && reference.pcm.iter().all(|v| v.is_finite()),
            "invalid reference PCM"
        );
        let seconds = reference.pcm.len() as f64 / reference.sample_rate as f64;
        ensure!(
            (1.0..=30.0).contains(&seconds),
            "reference must be 1–30 seconds"
        );
        let key = ReferenceCache::key(
            &request.voice,
            &reference.pcm,
            reference.sample_rate,
            transcript,
        );
        let mut metrics = Metrics {
            load_secs: self.ready.load_secs,
            ..Default::default()
        };
        let started = Instant::now();
        let conditioning = if let Some(reference) = self.cache.get(&key) {
            metrics.reference_cache_hit = true;
            reference
        } else {
            let pcm = crate::tts::audio::resample_reference(
                &reference.pcm,
                reference.sample_rate,
                24_000,
            );
            let speaker = self.codec.speaker(self.mel.extract(&pcm)?)?;
            let codes = self.codec.encode(&pcm)?;
            self.cache.insert(
                key,
                Reference {
                    speaker,
                    codes,
                    transcript: transcript.to_string(),
                },
            )
        };
        metrics.conditioning_secs = started.elapsed().as_secs_f64();
        // Sampling: an explicit per-request override wins; otherwise the
        // configured default (the pinned `balanced` preset).
        let sampling_request = request.sampling.as_ref().unwrap_or(&self.settings.sampling);
        let resolved = sampling::resolve(Some(sampling_request), request.seed)
            .map_err(|detail| anyhow!("invalid sampling: {detail}"))?;
        metrics.sampling_preset = resolved.preset.to_string();
        metrics.seed = resolved.seed;
        if let Some(control) = crate::tts::control::current() {
            control.set_seed(resolved.seed);
        }
        // Quality policy: a per-request mode overrides the configured mode,
        // but retries stay bounded by the shared hard ceiling.
        let policy = match request.quality {
            Some(mode) => QualityPolicy::new(mode, quality::MAX_RETRIES_CEILING),
            None => self.settings.quality,
        };
        let mode = policy.mode;
        let mut current = self.run_attempt(
            &conditioning,
            &request.text,
            &request.language,
            transcript,
            reseed(&resolved.talker, resolved.seed),
            reseed(&resolved.predictor, resolved.predictor.seed),
            mode,
        )?;
        metrics.prompt_secs += current.prompt_secs;
        metrics.generation_secs += current.generation_secs;
        metrics.decode_secs += current.decode_secs;
        metrics.frames = current.frames;
        metrics.reached_eos = true;
        // Bounded, reproducible regeneration: at most one extra attempt, with a
        // derived seed, only for a confident failure. Invalid requests and
        // structural errors never reach here (they already returned).
        let mut retries = 0usize;
        while policy.allows_retry(retries, current.report.is_confident_failure()) {
            check_cancelled()?;
            retries += 1;
            let retry_seed = resolved
                .seed
                .wrapping_add((retries as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let attempt = self.run_attempt(
                &conditioning,
                &request.text,
                &request.language,
                transcript,
                reseed(&resolved.talker, retry_seed),
                reseed(&resolved.predictor, retry_seed.wrapping_add(1)),
                mode,
            );
            match attempt {
                Ok(candidate) => {
                    metrics.prompt_secs += candidate.prompt_secs;
                    metrics.generation_secs += candidate.generation_secs;
                    metrics.decode_secs += candidate.decode_secs;
                    // Adopt the retry only when it is not worse: prefer a
                    // non-confident result, else the attempt with fewer
                    // findings. Failed candidate audio is never published when
                    // the first attempt is better.
                    let better = if candidate.report.is_confident_failure()
                        == current.report.is_confident_failure()
                    {
                        candidate.report.issues.len() <= current.report.issues.len()
                    } else {
                        !candidate.report.is_confident_failure()
                    };
                    if better {
                        metrics.frames = candidate.frames;
                        current = candidate;
                    }
                }
                Err(error) => {
                    // A cancelled/deadline-exceeded retry aborts the request;
                    // any other retry failure keeps the first valid result.
                    if error.to_string().contains("cancel") {
                        return Err(error);
                    }
                }
            }
        }
        let mut report = current.report;
        report.retries = retries;
        metrics.audio_secs = current.samples.len() as f64 / 24_000.0;
        if let Some(control) = crate::tts::control::current() {
            control.set_quality(report);
        }
        Ok(Synthesis {
            audio: PcmAudio::mono(current.samples, 24_000),
            metrics,
        })
    }

    /// One generation + decode attempt with the deterministic quality pass.
    /// Requires EOS (a reached-limit result is withheld, matching the legacy
    /// contract) and never leaks state between attempts: `generate` resets the
    /// talker and clears the predictor at its start.
    #[allow(clippy::too_many_arguments)]
    fn run_attempt(
        &mut self,
        conditioning: &Reference,
        text: &str,
        language: &str,
        transcript: &str,
        talker: Sampling,
        predictor: Sampling,
        mode: quality::DetectionMode,
    ) -> Result<Attempt> {
        let started = Instant::now();
        let prompt = conditioning::prompt(
            &self.tokenizer,
            &mut self.assets,
            conditioning,
            text,
            language,
        )?;
        let prompt_secs = started.elapsed().as_secs_f64();
        let started = Instant::now();
        let generated = generation::generate(
            &mut self.runtime,
            &mut self.assets,
            prompt,
            self.settings.max_tokens,
            talker,
            predictor,
        )?;
        let generation_secs = started.elapsed().as_secs_f64();
        ensure!(
            generated.reached_eos,
            "generation_limit: talker reached max_tokens without EOS; audio withheld"
        );
        let started = Instant::now();
        let samples = self.codec.decode(&generated.codes)?;
        let decode_secs = started.elapsed().as_secs_f64();
        // Deterministic in-path detection: no ASR model runs here, so echo is
        // not claimed (asr_required=true); audio health is fully measured.
        let report = quality::evaluate(&samples, 24_000, transcript, text, None, mode);
        Ok(Attempt {
            samples,
            report,
            prompt_secs,
            generation_secs,
            decode_secs,
            frames: generated.codes.len(),
        })
    }
}

/// One generation+decode attempt and its deterministic quality report.
struct Attempt {
    samples: Vec<f32>,
    report: quality::QualityReport,
    prompt_secs: f64,
    generation_secs: f64,
    decode_secs: f64,
    frames: usize,
}

/// Copy `base`'s sampling parameters with a different seed, leaving `base`
/// reusable for a bounded retry.
fn reseed(base: &Sampling, seed: u64) -> Sampling {
    Sampling {
        temperature: base.temperature,
        top_k: base.top_k,
        top_p: base.top_p,
        repetition_penalty: base.repetition_penalty,
        seed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_prefers_compiled_gpus_then_cpu_and_may_fall_back() {
        // Only Vulkan compiled: try Vulkan, then CPU, fallback permitted.
        assert_eq!(
            device_plan(Device::Auto, false, true, false),
            (vec![1, 0], true)
        );
        // Both compiled: deterministic priority Vulkan -> CUDA -> CPU.
        assert_eq!(
            device_plan(Device::Auto, false, true, true),
            (vec![1, 2, 0], true)
        );
        // Only CUDA compiled.
        assert_eq!(
            device_plan(Device::Auto, false, false, true),
            (vec![2, 0], true)
        );
        // No GPU compiled: CPU only, still no GPU reason to report.
        assert_eq!(
            device_plan(Device::Auto, false, false, false),
            (vec![0], true)
        );
    }

    #[test]
    fn explicit_devices_select_exactly_one_backend_unless_opted_in() {
        // CPU is exact and never falls back.
        assert_eq!(device_plan(Device::Cpu, true, true, true), (vec![0], false));
        // Explicit GPU: no CPU fallback by default...
        assert_eq!(
            device_plan(Device::Vulkan, false, true, false),
            (vec![1], false)
        );
        assert_eq!(
            device_plan(Device::Cuda, false, false, true),
            (vec![2], false)
        );
        // ...only when allow_cpu_fallback is set.
        assert_eq!(
            device_plan(Device::Vulkan, true, true, false),
            (vec![1, 0], true)
        );
        assert_eq!(
            device_plan(Device::Cuda, true, false, true),
            (vec![2, 0], true)
        );
    }

    #[test]
    fn device_names_match_the_native_shim_ids() {
        assert_eq!(device_name(0), "cpu");
        assert_eq!(device_name(1), "vulkan");
        assert_eq!(device_name(2), "cuda");
    }
}
