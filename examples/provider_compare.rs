//! Same-source before/after benchmark using an owned native reference.
use sha2::Digest;
use sonicboom::{
    engine::{EngineConfig, SonicBoomEngine, SynthesisRequest},
    tts::providers::{
        TtsProvider,
        qwen::{QwenConfig, QwenProvider},
        types::{CloneMode, DevicePreference, ProviderRequest, ReferenceAudio},
    },
};
use std::time::Instant;
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path = std::path::PathBuf::from(std::env::var("SONICBOOM_BENCH_REFERENCE")?);
    if std::env::args().nth(1).as_deref() == Some("reference") {
        let engine = SonicBoomEngine::prepare(
            EngineConfig::new(std::env::var("SUPERTONIC_MODEL_DIR")?),
            |_| {},
        )
        .await?;
        let audio = engine.synthesize(&SynthesisRequest { text: "Hello, this is an authorized synthetic reference recording for native voice cloning tests.".into(), language: "en".into(), voice: "F1".into(), inference_steps: 5 })?;
        std::fs::write(path, audio.wav)?;
        return Ok(());
    }
    let bytes = std::fs::read(path)?;
    let mut wav = hound::WavReader::new(&bytes[..])?;
    let rate = wav.spec().sample_rate;
    let scale = 2f32.powi(i32::from(wav.spec().bits_per_sample) - 1);
    let pcm = wav
        .samples::<i32>()
        .map(|s| s.map(|s| s as f32 / scale))
        .collect::<Result<Vec<_>, _>>()?;
    let provider = QwenProvider::new(QwenConfig {
        enabled: true,
        model_dir: std::env::var("QWEN_MODEL_DIR")?.into(),
        device: DevicePreference::Cpu,
        ..Default::default()
    });
    let load = Instant::now();
    provider.load("qwen3-tts-0.6b-base")?;
    println!(
        "{}",
        serde_json::json!({ "case": "verified_model_load", "elapsed_secs": load.elapsed().as_secs_f64() })
    );
    let mut request = ProviderRequest::new(
        "qwen3-tts-0.6b-base",
        "owned-benchmark",
        "Hello, this is a test.",
        "en",
    );
    request.clone_mode = Some(CloneMode::XVector);
    request.reference_audio = Some(ReferenceAudio {
        pcm,
        sample_rate: rate,
    });
    request.seed = Some(42);
    request.device = DevicePreference::Cpu;
    for iteration in 0..3 {
        let started = Instant::now();
        let audio = provider.synthesize(&request)?;
        anyhow::ensure!(
            audio.is_audible() && audio.samples.iter().all(|s| s.is_finite()),
            "invalid audio"
        );
        let elapsed = started.elapsed().as_secs_f64();
        let mut hash = sha2::Sha256::new();
        for sample in &audio.samples {
            hash.update(sample.to_le_bytes());
        }
        println!(
            "{}",
            serde_json::json!({ "iteration": iteration, "elapsed_secs": elapsed, "audio_secs": audio.duration_secs(), "rtf": elapsed/audio.duration_secs(), "sample_sha256": hex::encode(hash.finalize()), "rayon_threads": std::env::var("RAYON_NUM_THREADS").ok() })
        );
    }
    Ok(())
}
