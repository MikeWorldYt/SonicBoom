//! Opt-in tests of the actual converted artifacts using SonicBoom's ORT.
//! QWEN_GGUF_MODEL_DIR must point to the immutable, verified audit bundle.
use std::path::{Path, PathBuf};

use ort::{session::Session, value::Tensor};

fn model_dir() -> PathBuf {
    std::env::var_os("QWEN_GGUF_MODEL_DIR")
        .map(PathBuf::from)
        .expect("set QWEN_GGUF_MODEL_DIR to the downloaded model bundle")
}

fn session(dir: &Path, name: &str) -> anyhow::Result<Session> {
    Ok(Session::builder()?
        .with_intra_threads(2)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .with_inter_threads(1)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .commit_from_file(dir.join(name))?)
}

#[test]
#[ignore = "requires pinned real ONNX model files; tests CPU execution, not speech quality"]
fn int8_onnx_components_execute_on_shared_runtime() -> anyhow::Result<()> {
    let dir = model_dir();
    let mut speaker = session(&dir, "qwen3_tts_speaker_encoder.int8.onnx")?;
    let mels = Tensor::from_array(([1, 93, 128], vec![-4.0f32; 93 * 128]))?;
    let result = speaker.run(ort::inputs!["mels" => mels])?;
    let (shape, embedding) = result["spk_emb"].try_extract_tensor::<f32>()?;
    assert_eq!(&**shape, &[1, 2048]);
    assert!(embedding.iter().all(|v| v.is_finite()));
    drop(result);
    drop(speaker);

    let mut codec = session(&dir, "qwen3_tts_codec_encoder.int8.onnx")?;
    let pcm: Vec<f32> = (0..24_000)
        .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 24_000.0).sin() * 0.1)
        .collect();
    let result =
        codec.run(ort::inputs!["input_values" => Tensor::from_array(([1, 24_000], pcm))?])?;
    let (shape, codes) = result["audio_codes"].try_extract_tensor::<i64>()?;
    assert_eq!(shape.len(), 3);
    assert_eq!(shape[0], 1);
    assert_eq!(shape[2], 16);
    assert!(shape[1] > 0);
    assert!(codes.iter().all(|&v| (0..2048).contains(&v)));
    let frames = shape[1] as usize;
    let codes = codes.to_vec();
    drop(result);
    drop(codec);

    let mut decoder = session(&dir, "qwen3_tts_decoder.int8.onnx")?;
    let mut inputs = vec![
        (
            "audio_codes".to_owned(),
            Tensor::from_array(([1, frames, 16], codes))?.into_dyn(),
        ),
        (
            "pre_conv_history".to_owned(),
            Tensor::from_array(([1, 512, 0], Vec::<f32>::new()))?.into_dyn(),
        ),
        (
            "latent_buffer".to_owned(),
            Tensor::from_array(([1, 1024, 0], Vec::<f32>::new()))?.into_dyn(),
        ),
        (
            "conv_history".to_owned(),
            Tensor::from_array(([1, 1024, 0], Vec::<f32>::new()))?.into_dyn(),
        ),
        (
            "is_last".to_owned(),
            Tensor::from_array(([1], vec![1.0f32]))?.into_dyn(),
        ),
    ];
    for kind in ["key", "value"] {
        for layer in 0..8 {
            inputs.push((
                format!("past_{kind}_{layer}"),
                Tensor::from_array(([1, 16, 0, 64], Vec::<f32>::new()))?.into_dyn(),
            ));
        }
    }
    let result = decoder.run(inputs)?;
    let (shape, pcm) = result["final_wav"].try_extract_tensor::<f32>()?;
    let (_, valid) = result["valid_samples"].try_extract_tensor::<i64>()?;
    assert_eq!(shape[0], 1);
    assert!(!pcm.is_empty());
    assert!(pcm.iter().all(|v| v.is_finite()));
    assert_eq!(pcm.len(), frames * 1920);
    assert!(valid[0] >= 0 && valid[0] as usize <= pcm.len());
    eprintln!(
        "ORT {}: encoded {frames} frames; decoded {} finite samples",
        ort::info(),
        pcm.len()
    );
    Ok(())
}

#[cfg(feature = "qwen-gguf")]
#[test]
#[ignore = "requires pinned GGUF/ONNX bundle and test-owned reference WAV/transcript"]
fn native_worker_reuses_conditioning_and_recovers_after_cancellation() -> anyhow::Result<()> {
    use sonicboom::tts::{
        control::{self, InferenceControl},
        providers::{
            qwen_gguf::worker::{Device, Settings, Worker},
            types::{CloneMode, ProviderRequest, ReferenceAudio},
        },
    };
    use std::time::Duration;
    let dir = model_dir();
    let mut wav = hound::WavReader::open(std::env::var("QWEN_REFERENCE_WAV")?)?;
    let spec = wav.spec();
    anyhow::ensure!(
        spec.channels == 1 && spec.sample_format == hound::SampleFormat::Int,
        "test reference must be integer mono WAV"
    );
    let scale = 2f32.powi(spec.bits_per_sample as i32 - 1);
    let pcm = wav
        .samples::<i32>()
        .map(|v| v.map(|v| v as f32 / scale))
        .collect::<Result<Vec<_>, _>>()?;
    let mut request = ProviderRequest::new(
        "qwen3-tts-1.7b-base",
        "test-owned-reference",
        "Hello, this is a native voice cloning test.",
        "en",
    );
    request.clone_mode = Some(CloneMode::Icl);
    request.seed = Some(42);
    request.reference_transcript = Some(std::env::var("QWEN_REFERENCE_TRANSCRIPT")?);
    request.reference_audio = Some(ReferenceAudio {
        pcm,
        sample_rate: spec.sample_rate,
    });
    let worker = Worker::spawn(Settings {
        dir,
        device: Device::Cpu,
        gpu_layers: 0,
        threads: 4,
        max_tokens: 256,
        cache_capacity: 2,
    })?;
    assert_eq!(worker.ready.effective_device, "cpu");
    let first = worker.synthesize(request.clone())?;
    assert!(!first.metrics.reference_cache_hit);
    assert!(first.metrics.reached_eos && first.audio.is_audible());
    assert_eq!(first.audio.sample_rate, 24_000);
    assert!(first.audio.samples.iter().all(|v| v.is_finite()));
    let cancelled = InferenceControl::new(Duration::from_secs(300));
    let watcher = cancelled.clone();
    let cancellation = std::thread::spawn(move || {
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(240) {
            let progress = watcher.snapshot();
            if progress.stage == "talker" && progress.frames >= 1 {
                watcher.cancel();
                return true;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        watcher.cancel();
        false
    });
    let result = control::scope(cancelled, || worker.synthesize(request.clone()));
    assert!(
        cancellation.join().unwrap(),
        "must cancel after a generated native frame"
    );
    assert!(result.unwrap_err().to_string().contains("cancelled"));
    let second = worker.synthesize(request)?;
    assert!(second.metrics.reference_cache_hit);
    assert_eq!(
        first.audio.samples, second.audio.samples,
        "same seed/reference must reproduce CPU samples"
    );
    eprintln!("first={:?}; reused={:?}", first.metrics, second.metrics);
    drop(worker); // joins owner thread and releases model/session ownership.
    Ok(())
}
