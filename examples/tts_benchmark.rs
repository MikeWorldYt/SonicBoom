//! Explicit real-model benchmark. Ordinary CI only compiles this example.
//! Generates owned reference speech with native Supertonic; never reads user profiles.
use sonicboom::{
    engine::{EngineConfig, SonicBoomEngine, SynthesisRequest},
    tts::{
        control::{InferenceControl, scope},
        providers::{
            TtsProvider,
            qwen::{QwenConfig, QwenProvider},
            types::{CloneMode, DevicePreference, ProviderRequest, ReferenceAudio},
        },
    },
};
use std::time::{Duration, Instant};

fn memory() -> serde_json::Value {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let value = |name: &str| {
        status
            .lines()
            .find(|l| l.starts_with(name))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|n| n.parse::<u64>().ok())
    };
    serde_json::json!({ "rss_kib": value("VmRSS:"), "process_peak_rss_kib": value("VmHWM:") })
}
fn decode(bytes: &[u8]) -> anyhow::Result<ReferenceAudio> {
    let mut wav = hound::WavReader::new(bytes)?;
    let spec = wav.spec();
    anyhow::ensure!(spec.channels == 1, "benchmark expects mono reference");
    let pcm = if spec.sample_format == hound::SampleFormat::Float {
        wav.samples::<f32>().collect::<Result<Vec<_>, _>>()?
    } else {
        let scale = 2f32.powi(i32::from(spec.bits_per_sample) - 1);
        wav.samples::<i32>()
            .map(|s| s.map(|v| v as f32 / scale))
            .collect::<Result<Vec<_>, _>>()?
    };
    Ok(ReferenceAudio {
        pcm,
        sample_rate: spec.sample_rate,
    })
}
fn report(
    case: &str,
    elapsed: Duration,
    audio: Option<&sonicboom::tts::providers::PcmAudio>,
    provider: &QwenProvider,
) {
    println!(
        "{}",
        serde_json::json!({ "case": case, "elapsed_secs": elapsed.as_secs_f64(),
        "audio_secs": audio.map(|a| a.duration_secs()), "rtf": audio.map(|a| elapsed.as_secs_f64() / a.duration_secs()),
        "memory": memory(), "provider": provider.status(), "diagnostics": provider.diagnostics() })
    );
}
fn synth(provider: &QwenProvider, request: &ProviderRequest, label: &str) -> anyhow::Result<()> {
    let started = Instant::now();
    let audio = provider.synthesize(request)?;
    anyhow::ensure!(
        !audio.samples.is_empty()
            && audio.is_audible()
            && audio.samples.iter().all(|s| s.is_finite()),
        "invalid audio"
    );
    report(label, started.elapsed(), Some(&audio), provider);
    Ok(())
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    #[cfg(feature = "server")]
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();
    let case = std::env::args().nth(1).unwrap_or_else(|| "load".into());
    if case == "gpt" {
        return gpt_benchmark().await;
    }
    let model_dir = std::env::var("QWEN_MODEL_DIR").unwrap_or_else(|_| "./models/qwen3-tts".into());
    let device =
        DevicePreference::parse(&std::env::var("QWEN_DEVICE").unwrap_or_else(|_| "cpu".into()))
            .ok_or_else(|| anyhow::anyhow!("invalid QWEN_DEVICE"))?;
    let provider = QwenProvider::new(QwenConfig {
        enabled: true,
        model_dir: model_dir.into(),
        device,
        ..Default::default()
    });
    let started = Instant::now();
    provider.load("qwen3-tts-0.6b-base")?;
    report("cold_model_load", started.elapsed(), None, &provider);
    let started = Instant::now();
    provider.load("qwen3-tts-0.6b-base")?;
    report("warm_model_reuse", started.elapsed(), None, &provider);
    if case == "load" {
        return Ok(());
    }
    let engine = SonicBoomEngine::prepare(EngineConfig::new("./models"), |_| {}).await?;
    let transcript = "Hello, this is an authorized synthetic reference recording for testing native voice cloning.";
    let make_reference = |voice: &str| -> anyhow::Result<ReferenceAudio> {
        let audio = engine.synthesize(&SynthesisRequest {
            text: transcript.into(),
            language: "en".into(),
            voice: voice.into(),
            inference_steps: 5,
        })?;
        decode(&audio.wav)
    };
    let reference = make_reference("F1")?;
    let second = if case == "all" {
        Some(make_reference("M1")?)
    } else {
        None
    };
    drop(engine);
    let mut request = ProviderRequest::new(
        "qwen3-tts-0.6b-base",
        "owned-benchmark-F1",
        "Hello, this is a test.",
        "en",
    );
    request.reference_audio = Some(reference.clone());
    request.device = device;
    request.seed = Some(42);
    request.clone_mode = Some(CloneMode::XVector);
    if matches!(case.as_str(), "all" | "x_vector") {
        synth(&provider, &request, "x_vector")?;
        synth(&provider, &request, "same_voice_warm")?;
    }
    if matches!(case.as_str(), "all" | "icl") {
        request.clone_mode = Some(CloneMode::Icl);
        request.reference_transcript = Some(transcript.into());
        let started = Instant::now();
        provider.prepare_reference(
            &request.model,
            &reference.pcm,
            reference.sample_rate,
            Some(transcript),
            CloneMode::Icl,
        )?;
        report("icl_preparation", started.elapsed(), None, &provider);
        synth(&provider, &request, "icl")?;
    }
    if case == "all" {
        request.clone_mode = Some(CloneMode::XVector);
        request.reference_transcript = None;
        request.voice = "owned-benchmark-M1".into();
        request.reference_audio = second;
        synth(&provider, &request, "different_voice")?;
        request.text = "This longer passage exercises the native generation loop, its stopping conditions, and the memory bounds. We expect complete speech and an explicit error if the configured frame limit is reached before the end of the passage.".into();
        synth(&provider, &request, "long_text")?;
    }
    if matches!(case.as_str(), "all" | "cancel") {
        request.clone_mode = Some(CloneMode::XVector);
        request.reference_transcript = None;
        request.reference_audio = Some(reference);
        request.text =
            "This speech should be cancelled during active generation before it finishes.".into();
        let control = InferenceControl::new(Duration::from_secs(180));
        let monitor = control.clone();
        let canceller = std::thread::spawn(move || {
            while monitor.snapshot().frames < 1 && !monitor.stopped() {
                std::thread::sleep(Duration::from_millis(5));
            }
            let started = Instant::now();
            monitor.cancel();
            started
        });
        let started = Instant::now();
        let outcome = scope(control.clone(), || provider.synthesize(&request));
        let cancelled_at = canceller.join().expect("cancellation monitor");
        anyhow::ensure!(
            outcome.is_err() && control.snapshot().frames >= 1,
            "active generation cancellation was not demonstrated"
        );
        println!(
            "{}",
            serde_json::json!({ "case": "active_cancellation", "elapsed_secs": started.elapsed().as_secs_f64(), "acknowledgement_secs": cancelled_at.elapsed().as_secs_f64(), "progress": control.snapshot(), "memory": memory(), "diagnostics": provider.diagnostics() })
        );
        request.text = "Hi there.".into();
        synth(&provider, &request, "healthy_after_cancel")?;
    }
    Ok(())
}

