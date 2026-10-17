//! Quality policy: what to do with a quality finding.
//!
//! Detection is always non-destructive in [`DetectionMode::Report`]. Only
//! [`DetectionMode::ExperimentalTrim`] could ever remove audio, and it stays
//! inert (downgraded to `report`) unless accurate word timestamps exist —
//! which the current detector does not produce. Nothing here trims a fixed
//! number of milliseconds or removes words from the target.

/// Detection/handling mode for generated audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DetectionMode {
    /// No detection: the legacy request path, unchanged.
    Off,
    /// Detect and report suspicious results; never modify audio. Default.
    #[default]
    Report,
    /// Regenerate once after a sufficiently confident failure.
    Retry,
    /// Reserved timestamp-based removal. Without word timestamps it behaves
    /// exactly like [`DetectionMode::Report`] and records that fact.
    ExperimentalTrim,
}

impl DetectionMode {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "off" | "none" => Some(Self::Off),
            "report" => Some(Self::Report),
            "retry" => Some(Self::Retry),
            "experimental_trim" | "experimental-trim" | "trim" => Some(Self::ExperimentalTrim),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Report => "report",
            Self::Retry => "retry",
            Self::ExperimentalTrim => "experimental_trim",
        }
    }

    /// Whether any detection work runs at all.
    pub fn detects(self) -> bool {
        !matches!(self, Self::Off)
    }
}

/// Bounded remediation policy. `max_retries` is clamped to a hard ceiling so
/// a misconfiguration can never cause unbounded regeneration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualityPolicy {
    pub mode: DetectionMode,
    pub max_retries: usize,
}

/// Hard ceiling on automatic regeneration attempts per request.
pub const MAX_RETRIES_CEILING: usize = 1;

impl Default for QualityPolicy {
    fn default() -> Self {
        Self {
            mode: DetectionMode::Off,
            max_retries: 0,
        }
    }
}

impl QualityPolicy {
    pub fn new(mode: DetectionMode, max_retries: usize) -> Self {
        Self {
            mode,
            max_retries: max_retries.min(MAX_RETRIES_CEILING),
        }
    }

    /// Whether the policy permits one more attempt given `retries` already spent.
    pub fn allows_retry(&self, retries: usize, detection_failed: bool) -> bool {
        self.mode == DetectionMode::Retry && detection_failed && retries < self.max_retries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_parse_and_stringify_round_trip() {
        for mode in [
            DetectionMode::Off,
            DetectionMode::Report,
            DetectionMode::Retry,
            DetectionMode::ExperimentalTrim,
        ] {
            assert_eq!(DetectionMode::parse(mode.as_str()), Some(mode));
        }
        assert_eq!(DetectionMode::parse("REPORT"), Some(DetectionMode::Report));
        assert_eq!(DetectionMode::parse("none"), Some(DetectionMode::Off));
        assert_eq!(DetectionMode::parse("banana"), None);
        assert!(!DetectionMode::Off.detects());
        assert!(DetectionMode::Report.detects());
    }

    #[test]
    fn retries_are_bounded_by_a_hard_ceiling() {
        let policy = QualityPolicy::new(DetectionMode::Retry, 99);
        assert_eq!(policy.max_retries, MAX_RETRIES_CEILING);
        assert!(policy.allows_retry(0, true));
        assert!(!policy.allows_retry(1, true));
        assert!(
            !policy.allows_retry(0, false),
            "only a detection failure retries"
        );
        // Non-retry modes never regenerate.
        let report = QualityPolicy::new(DetectionMode::Report, 1);
        assert!(!report.allows_retry(0, true));
    }
}
