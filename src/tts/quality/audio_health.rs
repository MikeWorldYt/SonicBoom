//! Deterministic, measurement-only audio-health analysis.
//!
//! Nothing here alters, trims, filters, normalizes, gates, or compresses the
//! audio: it reports numerical evidence so generation defects can be localized
//! to inference, decoding, conditioning, or playback. Thresholds are
//! deliberately conservative heuristics, never verdicts. A clip is not
//! "acceptable" merely because every sample is finite.
//!
//! Cost is a single O(n) pass plus an O(n) first-difference scan, so this is
//! cheap enough for the latency-sensitive request path.

use serde::Serialize;

/// First-difference magnitude considered impulsive for normalized speech PCM.
const IMPULSE_THRESHOLD: f32 = 1.5;
/// |sample| at or above which a sample is considered clipped.
const CLIP_LEVEL: f32 = 0.999;
/// |sample| below which a sample counts as near-silent.
const SILENCE_LEVEL: f32 = 1e-4;
/// Samples produced by one decoder chunk (12 frames × 1920 samples) at 24 kHz.
pub const DECODER_CHUNK_SAMPLES_24K: usize = 12 * 1920;
/// First-difference at a chunk boundary above which a seam is suspected.
const BOUNDARY_DELTA_THRESHOLD: f32 = 0.5;

/// Numerical measurements over mono `f32` PCM (never modifies the input).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AudioHealth {
    pub sample_count: usize,
    pub sample_rate: u32,
    pub duration_secs: f64,
    pub non_finite_samples: usize,
    pub peak: f32,
    pub rms: f64,
    pub dc_offset: f64,
    pub clipped_samples: usize,
    pub clipping_fraction: f64,
    pub near_silent_fraction: f64,
    /// Largest absolute first difference between adjacent finite samples.
    pub max_discontinuity: f32,
    /// Number of adjacent-sample jumps above [`IMPULSE_THRESHOLD`].
    pub impulsive_samples: usize,
    /// Suspected decoder chunk seams: boundary deltas above the threshold.
    pub chunk_boundary_peaks: usize,
}

/// Measure `samples` (mono, `sample_rate` Hz). Non-finite samples are counted
/// and excluded from the amplitude statistics rather than poisoning them.
pub fn analyze(samples: &[f32], sample_rate: u32) -> AudioHealth {
    let denom = samples.len().max(1) as f64;
    let mut non_finite = 0usize;
    let mut peak = 0f32;
    let mut square_sum = 0f64;
    let mut dc_sum = 0f64;
    let mut clipped = 0usize;
    let mut near_silent = 0usize;
    for &sample in samples {
        if !sample.is_finite() {
            non_finite += 1;
            continue;
        }
        peak = peak.max(sample.abs());
        square_sum += (sample as f64) * (sample as f64);
        dc_sum += sample as f64;
        if sample.abs() >= CLIP_LEVEL {
            clipped += 1;
        }
        if sample.abs() < SILENCE_LEVEL {
            near_silent += 1;
        }
    }
    let (max_discontinuity, impulsive_samples, chunk_boundary_peaks) =
        discontinuity(samples, sample_rate);
    AudioHealth {
        sample_count: samples.len(),
        sample_rate,
        duration_secs: if sample_rate == 0 {
            0.0
        } else {
            samples.len() as f64 / f64::from(sample_rate)
        },
        non_finite_samples: non_finite,
        peak,
        rms: (square_sum / denom).sqrt(),
        dc_offset: dc_sum / denom,
        clipped_samples: clipped,
        clipping_fraction: clipped as f64 / denom,
        near_silent_fraction: near_silent as f64 / denom,
        max_discontinuity,
        impulsive_samples,
        chunk_boundary_peaks,
    }
}

