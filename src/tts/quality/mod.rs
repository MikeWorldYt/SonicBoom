//! Deterministic quality detection for generated speech.
//!
//! Two independent detectors feed one structured report:
//!
//! * [`audio_health`] — numerical measurements (finite samples, peak, RMS, DC
//!   offset, clipping, near-silence, discontinuities, chunk seams). Pure
//!   measurement: never trims, filters, gates, or normalizes.
//! * [`reference_echo`] — reference-transcript continuation detection, layered
//!   and ASR-optional.
//!
//! [`policy`] decides what, if anything, to do with a finding, and [`report`]
//! carries the structured result. Everything here is fast enough for the
//! request path; the optional ASR layer is a benchmark/offline dependency and
//! is never required during generation.

pub mod audio_health;
pub mod policy;
pub mod reference_echo;
pub mod report;

pub use audio_health::{AudioHealth, analyze as analyze_audio};
pub use policy::{DetectionMode, MAX_RETRIES_CEILING, QualityPolicy};
pub use reference_echo::EchoAnalysis;
pub use report::{QualityIssue, QualityReport, QualityStatus, Severity};

/// Health-derived findings. `expected_seconds` (if supplied) is a loose
/// speech-rate estimate used only for an informational duration check — never
/// a hard judgement.
pub fn health_issues(health: &AudioHealth, expected_seconds: Option<f64>) -> Vec<QualityIssue> {
    let mut issues = Vec::new();
    if health.non_finite_samples > 0 {
        issues.push(QualityIssue::error(
            "non_finite_samples",
            format!(
                "{} of {} samples are NaN/Inf",
                health.non_finite_samples, health.sample_count
            ),
        ));
    }
    if health.sample_count > 0 && health.near_silent_fraction >= 0.98 {
        issues.push(QualityIssue::warning(
            "near_silent_output",
            format!(
                "{:.1}% of samples are near-silent",
                health.near_silent_fraction * 100.0
            ),
        ));
    }
    if health.clipping_fraction >= 0.01 {
        issues.push(QualityIssue::warning(
            "severe_clipping",
            format!(
                "{} samples ({:.2}%) are clipped",
                health.clipped_samples,
                health.clipping_fraction * 100.0
            ),
        ));
    }
    if health.impulsive_samples > 0 {
        issues.push(QualityIssue::warning(
            "impulsive_artifacts",
            format!(
                "{} adjacent-sample jumps above threshold (max {:.3})",
                health.impulsive_samples, health.max_discontinuity
            ),
        ));
    }
    if health.chunk_boundary_peaks > 0 {
        issues.push(QualityIssue::warning(
            "possible_chunk_boundary_discontinuity",
            format!(
                "{} suspected decoder chunk seams",
                health.chunk_boundary_peaks
            ),
        ));
    }
    if health.dc_offset.abs() > 0.1 {
        issues.push(QualityIssue::warning(
            "large_dc_offset",
            format!("dc offset {:.3}", health.dc_offset),
        ));
    }
    if let Some(expected) = expected_seconds
        && expected > 0.0
        && health.duration_secs > expected * 3.0 + 2.0
    {
        issues.push(QualityIssue::info(
            "duration_outlier",
            format!(
                "audio is {:.2}s vs an estimated {:.2}s for the text (heuristic)",
                health.duration_secs, expected
            ),
        ));
    }
    issues
}

/// Rough speech-duration estimate from character count (heuristic; used only
/// for the informational `duration_outlier` check).
pub fn estimated_seconds(text: &str) -> f64 {
    let chars = text.chars().count() as f64;
    (chars / 13.0).max(0.5)
}

/// Run the full deterministic quality pass over generated audio.
pub fn evaluate(
    samples: &[f32],
    sample_rate: u32,
    reference_transcript: &str,
    target_text: &str,
    asr_transcript: Option<&str>,
    mode: DetectionMode,
) -> QualityReport {
    if !mode.detects() {
        return QualityReport::ok();
    }
    let health = audio_health::analyze(samples, sample_rate);
    let issues = health_issues(&health, Some(estimated_seconds(target_text)));
    let echo = reference_echo::analyze(reference_transcript, target_text, asr_transcript);
    QualityReport::from_parts(mode.as_str(), Some(health), Some(echo), issues, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_mode_short_circuits_to_ok() {
        let report = evaluate(
            &vec![f32::NAN; 100],
            24_000,
            "ref words",
            "target words",
            None,
            DetectionMode::Off,
        );
        assert_eq!(report.quality_status, QualityStatus::Ok);
        assert!(report.audio.is_none());
        assert!(report.echo.is_none());
    }

    #[test]
    fn non_finite_samples_are_reported_as_failure() {
        let mut samples = vec![0.2f32; 4800];
        samples[10] = f32::NAN;
        let report = evaluate(
            &samples,
            24_000,
            "reference",
            "target",
            None,
            DetectionMode::Report,
        );
        assert_eq!(report.quality_status, QualityStatus::Failed);
        assert!(report.issues.iter().any(|i| i.kind == "non_finite_samples"));
    }

    #[test]
    fn clean_tone_reports_ok_without_invented_confidence() {
        let samples: Vec<f32> = (0..24_000)
            .map(|i| 0.3 * (i as f32 * 440.0 * std::f32::consts::TAU / 24_000.0).sin())
            .collect();
        let report = evaluate(
            &samples,
            24_000,
            "the quick brown fox",
            "hello there",
            None,
            DetectionMode::Report,
        );
        assert_eq!(report.quality_status, QualityStatus::Ok);
        let echo = report.echo.unwrap();
        assert!(echo.asr_required);
        assert_eq!(echo.confidence, None);
        assert!(!echo.detected);
    }

    #[test]
    fn estimated_seconds_is_positive_and_monotonic() {
        assert!(estimated_seconds("") > 0.0);
        assert!(estimated_seconds("hello world this is longer") > estimated_seconds("hi"));
    }
}
