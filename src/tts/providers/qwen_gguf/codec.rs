//! CPU ONNX components. llama.cpp device selection never changes these sessions.
use super::models::{CODEBOOK_SIZE, CODEBOOKS};
use anyhow::{Context, Result, bail, ensure};
use ort::{
    session::Session,
    value::{DynValue, Tensor},
};
use std::path::Path;

pub struct Codec {
    speaker: Session,
    encoder: Session,
    decoder: Session,
}

fn session(dir: &Path, file: &str) -> Result<Session> {
    Session::builder()?
        .with_intra_threads(2)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .with_inter_threads(1)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .commit_from_file(dir.join(file))
        .with_context(|| format!("loading {file} with CPU ONNX Runtime"))
}

pub fn check_cancelled() -> Result<()> {
    if crate::tts::control::current().is_some_and(|c| c.stopped()) {
        bail!("inference cancelled or deadline exceeded");
    }
    Ok(())
}

impl Codec {
    pub fn load(dir: &Path) -> Result<Self> {
        Ok(Self {
            speaker: session(dir, "qwen3_tts_speaker_encoder.int8.onnx")?,
            encoder: session(dir, "qwen3_tts_codec_encoder.int8.onnx")?,
            decoder: session(dir, "qwen3_tts_decoder.int8.onnx")?,
        })
    }

    pub fn speaker(&mut self, mels: Vec<f32>) -> Result<Vec<f32>> {
        check_cancelled()?;
        ensure!(
            !mels.is_empty()
                && mels.len().is_multiple_of(128)
                && mels.iter().all(|v| v.is_finite()),
            "invalid speaker mel features"
        );
        let frames = mels.len() / 128;
        let outputs = self
            .speaker
            .run(ort::inputs!["mels" => Tensor::from_array(([1, frames, 128], mels))?])?;
        let output = outputs
            .get("spk_emb")
            .context("speaker output spk_emb missing")?;
        let (shape, values) = output.try_extract_tensor::<f32>()?;
        ensure!(
            **shape == [1, 2048] && values.iter().all(|v| v.is_finite()),
            "invalid speaker embedding output"
        );
        check_cancelled()?;
        Ok(values.to_vec())
    }

    pub fn encode(&mut self, pcm: &[f32]) -> Result<Vec<[i64; CODEBOOKS]>> {
        check_cancelled()?;
        ensure!(
            (24_000..=720_000).contains(&pcm.len()) && pcm.iter().all(|v| v.is_finite()),
            "reference must be 1–30 seconds of finite 24 kHz mono PCM"
        );
        let outputs = self.encoder.run(
            ort::inputs!["input_values" => Tensor::from_array(([1, pcm.len()], pcm.to_vec()))?],
        )?;
        let output = outputs
            .get("audio_codes")
            .context("codec output audio_codes missing")?;
        let (shape, values) = output.try_extract_tensor::<i64>()?;
        ensure!(
            shape.len() == 3 && shape[0] == 1 && shape[2] == CODEBOOKS as i64 && shape[1] > 0,
            "invalid reference codec shape {shape:?}"
        );
        ensure!(
            values.iter().all(|&v| (0..CODEBOOK_SIZE).contains(&v)),
            "reference codes out of range"
        );
        check_cancelled()?;
        Ok(values.as_chunks::<CODEBOOKS>().0.to_vec())
    }

    /// Request-local decoder history; never shares KV tensors across voices.
    /// This currently returns complete audio, not a public streaming interface.
    pub fn decode(&mut self, codes: &[[i64; CODEBOOKS]]) -> Result<Vec<f32>> {
        ensure!(
            !codes.is_empty() && codes.len() <= 4096,
            "invalid decoder frame count"
        );
        ensure!(
            codes
                .iter()
                .flatten()
                .all(|&v| (0..CODEBOOK_SIZE).contains(&v)),
            "decoder code out of range"
        );
        let mut state = DecoderState::empty()?;
        let mut audio = Vec::with_capacity(codes.len() * 1920);
        let count = codes.len().div_ceil(12);
        for (index, chunk) in codes.chunks(12).enumerate() {
            check_cancelled()?;
            let is_last = index + 1 == count;
            let mut inputs = vec![
                (
                    "audio_codes".into(),
                    Tensor::from_array((
                        [1, chunk.len(), CODEBOOKS],
                        chunk.iter().flatten().copied().collect::<Vec<_>>(),
                    ))?
                    .into_dyn(),
                ),
                (
                    "is_last".into(),
                    Tensor::from_array(([1], vec![if is_last { 1.0f32 } else { 0.0 }]))?.into_dyn(),
                ),
            ];
            inputs.append(&mut state.inputs);
            let outputs = self.decoder.run(inputs)?;
            let output = outputs
                .get("final_wav")
                .context("decoder final_wav missing")?;
            let (shape, wav) = output.try_extract_tensor::<f32>()?;
            ensure!(
                shape.len() == 2 && shape[0] == 1 && wav.iter().all(|v| v.is_finite()),
                "invalid decoder PCM output"
            );
            let (_, valid) = outputs
                .get("valid_samples")
                .context("decoder valid_samples missing")?
                .try_extract_tensor::<i64>()?;
            ensure!(
                valid.len() == 1 && valid[0] >= 0 && valid[0] as usize <= wav.len(),
                "invalid decoder valid_samples"
            );
            // The reference emits all residual samples on the final flush.
            let len = if is_last {
                wav.len()
            } else {
                valid[0] as usize
            };
            audio.extend_from_slice(&wav[..len]);
            if !is_last {
                // Input names are fixed by the decoder graph: reuse the
                // precomputed table instead of formatting 19 strings per
                // 12-frame group. The state tensors themselves must still be
                // copied — ORT outputs borrow from `outputs`, which is
                // dropped after `run`, so zero-copy reuse would dangle.
                let mut inputs = Vec::with_capacity(DECODER_STATE_NAMES.len());
                for (input, output) in DECODER_STATE_NAMES {
                    inputs.push((
                        input.to_owned(),
                        copy_tensor(outputs.get(output).context("decoder history missing")?)?,
                    ));
                }
                state = DecoderState { inputs };
            }
        }
        check_cancelled()?;
        ensure!(!audio.is_empty(), "decoder produced empty PCM");
        Ok(audio)
    }
}

