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
/// Token ids the sampler may penalize: the talker EOS marker (2150) plus the
/// largest codebook id range the sampler addresses. History only ever stores
/// ids below 2048, so membership for larger ids is always false.
const HISTORY_DOMAIN: usize = 2151;
/// Bounded repetition history length, matching the pre-optimization behavior.
const HISTORY_LIMIT: usize = 128;

struct Sampler {
    options: Sampling,
    rng: StdRng,
    history: VecDeque<usize>,
    /// Per-id occurrence counts shadowing `history`, turning the per-candidate
    /// linear `contains` scan into O(1) membership. Counts (not flags) keep
    /// duplicate ids exact under eviction: an id stays penalized while any
    /// occurrence remains in the window.
    recent: [u8; HISTORY_DOMAIN],
    /// Reused scratch buffers. Contents are rebuilt from scratch on every
    /// call in the same order as before, so sorting, truncation, weights,
    /// and RNG draws are bit-identical to fresh allocations.
    candidates: Vec<(usize, f32)>,
    weights: Vec<f64>,
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
            recent: [0; HISTORY_DOMAIN],
            candidates: Vec::new(),
            weights: Vec::new(),
        })
    }
    fn sample(&mut self, logits: &[f32], start: usize, end: usize, eos: bool) -> Result<usize> {
        ensure!(
            start < end && end <= logits.len() && (!eos || logits.len() > 2150),
            "sampling range out of bounds"
        );
        self.candidates.clear();
        self.candidates.extend(
            (start..end)
                .chain(if eos { Some(2150) } else { None })
                .map(|id| {
                    let mut logit = logits[id];
                    if id < HISTORY_DOMAIN && self.recent[id] > 0 {
                        logit = if logit < 0.0 {
                            logit * self.options.repetition_penalty
                        } else {
                            logit / self.options.repetition_penalty
                        };
                    }
                    (id, logit)
                }),
        );
        let candidates = &mut self.candidates;
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
            self.weights.clear();
            self.weights.extend(
                candidates
                    .iter()
                    .map(|(_, v)| (((*v - max) / self.options.temperature) as f64).exp()),
            );
            let weights = &mut self.weights;
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
            // Counts stay exact under duplicates: eviction decrements only
            // the evicted occurrence.
            self.recent[selected] = self.recent[selected].saturating_add(1);
            if self.history.len() > HISTORY_LIMIT
                && let Some(evicted) = self.history.pop_front()
            {
                self.recent[evicted] = self.recent[evicted].saturating_sub(1);
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
    /// Exact copy of the pre-optimization sampler: fresh allocations and a
    /// linear history scan on every candidate. Used only to prove the
    /// optimized sampler selects identical tokens.
    struct ReferenceSampler {
        options: Sampling,
        rng: StdRng,
        history: VecDeque<usize>,
    }

    impl ReferenceSampler {
        fn new(options: Sampling) -> Self {
            Self {
                rng: StdRng::seed_from_u64(options.seed),
                options,
                history: VecDeque::new(),
            }
        }

        fn sample(&mut self, logits: &[f32], start: usize, end: usize, eos: bool) -> Result<usize> {
            assert!(start < end && end <= logits.len() && (!eos || logits.len() > 2150));
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
            assert!(candidates.iter().all(|(_, v)| v.is_finite()));
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

    fn sampler_options(seed: u64, mutate: impl Fn(&mut Sampling)) -> Sampling {
        let mut options = Sampling {
            seed,
            ..Default::default()
        };
        mutate(&mut options);
        options
    }

    /// Drives both samplers over identical logit streams and asserts
    /// identical selections, including history evolution.
    fn assert_sampler_parity(options: Sampling, cases: &[Vec<f32>], eos: bool) {
        let mut optimized = Sampler::new(options.clone()).unwrap();
        let mut reference = ReferenceSampler::new(options);
        for logits in cases {
            let start = 0;
            let end = logits.len().min(2048);
            assert_eq!(
                optimized.sample(logits, start, end, eos).unwrap(),
                reference.sample(logits, start, end, eos).unwrap()
            );
        }
        // Histories must agree exactly (order and duplicates included).
        assert_eq!(
            optimized.history.make_contiguous(),
            reference.history.make_contiguous()
        );
    }

    fn pseudo_logits(seed: u64, len: usize, ties: bool) -> Vec<f32> {
        let mut x = seed.max(1);
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                if ties {
                    // Heavy ties: only 8 distinct values across the range.
                    (x % 8) as f32 - 4.0
                } else {
                    (x % 20001) as f32 / 1000.0 - 10.0
                }
            })
            .collect()
    }

    #[test]
    fn optimized_sampler_matches_reference_on_recorded_logits() {
        // Tied logits stress exact tie-breaking; narrow top_k/top_p stress
        // truncation; greedy stresses the temperature==0 path.
        let tied: Vec<Vec<f32>> = (0..150)
            .map(|i| pseudo_logits(0x9E37 + i, 2049, true))
            .collect();
        let varied: Vec<Vec<f32>> = (0..150)
            .map(|i| pseudo_logits(0x51F3 + i * 7919, 2049, false))
            .collect();

        for seed in [1u64, 7, 12345] {
            assert_sampler_parity(
                sampler_options(seed, |o| {
                    o.temperature = 0.9;
                    o.top_k = 50;
                    o.top_p = 1.0;
                }),
                &tied,
                false,
            );
            assert_sampler_parity(
                sampler_options(seed, |o| {
                    o.temperature = 0.7;
                    o.top_k = 5;
                    o.top_p = 0.9;
                }),
                &varied,
                false,
            );
            assert_sampler_parity(
                sampler_options(seed, |o| {
                    o.temperature = 0.0;
                    o.top_k = 50;
                }),
                &tied,
                false,
            );
            // top_k = 0 disables truncation; EOS appends candidate 2150
            // (EOS sampling requires logits longer than the 2150 marker).
            let eos_cases: Vec<Vec<f32>> = (0..150)
                .map(|i| pseudo_logits(0xE05 + i * 31, 2208, i % 2 == 0))
                .collect();
            assert_sampler_parity(
                sampler_options(seed, |o| {
                    o.top_k = 0;
                    o.top_p = 0.5;
                }),
                &eos_cases,
                true,
            );
            // Strong repetition penalty with duplicates churning through the
            // bounded 128-entry window.
            assert_sampler_parity(
                sampler_options(seed, |o| {
                    o.repetition_penalty = 4.0;
                    o.top_k = 0;
                }),
                &tied,
                false,
            );
        }
    }

    #[test]
    fn optimized_sampler_matches_reference_on_extremes() {
        let mut extreme = vec![0.0f32; 2049];
        extreme[0] = 1e30;
        extreme[1] = -1e30;
        extreme[2] = f32::MAX;
        extreme[3] = f32::MIN;
        extreme[4] = 1e-30;
        extreme[5] = -1e-30;
        // Ties at the extremes stress total_cmp ordering, not raw partial_cmp.
        for (i, slot) in extreme.iter_mut().enumerate().skip(6).take(10) {
            *slot = if i % 2 == 0 { 1e30 } else { -1e30 };
        }
        let cases = vec![extreme; 200];
        assert_sampler_parity(Sampling::default(), &cases, false);

        // Non-finite logits are rejected identically (never sampled).
        let mut bad = vec![0.0f32; 64];
        bad[3] = f32::NAN;
        let mut optimized = Sampler::new(Sampling::default()).unwrap();
        assert!(optimized.sample(&bad, 0, 64, false).is_err());
        bad[3] = f32::INFINITY;
        assert!(optimized.sample(&bad, 0, 64, false).is_err());
    }

    #[test]
    fn duplicate_history_ids_survive_partial_eviction() {
        // Force the same id repeatedly, then verify the penalty still
        // applies while any occurrence remains in the window.
        let options = Sampling {
            seed: 99,
            temperature: 0.0,
            repetition_penalty: 4.0,
            ..Default::default()
        };
        let mut optimized = Sampler::new(options.clone()).unwrap();
        let mut reference = ReferenceSampler::new(options);
        // Logits where id 5 always wins unless penalized below id 6.
        let mut logits = vec![-100.0f32; 64];
        logits[5] = 10.0;
        logits[6] = 9.0;
        for _ in 0..(HISTORY_LIMIT + 10) {
            let a = optimized.sample(&logits, 0, 64, false).unwrap();
            let b = reference.sample(&logits, 0, 64, false).unwrap();
            assert_eq!(a, b);
        }
        assert_eq!(optimized.history.len(), HISTORY_LIMIT);
    }

    #[test]
    fn scratch_buffers_stay_bounded_across_calls() {
        let mut sampler = Sampler::new(Sampling::default()).unwrap();
        let logits = pseudo_logits(42, 2208, false);
        for _ in 0..50 {
            sampler.sample(&logits, 0, 2048, true).unwrap();
        }
        // Steady-state capacity: one 2049-entry candidate buffer and one
        // top_k-sized weight buffer — no per-call growth.
        assert!(sampler.candidates.capacity() < 4096);
        assert!(sampler.weights.capacity() <= 2050);
        assert_eq!(sampler.history.len(), 50);
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
