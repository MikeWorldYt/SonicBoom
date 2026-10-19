//! Opt-in native real-model smoke/benchmark. No Python or external inference.
//! QWEN_GGUF_MODEL_DIR, QWEN_REFERENCE_WAV, QWEN_REFERENCE_TRANSCRIPT required.
//! QWEN_GGUF_DEVICE=cpu|vulkan|cuda (default cpu), QWEN_GGUF_OUTPUT optional.
use anyhow::{Context, Result, ensure};
use sonicboom::tts::providers::qwen_gguf::{
    codec::Codec,
    conditioning::{self, MelExtractor, Reference},
    generation::{self, Sampling},
    models::{self, Embeddings},
    runtime::Runtime,
    tokenizer::TextTokenizer,
};
use std::{path::PathBuf, time::Instant};

fn main() -> Result<()> {
    let dir =
        PathBuf::from(std::env::var("QWEN_GGUF_MODEL_DIR").context("set QWEN_GGUF_MODEL_DIR")?);
    sonicboom::tts::providers::assets::verify_local_files(&dir, &models::manifest())?;
    let wav = PathBuf::from(std::env::var("QWEN_REFERENCE_WAV").context("set QWEN_REFERENCE_WAV")?);
    let mut wav = hound::WavReader::open(wav)?;
    let spec = wav.spec();
    ensure!(spec.channels == 1, "reference WAV must be mono");
    let pcm: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => wav
            .samples::<f32>()
            .collect::<std::result::Result<_, _>>()?,
        hound::SampleFormat::Int => wav
            .samples::<i32>()
            .map(|v| v.map(|v| v as f32 / (1u64 << (spec.bits_per_sample - 1)) as f32))
            .collect::<std::result::Result<_, _>>()?,
    };
    let pcm = sonicboom::tts::audio::resample_pcm_linear(&pcm, spec.sample_rate, 24_000);
    let transcript = std::env::var("QWEN_REFERENCE_TRANSCRIPT")
        .context("set exact QWEN_REFERENCE_TRANSCRIPT")?;
    let text = std::env::var("QWEN_GGUF_TEXT")
        .unwrap_or_else(|_| "Hello, this is a native voice cloning test.".into());
    let language = std::env::var("QWEN_GGUF_LANGUAGE").unwrap_or_else(|_| "en".into());
    let device = std::env::var("QWEN_GGUF_DEVICE").unwrap_or_else(|_| "cpu".into());
    let device_id = match device.as_str() {
        "cpu" => 0,
        "vulkan" => 1,
        "cuda" => 2,
        _ => anyhow::bail!("unknown device {device}"),
    };
    let discovered = sonicboom_qwen_llama::runtime_backends();
    eprintln!("Runtime backends: {discovered}");
    if device == "vulkan" {
        ensure!(discovered.contains("vulkan: "), "no usable Vulkan device");
    }
    let started = Instant::now();
    let mut runtime = Runtime::load(&dir, device_id, -1, 4)?;
    let mut codec = Codec::load(&dir)?;
    let mut assets = Embeddings::load(&dir)?;
    let tokenizer = TextTokenizer::load(&dir)?;
    let load_secs = started.elapsed().as_secs_f64();
    eprintln!("native contexts and sessions loaded in {load_secs:.3}s ({device}; ONNX cpu)");
    let started = Instant::now();
    let reference = Reference {
        speaker: codec.speaker(MelExtractor::default().extract(&pcm)?)?,
        codes: codec.encode(&pcm)?,
        transcript,
    };
    let conditioning_secs = started.elapsed().as_secs_f64();
    let prompt = conditioning::prompt(&tokenizer, &mut assets, &reference, &text, &language)?;
    let seed = std::env::var("QWEN_GGUF_SEED")
        .ok()
        .map(|v| v.parse())
        .transpose()?
        .unwrap_or(42);
    let max_tokens = std::env::var("QWEN_GGUF_MAX_TOKENS")
        .ok()
        .map(|v| v.parse())
        .transpose()?
        .unwrap_or(256);
    let started = Instant::now();
    let generated = generation::generate(
        &mut runtime,
        &mut assets,
        prompt,
        max_tokens,
        Sampling {
            seed,
            ..Default::default()
        },
        Sampling {
            seed: seed + 1,
            repetition_penalty: 1.0,
            ..Default::default()
        },
    )?;
    let generation_secs = started.elapsed().as_secs_f64();
    let started = Instant::now();
    let pcm = codec.decode(&generated.codes)?;
    let decode_secs = started.elapsed().as_secs_f64();
    ensure!(
        !pcm.is_empty() && pcm.iter().all(|v| v.is_finite()),
        "empty/non-finite inference output"
    );
    runtime.reset()?;
    drop(runtime); // release both model/context handles before declaring success
    let audio_secs = pcm.len() as f64 / 24_000.0;
    let output = std::env::var("QWEN_GGUF_OUTPUT")
        .unwrap_or_else(|_| "sonicboom-qwen-gguf-smoke.wav".into());
    let mut writer = hound::WavWriter::create(
        &output,
        hound::WavSpec {
            channels: 1,
            sample_rate: 24_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )?;
    for &v in &pcm {
        writer.write_sample(v)?;
    }
    writer.finalize()?;
    println!(
        "{}",
        serde_json::json!({"device":device,"runtime_backends":discovered,"cleanup":true,"onnx_device":"cpu","model_revision":models::REVISION,"seed":seed,"frames":generated.codes.len(),"reached_eos":generated.reached_eos,"load_secs":load_secs,"conditioning_secs":conditioning_secs,"generation_secs":generation_secs,"decode_secs":decode_secs,"audio_secs":audio_secs,"rtf":(generation_secs+decode_secs)/audio_secs,"finite":pcm.iter().all(|v| v.is_finite()),"peak":pcm.iter().copied().map(f32::abs).fold(0.0f32,f32::max),"output":output})
    );
    Ok(())
}
