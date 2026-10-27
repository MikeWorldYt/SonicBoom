//! Splicing of spoken replacements into the original text, and budgets.
//!
//! Text outside the replaced spans is copied byte-for-byte. Budgets are
//! checked after rendering and fail with an explicit error; nothing is ever
//! truncated or wrapped.

use super::{NormalizeError, NormalizeLimits, TextChange};

/// Minimum input size used as the base of the expansion-ratio check, so
/// short valid inputs (`77%` -> "setenta y siete por ciento") are not
/// rejected. Long expanding inputs are still bounded by the ratio.
pub(crate) const EXPANSION_FLOOR_CHARS: usize = 32;

/// Character count of `text`, or `None` as soon as it exceeds `max`.
pub(crate) fn count_chars_bounded(text: &str, max: usize) -> Option<usize> {
    if text.len() <= max {
        return Some(text.chars().count());
    }
    let mut count = 0usize;
    for _ in text.chars() {
        count += 1;
        if count > max {
            return None;
        }
    }
    Some(count)
}

/// Replace each change span (ordered, non-overlapping, char boundaries).
pub(crate) fn splice(text: &str, changes: &[TextChange]) -> String {
    let extra: usize = changes.iter().map(|c| c.replacement.len()).sum();
    let mut out = String::with_capacity(text.len() + extra);
    let mut cursor = 0usize;
    for change in changes {
        // Defensive: skip anything out of order or off a boundary rather
        // than panic. The classifier never produces such spans.
        if change.start < cursor
            || change.end < change.start
            || change.end > text.len()
            || !text.is_char_boundary(change.start)
            || !text.is_char_boundary(change.end)
        {
            continue;
        }
        out.push_str(&text[cursor..change.start]);
        out.push_str(&change.replacement);
        cursor = change.end;
    }
    out.push_str(&text[cursor..]);
    out
}

/// Enforce `max_output_chars` and the expansion ratio:
/// spoken chars `<= max(input chars, EXPANSION_FLOOR_CHARS) * ratio`.
pub(crate) fn check_budget(
    spoken: &str,
    input_chars: usize,
    limits: &NormalizeLimits,
) -> Result<(), NormalizeError> {
    let output_chars = spoken.chars().count();
    if output_chars > limits.max_output_chars {
        return Err(NormalizeError::OutputTooLong {
            max_chars: limits.max_output_chars,
        });
    }
    let ratio = if limits.max_expansion_ratio.is_finite() && limits.max_expansion_ratio >= 1.0 {
        f64::from(limits.max_expansion_ratio)
    } else {
        1.0
    };
    let allowed = (input_chars.max(EXPANSION_FLOOR_CHARS) as f64) * ratio;
    if output_chars as f64 > allowed {
        return Err(NormalizeError::ExpansionTooLarge);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tts::normalization::{ChangeKind, Confidence};

    fn change(start: usize, end: usize, replacement: &str) -> TextChange {
        TextChange {
            kind: ChangeKind::Number,
            start,
            end,
            replacement: replacement.to_string(),
            confidence: Confidence::High,
        }
    }

    #[test]
    fn splice_preserves_unchanged_bytes() {
        let text = "a 1 ñ 2 😊";
        let out = splice(text, &[change(2, 3, "uno"), change(7, 8, "dos")]);
        assert_eq!(out, "a uno ñ dos 😊");
    }

    #[test]
    fn splice_skips_invalid_spans() {
        let text = "ñ1";
        // 1 is not a char boundary (inside 'ñ').
        assert_eq!(splice(text, &[change(1, 3, "x")]), "ñ1");
    }

    #[test]
    fn budgets() {
        let limits = NormalizeLimits {
            max_input_chars: 1000,
            max_output_chars: 100,
            max_expansion_ratio: 2.0,
        };
        // Short inputs use the floor: 2 input chars -> 32 * 2 = 64 allowed.
        assert!(check_budget(&"x".repeat(64), 2, &limits).is_ok());
        assert_eq!(
            check_budget(&"x".repeat(65), 2, &limits),
            Err(NormalizeError::ExpansionTooLarge)
        );
        // Above the floor the real input size is the base.
        assert!(check_budget(&"x".repeat(80), 40, &limits).is_ok());
        assert_eq!(
            check_budget(&"x".repeat(81), 40, &limits),
            Err(NormalizeError::ExpansionTooLarge)
        );
        assert_eq!(
            check_budget(&"x".repeat(101), 1000, &limits),
            Err(NormalizeError::OutputTooLong { max_chars: 100 })
        );
        let nan = NormalizeLimits {
            max_expansion_ratio: f32::NAN,
            ..limits
        };
        assert!(check_budget(&"x".repeat(32), 1, &nan).is_ok());
        assert_eq!(
            check_budget(&"x".repeat(33), 1, &nan),
            Err(NormalizeError::ExpansionTooLarge)
        );
        assert_eq!(count_chars_bounded("ñññ", 2), None);
        assert_eq!(count_chars_bounded("ñññ", 3), Some(3));
    }
}
