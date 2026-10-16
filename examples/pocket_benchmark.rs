//! CPU-only Pocket benchmark. Requires an authorized mono PCM16 reference.
use sonicboom::tts::providers::{
    TtsProvider,
    pocket_tts::{PocketTtsConfig, PocketTtsProvider},
    types::{ProviderRequest, ReferenceAudio},
};
use std::{path::PathBuf, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 5 {
        return Err(
            "usage: pocket_benchmark BUNDLE_ROOT AUTHORIZED_REFERENCE.wav OUTPUT_DIR TEXT".into(),
        );
    }
    let mut wav = hound::WavReader::open(&args[2])?;
    let spec = wav.spec();
    if spec.channels != 1
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
    {
        return Err("reference must be mono PCM16; convert explicitly before benchmarking".into());
    }
    let reference = ReferenceAudio {
        pcm: wav
            .samples::<i16>()
            .map(|v| v.map(|s| s as f32 / 32768.0))
            .collect::<Result<Vec<_>, _>>()?,
        sample_rate: spec.sample_rate,
    };
    std::fs::create_dir_all(&args[3])?;
    let steps: Vec<usize> = match std::env::var("POCKET_BENCH_STEPS") {
        Ok(value) => vec![value.parse()?],
        Err(_) => vec![1, 2, 4, 8],
    };
    for steps in steps {
        if ![1, 2, 4, 8].contains(&steps) {
            return Err("steps must be 1, 2, 4 or 8".into());
        }
        let provider = PocketTtsProvider::new(PocketTtsConfig {
            enabled: true,
            model_dir: PathBuf::from(&args[1]),
            lsd_steps: steps,
            ..PocketTtsConfig::default()
        });
        let load_start = Instant::now();
        provider.load("pocket-tts-en")?;
        let load_seconds = load_start.elapsed().as_secs_f64();
        for trial in 0..4 {
            let mut request = ProviderRequest::new("pocket-tts-en", "benchmark", &args[4], "en");
            request.seed = Some(49420);
            request.reference_audio = Some(reference.clone());
            let start = Instant::now();
            let audio = provider.synthesize(&request)?;
            let seconds = start.elapsed().as_secs_f64();
            let duration = audio.duration_secs();
            assert!(audio.is_audible() && audio.samples.iter().all(|s| s.is_finite()));
            let path = PathBuf::from(&args[3]).join(format!("lsd-{steps}-trial-{trial}.wav"));
            let mut out = hound::WavWriter::create(
                &path,
                hound::WavSpec {
                    channels: 1,
                    sample_rate: audio.sample_rate,
                    bits_per_sample: 32,
                    sample_format: hound::SampleFormat::Float,
                },
            )?;
            for sample in &audio.samples {
                out.write_sample(*sample)?;
            }
            out.finalize()?;
            // Provider returns the complete waveform; API TTFA equals completion.
            // This is not decoder first-frame latency or streaming TTFA.
            println!(
                "{}",
                serde_json::json!({"steps": steps, "trial": trial,
                "cold_conditioning": trial == 0, "seed": 49420, "load_seconds": load_seconds,
                "synthesis_seconds": seconds, "api_ttfa_seconds": seconds,
                "audio_seconds": duration, "rtf": seconds / duration as f64,
                "process_peak_rss_kib": peak_rss_kib(), "wav": path})
            );
        }
        provider.unload();
    }
    Ok(())
}

fn peak_rss_kib() -> Option<u64> {
    // Linux high-water mark includes loading and previous trials; use a fresh
    // process per step for cross-step memory comparisons.
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find(|line| line.starts_with("VmHWM:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}
