//! Reference-transcript echo detection.
//!
//! Qwen3-TTS ICL conditioning can continue the *reference* transcript before
//! the requested text (upstream issue #341). Confirming that requires knowing
//! what was actually spoken, so detection is layered:
//!
//! 1. Normalize the reference transcript and target text to word sequences.
//! 2. Identify reference suffixes that are absent from the target text's
//!    opening (the only positions where legitimate overlap can occur).
//! 3. When an ASR transcript is supplied (offline/optional — never required
//!    in the request path), check whether the generated speech *begins* with
//!    one of those suffixes.
//!
//! No ASR transcript means no claim is made (`detected = false`,
//! `asr_required = true`). The detector never guesses from PCM alone and never
//! searches mid-utterance, so it cannot report echo in the middle of valid
//! speech. Evidence carries word counts, never raw transcript content.

use serde::Serialize;
use unicode_normalization::UnicodeNormalization;

/// Minimum matched words before a suffix counts as a confirmed echo: a single
/// shared word is far too weak and would false-positive constantly.
const MIN_ECHO_WORDS: usize = 2;
/// Longest reference suffix considered as a candidate echo phrase.
const MAX_CANDIDATE_WORDS: usize = 12;

/// Structured echo analysis. `confidence` stays `None` until calibrated —
/// callers must gate on `detected`, never on an invented probability.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EchoAnalysis {
    /// True only when ASR evidence confirms a reference continuation absent
    /// from the target text.
    pub detected: bool,
    /// True when no ASR transcript was available, so nothing may be claimed.
    pub asr_required: bool,
    /// Reference words matched at the start of the ASR transcript.
    pub matched_words: usize,
    /// Longest reference suffix (in words) absent from the target opening.
    pub candidate_words: usize,
    /// Whether the confirmed match begins at the start of the utterance.
    pub at_start: bool,
    /// Uncalibrated confidence placeholder (always `None` for now).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    /// Machine-readable evidence tags; never raw transcript content.
    pub evidence: Vec<String>,
}

