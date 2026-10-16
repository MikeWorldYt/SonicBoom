//! Log-mel extraction matching the pinned reference implementation.
//!
//! Derived from HaujetZhao/Qwen3-TTS-GGUF inference/utils/mel.py (MIT).
//! Copyright (c) 2026 HaujetZhao. See vendor notices for the license.
use anyhow::{Result, ensure};
use rustfft::{FftPlanner, num_complex::Complex};

pub struct MelExtractor {
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
    window: Vec<f32>,
    basis: Vec<f32>,
}

impl Default for MelExtractor {
    fn default() -> Self {
        fn mel_to_hz(mel: f64) -> f64 {
            if mel < 15.0 {
                mel * (200.0 / 3.0)
            } else {
                1000.0 * ((mel - 15.0) * (6.4f64.ln() / 27.0)).exp()
            }
        }
        let max_mel = 15.0 + 12.0f64.ln() / (6.4f64.ln() / 27.0);
        let centers: Vec<_> = (0..130)
            .map(|i| mel_to_hz(i as f64 * max_mel / 129.0))
            .collect();
        let mut basis = vec![0.0; 128 * 513];
        for mel in 0..128 {
            let (lower, center, upper) = (centers[mel], centers[mel + 1], centers[mel + 2]);
            for bin in 0..513 {
                let hz = bin as f64 * 24_000.0 / 1024.0;
                basis[mel * 513 + bin] = (((hz - lower) / (center - lower))
                    .min((upper - hz) / (upper - center))
                    .max(0.0)
                    * 2.0
                    / (upper - lower)) as f32;
            }
        }
        Self {
            fft: FftPlanner::new().plan_fft_forward(1024),
            window: (0..1024)
                .map(|i| (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / 1024.0).cos()) as f32)
                .collect(),
            basis,
        }
    }
}

impl MelExtractor {
    pub fn extract(&self, pcm: &[f32]) -> Result<Vec<f32>> {
        ensure!(
            (24_000..=720_000).contains(&pcm.len()) && pcm.iter().all(|v| v.is_finite()),
            "invalid 24 kHz reference PCM"
        );
        let frames = (pcm.len() - 256) / 256 + 1;
        let mut output = Vec::with_capacity(frames * 128);
        let mut fft = vec![Complex::default(); 1024];
        let mut magnitude = vec![0.0f32; 513];
        for frame in 0..frames {
            super::codec::check_cancelled()?;
            for (i, value) in fft.iter_mut().enumerate() {
                let pos = (frame * 256 + i) as isize - 384;
                let reflected = if pos < 0 {
                    -pos
                } else if pos >= pcm.len() as isize {
                    2 * pcm.len() as isize - 2 - pos
                } else {
                    pos
                } as usize;
                *value = Complex::new(pcm[reflected] * self.window[i], 0.0);
            }
            self.fft.process(&mut fft);
            for (value, bin) in magnitude.iter_mut().zip(&fft) {
                *value = (bin.norm_sqr() + 1e-9).sqrt();
            }
            for filter in self.basis.as_chunks::<513>().0 {
                output.push(
                    filter
                        .iter()
                        .zip(&magnitude)
                        .map(|(a, b)| a * b)
                        .sum::<f32>()
                        .max(1e-5)
                        .ln(),
                );
            }
        }
        Ok(output)
    }
}

use super::{
    models::{Embeddings, add},
    tokenizer::TextTokenizer,
};

pub struct Reference {
    pub speaker: Vec<f32>,
    pub codes: Vec<[i64; 16]>,
    pub transcript: String,
}
pub struct Prompt {
    pub embeddings: Vec<f32>,
    pub trailing: Vec<Vec<f32>>,
}

/// Reference text and target text must be tokenized together for ICL parity.
pub fn prompt(
    tokenizer: &TextTokenizer,
    assets: &mut Embeddings,
    reference: &Reference,
    text: &str,
    language: &str,
) -> Result<Prompt> {
    ensure!(
        !text.trim().is_empty() && !reference.transcript.trim().is_empty(),
        "ICL requires target text and reference transcript"
    );
    ensure!(
        reference.speaker.len() == 2048 && reference.speaker.iter().all(|v| v.is_finite()),
        "invalid speaker conditioning"
    );
    ensure!(
        !reference.codes.is_empty(),
        "ICL requires reference codec frames"
    );
    let language = match language {
        "en" => 2050,
        "de" => 2053,
        "es" => 2054,
        "zh" => 2055,
        "ja" => 2058,
        "fr" => 2061,
        "ko" => 2064,
        "ru" => 2069,
        "it" => 2070,
        "pt" => 2071,
        _ => anyhow::bail!("unsupported Qwen language: {language}"),
    };
    let mut rows = Vec::new();
    for id in tokenizer.encode("<|im_start|>assistant\n")? {
        rows.push(assets.text(id)?);
    }
    for id in [2154, 2156, language, 2157] {
        let mut row = assets.pad.clone();
        add(&mut row, &assets.codec(0, id)?)?;
        rows.push(row);
    }
    let mut speaker = assets.pad.clone();
    add(&mut speaker, &reference.speaker)?;
    rows.push(speaker);
    let mut bos = assets.text(151672)?;
    add(&mut bos, &assets.codec(0, 2148)?)?;
    rows.push(bos);
    let mut ids = tokenizer.encode(&format!("{}{text}", reference.transcript))?;
    ids.push(151673);
    let text_vectors = ids
        .into_iter()
        .map(|id| assets.text(id))
        .collect::<Result<Vec<_>>>()?;
    let mut audio = vec![assets.codec(0, 2149)?];
    for codes in &reference.codes {
        audio.push(assets.frame(codes)?);
    }
    let audio_len = audio.len();
    for (index, mut audio) in audio.into_iter().enumerate() {
        add(&mut audio, text_vectors.get(index).unwrap_or(&assets.pad))?;
        rows.push(audio);
    }
    ensure!(rows.len() < 4096, "reference prompt exceeds talker context");
    let trailing = text_vectors.into_iter().skip(audio_len).collect();
    Ok(Prompt {
        embeddings: rows.into_iter().flatten().collect(),
        trailing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mel_matches_reference_scipy_fixture() {
        // Pinned reference mel.py, scipy RFFT, periodic Hann and Slaney filters.
        // Integer-defined input avoids cross-library sin rounding differences.
        let pcm: Vec<f32> = (0..24_000)
            .map(|i| ((i * 17 % 4096) as f32 - 2048.0) / 16384.0)
            .collect();
        let mel = MelExtractor::default().extract(&pcm).unwrap();
        assert_eq!(mel.len(), 93 * 128);
        for (index, expected) in [
            (0, -0.91555995),
            (1, -0.63485473),
            (31, -3.508173),
            (127, -5.3260217),
            (128, -2.9260232),
            (200, -4.0202036),
            (4096, -4.7433476),
            (8192, -4.7433476),
            (11903, -5.182108),
        ] {
            assert!(
                (mel[index] - expected).abs() < 0.0001,
                "mel[{index}]={} expected {expected}",
                mel[index]
            );
        }
    }
    #[test]
    fn mel_rejects_invalid_reference() {
        let mel = MelExtractor::default();
        assert!(mel.extract(&[]).is_err());
        assert!(mel.extract(&vec![f32::NAN; 24_000]).is_err());
        assert!(mel.extract(&vec![0.0; 720_001]).is_err());
    }
}
