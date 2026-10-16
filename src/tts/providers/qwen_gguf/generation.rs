//! Acoustic generation: talker -> 15 predictor codebooks -> talker feedback.
use super::{
    codec::check_cancelled,
    conditioning::Prompt,
    models::{Embeddings, add},
    runtime::Runtime,
};
use anyhow::{Result, anyhow, ensure};
use rand::{Rng, SeedableRng, rngs::StdRng};
use std::collections::VecDeque;

#[derive(Clone, Debug)]
pub struct Sampling {
    pub temperature: f32,
    pub top_k: usize,
    pub top_p: f32,
    pub repetition_penalty: f32,
    pub seed: u64,
}
impl Default for Sampling {
    fn default() -> Self {
        Self {
            temperature: 0.9,
            top_k: 50,
            top_p: 1.0,
            repetition_penalty: 1.05,
            seed: rand::random(),
        }
    }
}
struct Sampler {
    options: Sampling,
    rng: StdRng,
    history: VecDeque<usize>,
}
impl Sampler {
    fn new(options: Sampling) -> Result<Self> {
        ensure!(
            options.temperature.is_finite() && (0.0..=5.0).contains(&options.temperature),
            "invalid sampling temperature"
        );
        ensure!(
            options.top_p.is_finite() && options.top_p > 0.0 && options.top_p <= 1.0,
            "invalid top_p"
        );
        ensure!(
            options.repetition_penalty.is_finite()
                && (1.0..=4.0).contains(&options.repetition_penalty),
            "invalid repetition penalty"
        );
        Ok(Self {
            rng: StdRng::seed_from_u64(options.seed),
            options,
            history: VecDeque::new(),
        })
    }
    fn sample(&mut self, logits: &[f32], start: usize, end: usize, eos: bool) -> Result<usize> {
        ensure!(
            start < end && end <= logits.len() && (!eos || logits.len() > 2150),
            "sampling range out of bounds"
        );
        let mut candidates: Vec<_> = (start..end)
            .chain(if eos { Some(2150) } else { None })
            .map(|id| {
                let mut logit = logits[id];
                if self.history.contains(&id) {
                    logit = if logit < 0.0 {
                        logit * self.options.repetition_penalty
                    } else {
                        logit / self.options.repetition_penalty
                    };
                }
                (id, logit)
            })
            .collect();
        ensure!(
            candidates.iter().all(|(_, v)| v.is_finite()),
            "non-finite sampling logits"
        );
        candidates.sort_unstable_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let selected = if self.options.temperature == 0.0 {
            candidates[0].0
        } else {
            if self.options.top_k > 0 {
                candidates.truncate(self.options.top_k);
            }
            let max = candidates[0].1;
            let mut weights: Vec<f64> = candidates
                .iter()
                .map(|(_, v)| (((*v - max) / self.options.temperature) as f64).exp())
                .collect();
            let total = weights.iter().sum::<f64>();
            let mut cumulative = 0.0;
            let keep = weights
                .iter()
                .position(|w| {
                    cumulative += w / total;
                    cumulative >= self.options.top_p as f64
                })
                .map(|n| n + 1)
                .unwrap_or(weights.len());
            weights.truncate(keep);
            let mut draw = self.rng.random::<f64>() * weights.iter().sum::<f64>();
            let mut picked = keep - 1;
            for (index, weight) in weights.iter().enumerate() {
                draw -= weight;
                if draw <= 0.0 {
                    picked = index;
                    break;
                }
            }
            candidates[picked].0
        };
        if selected < 2048 {
            self.history.push_back(selected);
            if self.history.len() > 128 {
                self.history.pop_front();
            }
        }
        Ok(selected)
    }
}

pub struct Generated {
    pub codes: Vec<[i64; 16]>,
    pub reached_eos: bool,
}

