//! Explicit single-language Pocket test. Downloads only with --download.
//! POCKET_TTS_BUNDLE_DIR and POCKET_TTS_REF_WAV required; model defaults to English.
use anyhow::{Context, Result, ensure};
use sonicboom::tts::{
    audio,
    providers::{
        ProviderRequest, TtsProvider, assets,
        pocket_tts::{PocketTtsConfig, PocketTtsProvider, pocket_manifest, pocket_model_language},
        types::ReferenceAudio,
    },
};
use std::{path::PathBuf, time::Instant};

#[tokio::main]
async fn main() -> Result<()> {
    let dir =
        PathBuf::from(std::env::var("POCKET_TTS_BUNDLE_DIR").context("set POCKET_TTS_BUNDLE_DIR")?);
    let reference = std::env::var("POCKET_TTS_REF_WAV").context("set POCKET_TTS_REF_WAV")?;
    let model = std::env::var("POCKET_TTS_MODEL").unwrap_or_else(|_| "pocket-tts-en".into());
    let language = pocket_model_language(&model).context("unsupported Pocket model")?;
    if std::env::args().any(|s| s == "--download") {
        assets::ensure_pinned_files(
            "pocket-tts",
            &dir,
            &pocket_manifest(&model)?,
            None,
            |_| {},
            || {},
            || false,
        )
        .await?;
    }
    let mut wav = hound::WavReader::open(reference)?;
    let spec = wav.spec();
    ensure!(
        spec.channels == 1
            && spec.bits_per_sample == 16
            && spec.sample_format == hound::SampleFormat::Int,
        "reference must be mono PCM16 WAV"
    );
    let pcm = wav
        .samples::<i16>()
        .map(|v| v.map(|n| n as f32 / 32768.0))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    sonicboom::onnx_runtime::self_test()?;
    let provider = PocketTtsProvider::new(PocketTtsConfig {
        enabled: true,
        model_dir: dir,
        model_id: model.clone(),
        ..Default::default()
    });
    let start = Instant::now();
    provider.load(&model)?;
    let load = start.elapsed().as_secs_f64();
    let mut request = ProviderRequest::new(
        &model,
        "termux-probe",
        "Hello from Android Termux",
        language,
    );
    request.reference_audio = Some(ReferenceAudio {
        pcm,
        sample_rate: spec.sample_rate,
    });
    request.seed = Some(42);
    let start = Instant::now();
    let output = provider.synthesize(&request)?;
    ensure!(
        output.is_audible() && output.samples.iter().all(|s| s.is_finite()),
        "invalid or silent Pocket output"
    );
    let latency = start.elapsed().as_secs_f64();
    let destination =
        std::env::var("POCKET_TTS_OUTPUT").unwrap_or_else(|_| "pocket-termux.wav".into());
    std::fs::write(
        destination,
        audio::encode_audio(&output.samples, output.sample_rate, audio::AudioFormat::Wav)?,
    )?;
    println!(
        "{}",
        serde_json::json!({"model":model, "load_seconds":load, "latency_seconds":latency, "audio_seconds":output.duration_secs(), "real_time_factor":latency/output.duration_secs()})
    );
    provider.unload();
    Ok(())
}
