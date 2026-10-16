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
    tokenizer::TextTokenizer,
};
use crate::tts::{
    control::InferenceControl,
    providers::types::{CloneMode, PcmAudio, ProviderRequest},
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
        let candidate = match settings.device {
            Device::Auto if cfg!(feature = "qwen-gguf-vulkan") => 1,
            Device::Auto if cfg!(feature = "qwen-gguf-cuda") => 2,
            Device::Auto | Device::Cpu => 0,
            Device::Vulkan => 1,
            Device::Cuda => 2,
        };
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
        let (runtime, device, fallback_reason) = match load(candidate) {
            Ok(runtime) => (runtime, candidate, None),
            Err(error) if candidate != 0 => (
                load(0)?,
                0,
                Some(format!(
                    "GPU initialization failed; CPU selected before synthesis: {error}"
                )),
            ),
            Err(error) => return Err(error),
        };
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
            let pcm = crate::tts::audio::resample_pcm_linear(
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
        let started = Instant::now();
        let prompt = conditioning::prompt(
            &self.tokenizer,
            &mut self.assets,
            &conditioning,
            &request.text,
            &request.language,
        )?;
        metrics.prompt_secs = started.elapsed().as_secs_f64();
        let seed = request.seed.unwrap_or_else(rand::random);
        let started = Instant::now();
        let generated = generation::generate(
            &mut self.runtime,
            &mut self.assets,
            prompt,
            self.settings.max_tokens,
            Sampling {
                seed,
                ..Default::default()
            },
            Sampling {
                seed: seed.wrapping_add(1),
                repetition_penalty: 1.0,
                ..Default::default()
            },
        )?;
        metrics.generation_secs = started.elapsed().as_secs_f64();
        metrics.frames = generated.codes.len();
        metrics.reached_eos = generated.reached_eos;
        ensure!(
            generated.reached_eos,
            "generation_limit: talker reached max_tokens without EOS; audio withheld"
        );
        let started = Instant::now();
        let samples = self.codec.decode(&generated.codes)?;
        metrics.decode_secs = started.elapsed().as_secs_f64();
        metrics.audio_secs = samples.len() as f64 / 24_000.0;
        Ok(Synthesis {
            audio: PcmAudio::mono(samples, 24_000),
            metrics,
        })
    }
}