pub fn generate(
    runtime: &mut Runtime,
    assets: &mut Embeddings,
    prompt: Prompt,
    max_tokens: usize,
    talker_options: Sampling,
    predictor_options: Sampling,
) -> Result<Generated> {
    ensure!(
        (1..=1024).contains(&max_tokens),
        "max_tokens must be 1–1024"
    );
    ensure!(
        prompt.embeddings.len().is_multiple_of(2048),
        "invalid prompt embedding dimensions"
    );
    let prompt_tokens = prompt.embeddings.len() / 2048;
    ensure!(
        prompt_tokens > 0 && prompt_tokens + max_tokens <= 4096,
        "prompt/generation exceeds 4096-token context"
    );
    check_cancelled()?;
    runtime.reset()?;
    let mut talker_sampler = Sampler::new(talker_options)?;
    let mut predictor_sampler = Sampler::new(predictor_options)?;
    let mut last = (Vec::new(), Vec::new());
    let mut position = 0;
    for chunk in prompt.embeddings.chunks(512 * 2048) {
        check_cancelled()?;
        last = runtime
            .talker
            .decode(chunk, position)
            .map_err(|e| anyhow!("talker prefill: {e}"))?;
        position += chunk.len() / 2048;
    }
    let mut codes = Vec::with_capacity(max_tokens);
    let mut reached_eos = false;
    for step in 0..max_tokens {
        check_cancelled()?;
        if let Some(control) = crate::tts::control::current() {
            control.update("talker", step);
        }
        let code0 = talker_sampler.sample(&last.0, 0, 2048, true)?;
        if code0 == 2150 {
            reached_eos = true;
            break;
        }
        let mut frame = [0i64; 16];
        frame[0] = code0 as i64;
        let mut prefill = assets.project(&last.1)?;
        let row = assets.codec(0, code0)?;
        prefill.extend(assets.project(&row)?);
        runtime
            .predictor
            .clear()
            .map_err(|e| anyhow!("predictor clear: {e}"))?;
        let mut predicted = runtime
            .predictor
            .decode(&prefill, 0)
            .map_err(|e| anyhow!("predictor prefill: {e}"))?
            .0;
        for (q, code) in frame.iter_mut().enumerate().skip(1) {
            check_cancelled()?;
            let token = predictor_sampler.sample(&predicted, (q - 1) * 2048, q * 2048, false)?;
            *code = (token - (q - 1) * 2048) as i64;
            if q < 15 {
                let row = assets.codec(q, *code as usize)?;
                let embedding = assets.project(&row)?;
                predicted = runtime
                    .predictor
                    .decode(&embedding, q + 1)
                    .map_err(|e| anyhow!("predictor decode: {e}"))?
                    .0;
            }
        }
        codes.push(frame);
        if step + 1 < max_tokens {
            let mut embedding = assets.frame(&frame)?;
            add(
                &mut embedding,
                prompt.trailing.get(step).unwrap_or(&assets.pad),
            )?;
            last = runtime
                .talker
                .decode(&embedding, position)
                .map_err(|e| anyhow!("talker decode: {e}"))?;
            position += 1;
        }
    }
    check_cancelled()?;
    ensure!(
        !codes.is_empty(),
        "talker ended without producing acoustic frames"
    );
    Ok(Generated { codes, reached_eos })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sampling_rejects_invalid_ranges_and_options() {
        assert!(
            Sampler::new(Sampling {
                temperature: f32::NAN,
                ..Default::default()
            })
            .is_err()
        );
        let mut s = Sampler::new(Sampling::default()).unwrap();
        assert!(s.sample(&[1.0], 0, 2, false).is_err());
        assert!(s.sample(&[f32::INFINITY], 0, 1, false).is_err());
    }
    #[test]
    fn seeded_sampling_stays_in_codebook_range() {
        let options = Sampling {
            seed: 42,
            repetition_penalty: 1.0,
            ..Default::default()
        };
        let mut a = Sampler::new(options.clone()).unwrap();
        let mut b = Sampler::new(options).unwrap();
        let logits = vec![0.0; 30720];
        for _ in 0..100 {
            let id = a.sample(&logits, 2048, 4096, false).unwrap();
            assert!((2048..4096).contains(&id));
            assert_eq!(id, b.sample(&logits, 2048, 4096, false).unwrap());
        }
    }
}