#[cfg(feature = "gpt-sovits")]
async fn gpt_benchmark() -> anyhow::Result<()> {
    use sonicboom::tts::providers::gptsovits::{GptSovitsConfig, GptSovitsProvider};
    let transcript = "Hello, this is an authorized synthetic reference recording for testing native voice cloning.";
    let engine = SonicBoomEngine::prepare(EngineConfig::new("./models"), |_| {}).await?;
    let reference = decode(
        &engine
            .synthesize(&SynthesisRequest {
                text: transcript.into(),
                language: "en".into(),
                voice: "F1".into(),
                inference_steps: 5,
            })?
            .wav,
    )?;
    drop(engine);
    let provider = GptSovitsProvider::new(GptSovitsConfig {
        enabled: true,
        model_dir: std::env::var("GPTSOVITS_BUNDLE_DIR")
            .unwrap_or_else(|_| "models/gpt-sovits-v2pro".into())
            .into(),
        ..Default::default()
    });
    let started = Instant::now();
    provider.load("gpt-sovits-v2pro-onnx")?;
    println!(
        "{}",
        serde_json::json!({ "case": "gpt_cold_load", "elapsed_secs": started.elapsed().as_secs_f64(), "memory": memory(), "provider": provider.status() })
    );
    let mut request = ProviderRequest::new(
        "gpt-sovits-v2pro-onnx",
        "owned-benchmark-F1",
        "Hello, this is a local voice cloning test.",
        "en",
    );
    request.clone_mode = Some(CloneMode::Icl);
    request.reference_transcript = Some(transcript.into());
    request.reference_audio = Some(reference);
    request.seed = Some(42);
    for case in ["gpt_icl", "gpt_same_voice_warm"] {
        let started = Instant::now();
        let audio = provider.synthesize(&request)?;
        anyhow::ensure!(
            audio.is_audible() && audio.samples.iter().all(|s| s.is_finite()),
            "invalid GPT audio"
        );
        println!(
            "{}",
            serde_json::json!({ "case": case, "elapsed_secs": started.elapsed().as_secs_f64(), "audio_secs": audio.duration_secs(), "rtf": started.elapsed().as_secs_f64()/audio.duration_secs(), "memory": memory(), "provider": provider.status() })
        );
    }
    Ok(())
}
#[cfg(not(feature = "gpt-sovits"))]
async fn gpt_benchmark() -> anyhow::Result<()> {
    anyhow::bail!("gpt benchmark requires --features gpt-sovits")
}
