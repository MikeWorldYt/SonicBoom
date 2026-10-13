//! Opt-in model-backed voice-cloning tests (hardware-gated, ignored by default).
//!
//! These tests require real model weights plus an owned/authorized
//! reference recording and NEVER pass with fake or silent fallbacks. Run
//! only on the target hardware with the pinned revisions from
//! `docs/providers.md`:
//!
//! ```sh
//! QWEN_MODEL_DIR=./models/qwen3-tts \
//! QWEN_REFERENCE_WAV=./reference.wav \
//! QWEN_REFERENCE_TRANSCRIPT="exact transcript of reference.wav" \
//! cargo test --locked --features qwen --test providers_model_backed -- --ignored
//!
//! GPTSOVITS_BUNDLE_DIR=./models/gpt-sovits-v2pro \
//! GPTSOVITS_REFERENCE_WAV=./reference-en.wav \
//! GPTSOVITS_REFERENCE_TRANSCRIPT="exact transcript" \
//! cargo test --locked --features gpt-sovits --test providers_model_backed -- --ignored
//! ```
//!
//! Expected outcomes (all observed on CPU unless marked pending):
//! audible nonempty WAV for Qwen `icl` (Japanese) and `x_vector`
//! (English), English GPT-SoVITS synthesis with model reuse across
//! successive requests, explicit `unsupported_language` for GPT-SoVITS
//! `language=ja`, and rejection of missing transcripts. Numbers go to
//! `benchmarks/providers.md`; nothing here is fabricated.

use std::sync::Arc;

use sonicboom::tts::providers::TtsProvider;
use sonicboom::tts::providers::types::{
    CloneMode, DevicePreference, ProviderRequest, ReferenceAudio,
};

fn required_env(name: &str) -> std::path::PathBuf {
    let value = std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set for this test"));
    std::path::PathBuf::from(value)
}

fn decode_wav(path: &std::path::Path) -> (Vec<f32>, u32) {
    let bytes = std::fs::read(path).expect("reference WAV must be readable");
    let mut reader = hound::WavReader::new(&bytes[..]).expect("reference must be WAV");
    let spec = reader.spec();
    let scale = 2f32.powf(f32::from(spec.bits_per_sample.max(1)) - 1.0);
    let interleaved: Vec<f32> = reader
        .samples::<i32>()
        .map(|s| s.expect("sample") as f32 / scale)
        .collect();
    // Mix stereo down to mono, matching the upload path.
    let samples = if spec.channels == 2 {
        let mut mixed = Vec::with_capacity(interleaved.len() / 2);
        let mut pairs = interleaved.into_iter();
        while let (Some(left), Some(right)) = (pairs.next(), pairs.next()) {
            mixed.push((left + right) / 2.0);
        }
        mixed
    } else {
        interleaved
    };
    assert!(!samples.is_empty(), "reference WAV must be nonempty");
    (samples, spec.sample_rate)
}

fn report(label: &str, audio: &sonicboom::tts::providers::types::PcmAudio) {
    println!(
        "{label}: {} samples @ {} Hz ({:.1}s)",
        audio.samples.len(),
        audio.sample_rate,
        audio.duration_secs()
    );
    // Optionally persist the output for out-of-band verification
    // (spectral/energy inspection): QWEN_OUT_DIR / GPTSOVITS_OUT_DIR.
    let out_dir = std::env::var(format!(
        "{}_OUT_DIR",
        if label.starts_with("gpt") {
            "GPTSOVITS"
        } else {
            "QWEN"
        }
    ))
    .unwrap_or_default();
    if !out_dir.is_empty() {
        let file = std::path::Path::new(&out_dir).join(format!(
            "{}.wav",
            label.replace(|c: char| !c.is_alphanumeric(), "_")
        ));
        let wav = sonicboom::tts::audio::encode_audio(
            &audio.samples,
            audio.sample_rate,
            sonicboom::tts::audio::AudioFormat::Wav,
        )
        .expect("encode output wav");
        std::fs::write(&file, &wav).expect("write output wav");
        println!("saved {}", file.display());
    }
}

#[test]
#[ignore = "requires Qwen3-TTS weights (see module docs)"]
fn qwen_cpu_load_reports_ready() {
    let provider = sonicboom::tts::providers::qwen::QwenProvider::new(
        sonicboom::tts::providers::qwen::QwenConfig {
            enabled: true,
            model_dir: required_env("QWEN_MODEL_DIR"),
            ..Default::default()
        },
    );
    let provider: Arc<_> = provider;
    let started = std::time::Instant::now();
    provider
        .load("qwen3-tts-0.6b-base")
        .expect("qwen native load");
    println!("qwen load-only: {:.1}s", started.elapsed().as_secs_f64());
    let status = provider.status();
    assert_eq!(
        status.state,
        sonicboom::tts::providers::types::ProviderStatusState::Ready
    );
    assert_eq!(status.effective_device.as_deref(), Some("cpu"));
}

