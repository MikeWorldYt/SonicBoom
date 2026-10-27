//! Shared, offline, rule-based TTS text normalization ("TN").
//!
//! Turns written forms (numbers, dates, prices, times, units, ...) into
//! speakable text before chunking and before any provider-specific
//! preprocessing. The pipeline is span based, never a chain of global
//! regex replacements:
//!
//! ```text
//! original text (immutable)
//!   -> input length + language + locale validation   (options.rs)
//!   -> Unicode-aware field scan, protected regions    (lexer.rs)
//!   -> classification with ambiguity policy           (classify.rs)
//!   -> locale-aware spoken forms                      (rules/*.rs)
//!   -> splice + budget checks                         (render.rs)
//! ```
//!
//! Policy: ambiguous input is preserved byte-for-byte and reported as a
//! warning, never guessed. Text outside replaced spans is preserved
//! byte-for-byte. `off` mode returns the input unchanged. Languages without
//! rules pass through unchanged and report `none` coverage.
//!
//! See `docs/normalization.md` for coverage and limitations.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

mod classify;
#[allow(dead_code)]
mod dictionary;
mod lexer;
mod options;
mod render;
mod rules;
pub(crate) mod segments;

#[cfg(test)]
mod tests;

pub use options::{
    NormalizationMode, NormalizeLimits, NormalizeOptions, default_locale_for, is_supported_locale,
    locales_for,
};

/// Version of the rule set. Participates in [`SpeechText::identity`].
pub const RULESET_VERSION: &str = "tn-v1";

/// Stable feature keys used in coverage maps.
pub const FEATURE_KEYS: [&str; 11] = [
    "cardinal_numbers",
    "decimals",
    "ordinals",
    "percentages",
    "currency",
    "dates",
    "times",
    "units",
    "abbreviations",
    "ranges",
    "fractions",
];

/// How completely a language implements a feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CoverageLevel {
    /// All common written forms of the feature are converted.
    Full,
    /// Some forms are converted; the rest are preserved.
    Partial,
    /// Nothing is converted (pass-through).
    None,
}

/// Category of a replaced span.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Number,
    Decimal,
    Ordinal,
    Percentage,
    Currency,
    Date,
    Time,
    Unit,
    Abbreviation,
    Range,
}

/// Confidence attached to a change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    High,
    Medium,
}

/// One replaced span of the original text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextChange {
    pub kind: ChangeKind,
    /// Byte offset into the ORIGINAL text (char boundary).
    pub start: usize,
    /// Byte offset into the ORIGINAL text, exclusive (char boundary).
    pub end: usize,
    /// Spoken form that replaces `original[start..end]`.
    pub replacement: String,
    pub confidence: Confidence,
}

/// Machine-readable warning codes (serialized as snake_case).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningCode {
    AmbiguousNumber,
    AmbiguousDate,
    InvalidDate,
    AmbiguousCurrency,
    AmbiguousTime,
    AmbiguousFraction,
    NumberTooLarge,
}

impl WarningCode {
    /// Stable snake_case name, identical to the serde form.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AmbiguousNumber => "ambiguous_number",
            Self::AmbiguousDate => "ambiguous_date",
            Self::InvalidDate => "invalid_date",
            Self::AmbiguousCurrency => "ambiguous_currency",
            Self::AmbiguousTime => "ambiguous_time",
            Self::AmbiguousFraction => "ambiguous_fraction",
            Self::NumberTooLarge => "number_too_large",
        }
    }
}

/// A preserved span that the caller may want to review.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizationWarning {
    pub code: WarningCode,
    /// Byte offsets into the ORIGINAL text.
    pub start: usize,
    pub end: usize,
}

/// Result of [`normalize`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NormalizedText {
    /// Text to synthesize.
    pub spoken: String,
    /// Replaced spans, ordered by `start`, non-overlapping.
    pub changes: Vec<TextChange>,
    /// Empty when `report_ambiguities` is false. Ordered by `start`.
    pub warnings: Vec<NormalizationWarning>,
    /// Per-feature coverage for this language/locale.
    pub coverage: BTreeMap<String, CoverageLevel>,
    pub mode: NormalizationMode,
    pub language: String,
    pub locale: String,
    pub ruleset_version: &'static str,
}

