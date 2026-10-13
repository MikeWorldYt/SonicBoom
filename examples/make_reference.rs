//! Generate a genuine spoken reference WAV with the resident Supertonic
//! model for voice-cloning smoke tests. The transcript is the exact text
//! synthesized, so ICL references are faithful by construction.
//!
//! ```sh
//! cargo run --locked --example make_reference -- \
//!   "Hello, this is a voice cloning reference recording." /tmp/ref-en.wav
//! ```

use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: make_reference <text> <out.wav>");
        std::process::exit(2);
    }
    let (text, out) = (args[1].clone(), PathBuf::from(&args[2]));
    let models = PathBuf::from("models");
    let voices: Vec<(String, PathBuf)> =
        ["F1", "F2", "F3", "F4", "F5", "M1", "M2", "M3", "M4", "M5"]
            .iter()
            .map(|name| {
                (
                    name.to_string(),
                    models.join("voice_styles").join(format!("{name}.json")),
                )
            })
            .collect();
    let paths = sonicboom::tts::download::ModelPaths {
        duration_predictor: models.join("onnx").join("duration_predictor.onnx"),
        text_encoder: models.join("onnx").join("text_encoder.onnx"),
        vector_estimator: models.join("onnx").join("vector_estimator.onnx"),
        vocoder: models.join("onnx").join("vocoder.onnx"),
        unicode_indexer: models.join("onnx").join("unicode_indexer.json"),
        tts_config: models.join("onnx").join("tts.json"),
        voice_files: voices,
    };
    let handle = sonicboom::tts::model::ModelHandle::load(&paths)?;
    let samples = sonicboom::tts::inference::synthesize(&handle, &text, "en", "F1", 5, 200)?;
    let wav = sonicboom::tts::audio::encode_audio(
        &samples,
        handle.sample_rate(),
        sonicboom::tts::audio::AudioFormat::Wav,
    )?;
    std::fs::write(&out, &wav)?;
    println!(
        "wrote {} ({} samples @ {} Hz)",
        out.display(),
        samples.len(),
        handle.sample_rate()
    );
    Ok(())
}
