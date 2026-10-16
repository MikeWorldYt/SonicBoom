//! Explicit offline diagnostics; never logs text or raw references.
use anyhow::{Context, Result, bail, ensure};
use serde_json::json;
use sha2::{Digest, Sha256};
use sonicboom::tts::{
    audio::{AudioFormat, encode_audio},
    providers::{PcmAudio, ProviderRequest, TtsProvider, types::ReferenceAudio},
};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};
fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn reference(path: &Path) -> Result<ReferenceAudio> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    ensure!(spec.channels == 1, "reference must be mono");
    let pcm = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()?,
        hound::SampleFormat::Int => reader
            .samples::<i32>()
            .map(|v| v.map(|s| s as f32 / 2f32.powi(spec.bits_per_sample as i32 - 1)))
            .collect::<std::result::Result<Vec<_>, _>>()?,
    };
    ensure!(pcm.iter().all(|s| s.is_finite()), "non-finite reference");
    Ok(ReferenceAudio {
        pcm,
        sample_rate: spec.sample_rate,
    })
}
enum Engine {
    Mock,
    Pocket(std::sync::Arc<sonicboom::tts::providers::pocket_tts::PocketTtsProvider>),
    #[cfg(feature = "qwen-gguf")]
    Qwen(sonicboom::tts::providers::qwen_gguf::worker::Worker),
}
impl Engine {
    fn load(provider: &str, model: &str, dir: &Path) -> Result<Self> {
        match provider {
            "mock" => Ok(Self::Mock),
            "pocket" => {
                use sonicboom::tts::providers::pocket_tts::{PocketTtsConfig, PocketTtsProvider};
                let p = PocketTtsProvider::new(PocketTtsConfig {
                    enabled: true,
                    model_dir: dir.into(),
                    model_id: model.into(),
                    ..Default::default()
                });
                p.load(model)?;
                Ok(Self::Pocket(p))
            }
            #[cfg(feature = "qwen-gguf")]
            "qwen" => {
                use sonicboom::tts::providers::qwen_gguf::worker::{Device, Settings, Worker};
                Ok(Self::Qwen(Worker::spawn(Settings {
                    dir: dir.into(),
                    device: Device::Cpu,
                    gpu_layers: 0,
                    threads: 4,
                    max_tokens: 256,
                    cache_capacity: 2,
                })?))
            }
            _ => bail!("unknown or uncompiled provider {provider}"),
        }
    }
    fn synthesize(&self, request: &ProviderRequest) -> Result<(PcmAudio, serde_json::Value)> {
        match self {
            Self::Mock => {
                // Encodes target identity, not speech. Never a quality score.
                let bytes = Sha256::digest(request.text.as_bytes());
                Ok((
                    PcmAudio::mono(
                        bytes.iter().map(|v| (*v as f32 - 128.0) / 256.0).collect(),
                        24000,
                    ),
                    json!({"termination":"mock", "cache_hit":false}),
                ))
            }
            Self::Pocket(p) => Ok((
                p.synthesize(request)?,
                json!({"termination":"not_exposed", "cache_hit":null}),
            )),
            #[cfg(feature = "qwen-gguf")]
            Self::Qwen(w) => {
                let output = w.synthesize(request.clone())?;
                Ok((
                    output.audio,
                    json!({"frames":output.metrics.frames,"termination":if output.metrics.reached_eos {"eos"} else {"limit"},"cache_hit":output.metrics.reference_cache_hit}),
                ))
            }
        }
    }
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() < 6 || args.iter().any(|arg| arg == "--help") {
        println!(
            "tts-quality-audit PROVIDER MODEL LANGUAGE MODEL_DIR NEW_OUTPUT_DIR [AUTHORIZED_REFERENCE_WAV]\nProviders: mock, pocket, qwen (feature required). Reference positional argument explicitly certifies authorized diagnostic use. Qwen requires exact QWEN_REFERENCE_TRANSCRIPT. No outputs overwritten. CPU, seed 42, default sampling. Writes hashed inputs only."
        );
        return Ok(());
    }
    let (provider, model, language, dir, out) = (
        &args[1],
        &args[2],
        &args[3],
        Path::new(&args[4]),
        PathBuf::from(&args[5]),
    );
    let mut request = ProviderRequest::new(model, "audit-authorized-A", "", language);
    request.seed = Some(42);
    let reference_hash = if provider != "mock" {
        let path = Path::new(args.get(6).context("supply authorized reference WAV")?);
        let bytes = fs::read(path)?;
        request.reference_audio = Some(reference(path)?);
        Some(hash(&bytes))
    } else {
        None
    };
    if provider == "qwen" {
        request.clone_mode = Some(sonicboom::tts::providers::CloneMode::Icl);
        request.reference_transcript = Some(
            std::env::var("QWEN_REFERENCE_TRANSCRIPT")
                .context("set exact QWEN_REFERENCE_TRANSCRIPT")?,
        );
    }
    fs::create_dir(&out).context("output directory must be fresh; create parent beforehand")?;
    let mut manifest = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out.join("manifest.jsonl"))?;
    let revision = match provider.as_str() {
        "pocket" => sonicboom::tts::providers::pocket_tts::POCKET_TTS_UPSTREAM_REVISION,
        "qwen" => "0a36d692562f83a862466b0bf8c12154ddf1965d",
        _ => "mock-v1",
    };
    let git = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()?;
    let mut engine = Engine::load(provider, model, dir)?;
    let mut test_hash = None;
    let mut index = String::from(
        "<!doctype html><meta charset=utf-8><title>TTS audit</title><p>Speech-content and listening review pending. Mock audio is not speech.</p>",
    );
    for (i, text) in [
        "hola",
        "test",
        "mundo",
        "test",
        "test",
        "¿Hola?",
        "sí",
        "no",
        "¿Qué tal?",
        "hola test",
        "Hola mundo. Esto es una prueba.",
    ]
    .iter()
    .enumerate()
    {
        if i == 3 {
            drop(engine);
            engine = Engine::load(provider, model, dir)?;
        }
        request.text = (*text).into();
        let started = Instant::now();
        let (audio, metrics) = engine.synthesize(&request)?;
        let elapsed = started.elapsed().as_secs_f64();
        ensure!(
            !audio.samples.is_empty() && audio.samples.iter().all(|s| s.is_finite()),
            "invalid PCM"
        );
        let wav = encode_audio(&audio.samples, audio.sample_rate, AudioFormat::Wav)?;
        let reader = hound::WavReader::new(std::io::Cursor::new(&wav))?;
        ensure!(
            reader.len() as usize == audio.samples.len(),
            "WAV sample count mismatch"
        );
        let wav_hash = hash(&wav);
        if i == 1 {
            test_hash = Some(wav_hash.clone());
        }
        let name = format!("{:02}.wav", i + 1);
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(out.join(&name))?
            .write_all(&wav)?;
        let n = audio.samples.len() as f64;
        let rms = (audio
            .samples
            .iter()
            .map(|s| (*s as f64).powi(2))
            .sum::<f64>()
            / n)
            .sqrt();
        let row = json!({"request_id":uuid::Uuid::new_v4().to_string(),"job_id":null,"boundary":"direct-provider","provider":provider,"model":model,"revision":revision,"git_sha":String::from_utf8_lossy(&git.stdout).trim(),"effective_device":"cpu","language":language,"voice":"audit-authorized-A","reference_hash":reference_hash,"text_hash":hash(text.as_bytes()),"seed":42,"chunk_index":null,"sample_count":audio.samples.len(),"sample_rate":audio.sample_rate,"wav_hash":wav_hash,"output":name,"elapsed_secs":elapsed,"duration_secs":audio.duration_secs(),"rtf":elapsed/audio.duration_secs(),"peak":audio.samples.iter().fold(0f32,|a,b|a.max(b.abs())),"rms":rms,"dc_offset":audio.samples.iter().map(|s| *s as f64).sum::<f64>()/n,"clipping_fraction":audio.samples.iter().filter(|s|s.abs()>=1.0).count() as f64/n,"same_as_first_test":if *text=="test" {Some(test_hash.as_ref()==Some(&wav_hash))}else{None},"metrics":metrics,"content_review":"NOT RUN"});
        writeln!(manifest, "{row}")?;
        manifest.flush()?;
        index.push_str(&format!(
            "<p>{:02} <audio controls src=\"{name}\"></audio></p>",
            i + 1
        ));
        eprintln!(
            "{}: {:.2}s, {:.2}s audio",
            name,
            elapsed,
            audio.duration_secs()
        );
    }
    fs::write(out.join("listen.html"), index)?;
    Ok(())
}