/// NFKC-normalize, lowercase, and split into alphanumeric word tokens
/// (accents and Unicode letters are preserved).
pub fn normalize_words(text: &str) -> Vec<String> {
    text.nfkc()
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

fn is_prefix(phrase: &[String], target: &[String]) -> bool {
    phrase.len() <= target.len() && phrase == &target[..phrase.len()]
}

/// Longest reference suffix (≤ [`MAX_CANDIDATE_WORDS`]) that is not a prefix
/// of the target text. Length 0 means every reference tail word is a plausible
/// target opening, so no echo phrase can be isolated.
pub fn candidate_words(reference: &[String], target: &[String]) -> usize {
    for k in (1..=reference.len().min(MAX_CANDIDATE_WORDS)).rev() {
        if !is_prefix(&reference[reference.len() - k..], target) {
            return k;
        }
    }
    0
}

/// Longest reference suffix (≤ [`MAX_CANDIDATE_WORDS`]) that the ASR transcript
/// begins with. The match need not include the reference's final word alone:
/// any length up to the candidate cap is checked, longest first.
fn matched_prefix(reference: &[String], asr: &[String]) -> usize {
    let limit = reference.len().min(MAX_CANDIDATE_WORDS).min(asr.len());
    for k in (1..=limit).rev() {
        if asr[..k] == reference[reference.len() - k..] {
            return k;
        }
    }
    0
}

/// Analyze an utterance for reference-transcript echo.
pub fn analyze(
    reference_transcript: &str,
    target_text: &str,
    asr_transcript: Option<&str>,
) -> EchoAnalysis {
    let reference = normalize_words(reference_transcript);
    let target = normalize_words(target_text);
    let candidate = candidate_words(&reference, &target);
    let mut evidence = Vec::new();
    let (detected, matched_words, at_start) = match asr_transcript {
        None => {
            evidence.push("asr_unavailable:no_claim".to_string());
            if candidate > 0 {
                evidence.push(format!(
                    "reference_suffix_absent_from_target:{candidate}words"
                ));
            }
            (false, 0, false)
        }
        Some(asr) => {
            let asr = normalize_words(asr);
            let matched = matched_prefix(&reference, &asr);
            if matched == 0 {
                (false, 0, false)
            } else {
                let phrase = &reference[reference.len() - matched..];
                if is_prefix(phrase, &target) {
                    // The target legitimately opens with these words.
                    evidence.push(format!("target_also_opens_with:{matched}words"));
                    (false, matched, true)
                } else if matched >= MIN_ECHO_WORDS {
                    evidence.push(format!("asr_start_matches_reference_suffix:{matched}words"));
                    (true, matched, true)
                } else {
                    evidence.push("single_word_overlap:below_threshold".to_string());
                    (false, matched, true)
                }
            }
        }
    };
    EchoAnalysis {
        detected,
        asr_required: asr_transcript.is_none(),
        matched_words,
        candidate_words: candidate,
        at_start,
        confidence: None,
        evidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_handles_punctuation_case_and_accents() {
        assert_eq!(
            normalize_words("¡Hola, Mundo! ¿Qué tal?"),
            vec!["hola", "mundo", "qué", "tal"]
        );
        assert_eq!(normalize_words("  A—B  "), vec!["a", "b"]);
        assert!(normalize_words("").is_empty());
    }

    #[test]
    fn confirms_echo_when_asr_begins_with_absent_reference_suffix() {
        // Reference tail "quick brown fox" is absent from the target opening:
        // the ASR transcript that starts with it is a confirmed echo.
        let analysis = analyze(
            "the quick brown fox",
            "hello there everyone",
            Some("quick brown fox hello there everyone"),
        );
        assert!(analysis.detected);
        assert!(!analysis.asr_required);
        assert_eq!(analysis.matched_words, 3);
        assert!(analysis.at_start);
        assert_eq!(analysis.confidence, None);
        assert!(
            analysis
                .evidence
                .iter()
                .any(|e| e.starts_with("asr_start_matches_reference_suffix"))
        );
    }

    #[test]
    fn no_false_positive_when_target_legitimately_opens_with_shared_words() {
        // Target genuinely starts with the reference's tail words: not echo,
        // though the shorter suffix ("quick brown fox") is still a candidate.
        let analysis = analyze(
            "the quick brown fox",
            "the quick brown fox jumps",
            Some("the quick brown fox jumps"),
        );
        assert!(!analysis.detected);
        assert_eq!(analysis.candidate_words, 3);
        assert!(
            analysis
                .evidence
                .iter()
                .any(|e| e.starts_with("target_also_opens_with"))
        );
    }

    #[test]
    fn single_word_overlap_is_not_echo() {
        let analysis = analyze("alpha bravo", "hello world", Some("bravo hello world"));
        assert!(
            !analysis.detected,
            "one shared word must not be reported as echo"
        );
    }

    #[test]
    fn without_asr_no_claim_is_made_but_risk_is_quantified() {
        let analysis = analyze("the quick brown fox", "hello there", None);
        assert!(!analysis.detected);
        assert!(analysis.asr_required);
        assert_eq!(analysis.matched_words, 0);
        assert_eq!(analysis.candidate_words, 4);
        assert!(
            analysis
                .evidence
                .iter()
                .any(|e| e == "asr_unavailable:no_claim")
        );
    }

    #[test]
    fn clean_generation_without_overlap_is_not_echo() {
        let analysis = analyze(
            "buenos días amigo",
            "esto es una prueba",
            Some("esto es una prueba"),
        );
        assert!(!analysis.detected);
        assert_eq!(analysis.matched_words, 0);
    }

    #[test]
    fn empty_reference_or_target_never_panics() {
        assert!(!analyze("", "hello", Some("hello")).detected);
        assert!(!analyze("hello", "", Some("hello world")).detected);
        assert!(!analyze("", "", None).detected);
    }
}