/// Errors from option resolution and normalization. Messages never contain
/// user text.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum NormalizeError {
    #[error("input exceeds {max_chars} characters")]
    InputTooLong { max_chars: usize },
    #[error("spoken text would exceed {max_chars} characters")]
    OutputTooLong { max_chars: usize },
    #[error("spoken text expansion exceeds the configured ratio")]
    ExpansionTooLarge,
    #[error("speech language is not a valid language code")]
    InvalidLanguage,
    #[error("locale is not a valid BCP-47 tag")]
    InvalidLocale,
    #[error("locale is not supported for this language")]
    UnsupportedLocale,
    #[error("locale does not belong to the speech language")]
    LocaleLanguageMismatch,
}

/// Normalize `text` for speech.
///
/// Pure, offline and deterministic. Never panics; runs in time linear in the
/// input length (inputs above `max_input_chars` are rejected before any
/// scanning beyond the length check).
pub fn normalize(text: &str, opts: &NormalizeOptions) -> Result<NormalizedText, NormalizeError> {
    let limits = opts.limits;
    let input_chars = render::count_chars_bounded(text, limits.max_input_chars).ok_or(
        NormalizeError::InputTooLong {
            max_chars: limits.max_input_chars,
        },
    )?;
    let language_rules = rules::for_language(&opts.language);
    let coverage = rules::coverage_map(language_rules);

    let (changes, warnings) = match opts.mode {
        NormalizationMode::Off => (Vec::new(), Vec::new()),
        NormalizationMode::Auto => classify::classify(text, language_rules, opts),
    };
    let spoken = if changes.is_empty() {
        text.to_string()
    } else {
        render::splice(text, &changes)
    };
    if opts.mode == NormalizationMode::Auto {
        render::check_budget(&spoken, input_chars, &limits)?;
    }
    Ok(NormalizedText {
        spoken,
        changes,
        warnings: if opts.report_ambiguities {
            warnings
        } else {
            Vec::new()
        },
        coverage,
        mode: opts.mode,
        language: opts.language.clone(),
        locale: opts.locale.clone(),
        ruleset_version: RULESET_VERSION,
    })
}

/// Typed marker for text that has been through [`normalize`] (or `off`).
///
/// Synthesis entry points take this type rather than raw user strings so
/// that text is prepared exactly once.
#[derive(Clone, Debug)]
pub struct SpeechText {
    spoken: String,
    identity: String,
    changed: bool,
}

impl SpeechText {
    /// Normalize `text` and keep only the spoken form and identity.
    pub fn prepare(text: &str, opts: &NormalizeOptions) -> Result<Self, NormalizeError> {
        let normalized = normalize(text, opts)?;
        let changed = normalized.spoken != text;
        Ok(Self {
            identity: format!(
                "{}|{}|{}|{}",
                RULESET_VERSION,
                opts.mode.as_str(),
                opts.language,
                opts.locale
            ),
            spoken: normalized.spoken,
            changed,
        })
    }

    /// Spoken text.
    pub fn as_str(&self) -> &str {
        &self.spoken
    }

    /// Consume into the spoken text.
    pub fn into_string(self) -> String {
        self.spoken
    }

    /// Stable identity, e.g. `"tn-v1|auto|es|es-PE"`. Contains no user text.
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// `true` when the spoken text differs from the original.
    pub fn changed(&self) -> bool {
        self.changed
    }
}

/// Normalization coverage of one supported language.
#[derive(Clone, Debug, Serialize)]
pub struct LanguageCoverage {
    pub language: &'static str,
    pub locales: &'static [&'static str],
    pub default_locale: &'static str,
    /// Overall level: `full` only when every feature is full, `none` when
    /// every feature is none, `partial` otherwise.
    pub rules: CoverageLevel,
    pub features: BTreeMap<&'static str, CoverageLevel>,
}

/// One entry per supported language, for `/api/info` and the UI.
pub fn language_coverage() -> Vec<LanguageCoverage> {
    options::LOCALE_TABLE
        .iter()
        .map(|(language, locales, default_locale)| {
            let language_rules = rules::for_language(language);
            let features: BTreeMap<&'static str, CoverageLevel> = rules::Feature::ALL
                .iter()
                .map(|feature| (feature.key(), language_rules.coverage(*feature)))
                .collect();
            LanguageCoverage {
                language,
                locales,
                default_locale,
                rules: rules::overall(features.values().copied()),
                features,
            }
        })
        .collect()
}