fn copy_tensor(value: &DynValue) -> Result<DynValue> {
    let (shape, data) = value.try_extract_tensor::<f32>()?;
    ensure!(
        data.iter().all(|v| v.is_finite()),
        "non-finite decoder state"
    );
    Ok(Tensor::from_array((shape.to_vec(), data.to_vec()))?.into_dyn())
}

/// Fixed decoder state wiring: `(session input name, session output name)`.
/// The graph carries 3 convolution histories plus 8 layers × (key, value).
/// Order matches the historical construction (conv histories, then key
/// layers 0–7, then value layers 0–7).
const DECODER_STATE_NAMES: [(&str, &str); 19] = [
    ("pre_conv_history", "next_pre_conv_history"),
    ("latent_buffer", "next_latent_buffer"),
    ("conv_history", "next_conv_history"),
    ("past_key_0", "next_key_0"),
    ("past_key_1", "next_key_1"),
    ("past_key_2", "next_key_2"),
    ("past_key_3", "next_key_3"),
    ("past_key_4", "next_key_4"),
    ("past_key_5", "next_key_5"),
    ("past_key_6", "next_key_6"),
    ("past_key_7", "next_key_7"),
    ("past_value_0", "next_value_0"),
    ("past_value_1", "next_value_1"),
    ("past_value_2", "next_value_2"),
    ("past_value_3", "next_value_3"),
    ("past_value_4", "next_value_4"),
    ("past_value_5", "next_value_5"),
    ("past_value_6", "next_value_6"),
    ("past_value_7", "next_value_7"),
];

struct DecoderState {
    inputs: Vec<(String, DynValue)>,
}
impl DecoderState {
    fn empty() -> Result<Self> {
        let mut inputs = Vec::with_capacity(DECODER_STATE_NAMES.len());
        for (input, _) in DECODER_STATE_NAMES {
            let value = match input {
                "pre_conv_history" => {
                    Tensor::from_array(([1, 512, 0], Vec::<f32>::new()))?.into_dyn()
                }
                "latent_buffer" | "conv_history" => {
                    Tensor::from_array(([1, 1024, 0], Vec::<f32>::new()))?.into_dyn()
                }
                _ => Tensor::from_array(([1, 16, 0, 64], Vec::<f32>::new()))?.into_dyn(),
            };
            inputs.push((input.to_owned(), value));
        }
        Ok(Self { inputs })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoder_state_wiring_covers_all_histories_in_order() {
        assert_eq!(DECODER_STATE_NAMES.len(), 19);
        assert_eq!(
            DECODER_STATE_NAMES[0],
            ("pre_conv_history", "next_pre_conv_history")
        );
        // Key layers precede value layers, each 0–7 in order.
        for layer in 0..8 {
            assert_eq!(
                DECODER_STATE_NAMES[3 + layer].0,
                format!("past_key_{layer}")
            );
            assert_eq!(
                DECODER_STATE_NAMES[3 + layer].1,
                format!("next_key_{layer}")
            );
            assert_eq!(
                DECODER_STATE_NAMES[11 + layer].0,
                format!("past_value_{layer}")
            );
            assert_eq!(
                DECODER_STATE_NAMES[11 + layer].1,
                format!("next_value_{layer}")
            );
        }
        // Every output name is unique: no history slot is wired twice.
        let mut outputs: Vec<&str> = DECODER_STATE_NAMES.iter().map(|(_, o)| *o).collect();
        outputs.sort_unstable();
        outputs.dedup();
        assert_eq!(outputs.len(), 19);
    }

    #[test]
    fn empty_decoder_state_matches_wiring_shapes() {
        let state = DecoderState::empty().unwrap();
        assert_eq!(state.inputs.len(), 19);
        for (name, _) in DECODER_STATE_NAMES {
            assert!(state.inputs.iter().any(|(n, _)| n == name));
        }
    }
}