/// First-difference scan returning `(max delta, impulsive count, boundary seams)`.
/// Chunk boundaries are scaled from the 24 kHz decoder geometry so the check
/// stays meaningful if a caller analyzes another rate.
fn discontinuity(samples: &[f32], sample_rate: u32) -> (f32, usize, usize) {
    let chunk = if sample_rate == 0 {
        0
    } else {
        (u64::from(sample_rate) as usize).saturating_mul(DECODER_CHUNK_SAMPLES_24K) / 24_000
    };
    let mut max_delta = 0f32;
    let mut impulsive = 0usize;
    let mut boundary = 0usize;
    for index in 1..samples.len() {
        let (a, b) = (samples[index - 1], samples[index]);
        if !a.is_finite() || !b.is_finite() {
            continue;
        }
        let delta = (b - a).abs();
        if delta > max_delta {
            max_delta = delta;
        }
        if delta > IMPULSE_THRESHOLD {
            impulsive += 1;
        }
        if chunk > 0 && index % chunk == 0 && delta > BOUNDARY_DELTA_THRESHOLD {
            boundary += 1;
        }
    }
    (max_delta, impulsive, boundary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(len: usize, hz: f64, amp: f32) -> Vec<f32> {
        (0..len)
            .map(|i| (amp as f64 * (std::f64::consts::TAU * hz * i as f64 / 24_000.0).sin()) as f32)
            .collect()
    }

    #[test]
    fn silence_and_tone_measure_as_expected() {
        let silence = analyze(&vec![0.0; 24_000], 24_000);
        assert_eq!(silence.sample_count, 24_000);
        assert!((silence.duration_secs - 1.0).abs() < 1e-9);
        assert_eq!(silence.peak, 0.0);
        assert_eq!(silence.rms, 0.0);
        assert_eq!(silence.near_silent_fraction, 1.0);
        assert_eq!(silence.non_finite_samples, 0);

        let tone = analyze(&tone(24_000, 440.0, 0.5), 24_000);
        assert!((tone.peak - 0.5).abs() < 0.01);
        assert!((tone.rms - 0.5 / 2f64.sqrt()).abs() < 0.01);
        assert!(tone.impulsive_samples == 0, "smooth tone has no impulses");
        assert!(tone.max_discontinuity < 0.2);
    }

    #[test]
    fn non_finite_samples_are_counted_and_excluded() {
        let mut samples = tone(1000, 440.0, 0.5);
        samples[10] = f32::NAN;
        samples[20] = f32::INFINITY;
        let health = analyze(&samples, 24_000);
        assert_eq!(health.non_finite_samples, 2);
        assert!(health.peak <= 0.5 + 1e-6);
        assert!(health.rms.is_finite());
    }

    #[test]
    fn clipping_and_dc_offset_are_measured() {
        let clipped: Vec<f32> = (0..1000)
            .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let health = analyze(&clipped, 24_000);
        assert_eq!(health.clipped_samples, 1000);
        assert!((health.clipping_fraction - 1.0).abs() < 1e-9);
        assert!(
            health.impulsive_samples > 0,
            "full-scale flips are impulses"
        );

        let dc = analyze(&vec![0.5; 1000], 24_000);
        assert!((dc.dc_offset - 0.5).abs() < 1e-9);
        assert!(dc.max_discontinuity < 1e-6);
    }

    #[test]
    fn chunk_boundary_seams_are_detected_by_index() {
        // A clean tone with one injected full-scale step exactly on a chunk
        // boundary, and none elsewhere.
        let mut samples = tone(3 * DECODER_CHUNK_SAMPLES_24K, 220.0, 0.3);
        let seam = 2 * DECODER_CHUNK_SAMPLES_24K;
        samples[seam] = 0.95;
        let health = analyze(&samples, 24_000);
        assert_eq!(health.chunk_boundary_peaks, 1);
        // The same step at a non-boundary index is not a boundary seam.
        let mut samples = tone(3 * DECODER_CHUNK_SAMPLES_24K, 220.0, 0.3);
        samples[seam + 5] = 0.95;
        assert_eq!(analyze(&samples, 24_000).chunk_boundary_peaks, 0);
    }

    #[test]
    fn empty_input_is_finite_and_zeroed() {
        let health = analyze(&[], 24_000);
        assert_eq!(health.sample_count, 0);
        assert_eq!(health.duration_secs, 0.0);
        assert!(health.rms.is_finite());
        assert_eq!(analyze(&[], 0).duration_secs, 0.0);
    }
}