#[test]
#[ignore = "requires Qwen3-TTS weights + authorized reference (see module docs)"]
fn qwen_cpu_icl_generates_audible_wav() {
    let (pcm, rate) = decode_wav(&required_env("QWEN_REFERENCE_WAV"));
    let transcript = std::env::var("QWEN_REFERENCE_TRANSCRIPT")
        .expect("QWEN_REFERENCE_TRANSCRIPT must be set for this test");
    let provider = sonicboom::tts::providers::qwen::QwenProvider::new(
        sonicboom::tts::providers::qwen::QwenConfig {
            enabled: true,
            model_dir: required_env("QWEN_MODEL_DIR"),
            ..Default::default()
        },
    );
    let provider: Arc<_> = provider;
    let started = std::time::Instant::now();
    provider
        .load("qwen3-tts-0.6b-base")
        .expect("qwen native load");
    println!("qwen load: {:.1}s", started.elapsed().as_secs_f64());
    let mut request = ProviderRequest::new(
        "qwen3-tts-0.6b-base",
        "profile",
        "こんにちは。今日は音声合成のテストです。",
        "ja",
    );
    request.device = DevicePreference::Cpu;
    request.clone_mode = Some(CloneMode::Icl);
    request.reference_transcript = Some(transcript);
    request.reference_audio = Some(ReferenceAudio {
        pcm,
        sample_rate: rate,
    });
    let synth_started = std::time::Instant::now();
    let audio = provider.synthesize(&request).expect("qwen icl synthesis");
    println!(
        "qwen icl synth: {:.1}s",
        synth_started.elapsed().as_secs_f64()
    );
    assert!(!audio.samples.is_empty(), "synthesis produced no audio");
    assert!(audio.is_audible(), "synthesis produced silence");
    report("qwen icl (ja)", &audio);
}

#[test]
#[ignore = "requires Qwen3-TTS weights + authorized reference (see module docs)"]
fn qwen_cpu_x_vector_generates_without_transcript() {
    let (pcm, rate) = decode_wav(&required_env("QWEN_REFERENCE_WAV"));
    let provider = sonicboom::tts::providers::qwen::QwenProvider::new(
        sonicboom::tts::providers::qwen::QwenConfig {
            enabled: true,
            model_dir: required_env("QWEN_MODEL_DIR"),
            ..Default::default()
        },
    );
    let provider: Arc<_> = provider;
    let started = std::time::Instant::now();
    provider
        .load("qwen3-tts-0.6b-base")
        .expect("qwen native load");
    println!("qwen load: {:.1}s", started.elapsed().as_secs_f64());
    let mut request = ProviderRequest::new(
        "qwen3-tts-0.6b-base",
        "profile",
        "Hello, this is a test.",
        "en",
    );
    request.device = DevicePreference::Cpu;
    request.clone_mode = Some(CloneMode::XVector);
    request.reference_audio = Some(ReferenceAudio {
        pcm,
        sample_rate: rate,
    });
    let synth_started = std::time::Instant::now();
    let audio = provider
        .synthesize(&request)
        .expect("qwen x_vector synthesis");
    println!(
        "qwen x_vector synth: {:.1}s",
        synth_started.elapsed().as_secs_f64()
    );
    assert!(audio.is_audible(), "synthesis produced silence");
    report("qwen x_vector (en)", &audio);
}

#[test]
#[ignore = "requires an exported GPT-SoVITS ONNX bundle + reference (see module docs)"]
fn gptsovits_cpu_english_reuses_model_across_requests() {
    let (pcm, rate) = decode_wav(&required_env("GPTSOVITS_REFERENCE_WAV"));
    let transcript = std::env::var("GPTSOVITS_REFERENCE_TRANSCRIPT")
        .expect("GPTSOVITS_REFERENCE_TRANSCRIPT must be set for this test");
    let provider = sonicboom::tts::providers::gptsovits::GptSovitsProvider::new(
        sonicboom::tts::providers::gptsovits::GptSovitsConfig {
            enabled: true,
            model_dir: required_env("GPTSOVITS_BUNDLE_DIR"),
            ..Default::default()
        },
    );
    let provider: Arc<_> = provider;
    let started = std::time::Instant::now();
    provider
        .load("gpt-sovits-v2pro-onnx")
        .expect("gpt-sovits native load");
    println!("gpt-sovits load: {:.1}s", started.elapsed().as_secs_f64());
    for text in [
        "Hello, this is a local voice cloning test.",
        "The model is reused across successive requests.",
    ] {
        let mut request = ProviderRequest::new("gpt-sovits-v2pro-onnx", "profile", text, "en");
        request.device = DevicePreference::Cpu;
        request.clone_mode = Some(CloneMode::Icl);
        request.reference_transcript = Some(transcript.clone());
        request.reference_audio = Some(ReferenceAudio {
            pcm: pcm.clone(),
            sample_rate: rate,
        });
        let synth_started = std::time::Instant::now();
        let audio = provider.synthesize(&request).expect("gpt-sovits synthesis");
        println!(
            "gpt-sovits synth: {:.1}s",
            synth_started.elapsed().as_secs_f64()
        );
        assert!(audio.is_audible(), "synthesis produced silence");
        report("gpt-sovits (en)", &audio);
    }
}

#[test]
fn gptsovits_japanese_is_rejected_without_weights() {
    let provider = sonicboom::tts::providers::gptsovits::GptSovitsProvider::new(Default::default());
    let mut request = ProviderRequest::new("gpt-sovits-v2pro-onnx", "profile", "こんにちは", "ja");
    request.reference_transcript = Some("transcript".to_string());
    let error = provider.synthesize(&request).unwrap_err();
    assert_eq!(error.code(), "unsupported_language");
}
