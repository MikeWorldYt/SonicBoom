//! Normalization mode, limits, language/locale validation and resolution.
//!
//! The locale table here is the single source of truth for supported
//! pronunciation locales (see `docs/normalization.md`). Locales are never
//! inferred from the server location or the UI language.

use serde::{Deserialize, Serialize};

use super::NormalizeError;

/// Normalization mode requested by the caller or configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NormalizationMode {
    /// Spoken text is byte-identical to the input.
    Off,
    /// Conservative, high-confidence transforms only.
    Auto,
}

impl NormalizationMode {
    /// Parse `"off"` or `"auto"` (trimmed, case-insensitive).
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.eq_ignore_ascii_case("off") {
            Some(Self::Off)
        } else if raw.eq_ignore_ascii_case("auto") {
            Some(Self::Auto)
        } else {
            None
        }
    }

    /// Stable lowercase name: `"off"` or `"auto"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Auto => "auto",
        }
    }
}

/// Resource budgets for one normalization call.
#[derive(Clone, Copy, Debug)]
pub struct NormalizeLimits {
    /// Maximum characters of the original text.
    pub max_input_chars: usize,
    /// Maximum characters of the spoken text.
    pub max_output_chars: usize,
    /// Spoken chars must be `<= max(input chars, 32) * ratio`. The floor of
    /// 32 characters keeps short valid inputs (`77%`) from being rejected.
    /// Non-finite values or values below 1.0 are treated as 1.0.
    pub max_expansion_ratio: f32,
}

impl Default for NormalizeLimits {
    /// Matches the server defaults: 10 000 input characters (the default
    /// `tts.max_text_length`), 20 000 spoken characters, ratio 8.0.
    fn default() -> Self {
        Self {
            max_input_chars: 10_000,
            max_output_chars: 20_000,
            max_expansion_ratio: 8.0,
        }
    }
}

/// Fully resolved options for [`super::normalize`].
#[derive(Clone, Debug)]
pub struct NormalizeOptions {
    /// Requested mode.
    pub mode: NormalizationMode,
    /// Base speech language code, lowercase (e.g. `"es"`).
    pub language: String,
    /// Concrete locale tag (e.g. `"es-PE"`), never `"auto"`. For a
    /// well-formed language without normalization rules this is the
    /// language code itself.
    pub locale: String,
    /// `true` when the caller supplied a concrete locale tag.
    pub locale_explicit: bool,
    /// Budgets.
    pub limits: NormalizeLimits,
    /// When `false`, [`super::NormalizedText::warnings`] is always empty.
    pub report_ambiguities: bool,
}

impl NormalizeOptions {
    /// Validate and resolve options.
    ///
    /// `requested_locale`: `None`, `""` or `"auto"` (case-insensitive)
    /// selects the language default. A concrete tag must be well-formed,
    /// belong to `speech_language` and be in the supported table.
    pub fn resolve(
        mode: NormalizationMode,
        speech_language: &str,
        requested_locale: Option<&str>,
        limits: NormalizeLimits,
        report_ambiguities: bool,
    ) -> Result<Self, NormalizeError> {
        let language =
            canonical_language(speech_language).ok_or(NormalizeError::InvalidLanguage)?;
        let requested = requested_locale
            .map(str::trim)
            .filter(|raw| !raw.is_empty() && !raw.eq_ignore_ascii_case("auto"));
        let (locale, locale_explicit) = match requested {
            None => {
                let locale = default_locale_for(&language)
                    .map(str::to_string)
                    .unwrap_or_else(|| language.clone());
                (locale, false)
            }
            Some(raw) => {
                let tag = canonical_locale(raw).ok_or(NormalizeError::InvalidLocale)?;
                let primary = tag.split('-').next().unwrap_or_default();
                if primary != language {
                    return Err(NormalizeError::LocaleLanguageMismatch);
                }
                if !locales_for(&language).contains(&tag.as_str()) {
                    return Err(NormalizeError::UnsupportedLocale);
                }
                (tag, true)
            }
        };
        Ok(Self {
            mode,
            language,
            locale,
            locale_explicit,
            limits,
            report_ambiguities,
        })
    }
}

/// Supported languages: (language, locales, default locale).
pub(crate) const LOCALE_TABLE: &[(&str, &[&str], &str)] = &[
    ("es", &["es-ES", "es-MX", "es-PE"], "es-ES"),
    ("en", &["en-US", "en-GB"], "en-US"),
    ("pt", &["pt-BR", "pt-PT"], "pt-BR"),
    ("fr", &["fr-FR"], "fr-FR"),
    ("de", &["de-DE"], "de-DE"),
    ("it", &["it-IT"], "it-IT"),
    ("ru", &["ru-RU"], "ru-RU"),
    ("zh", &["zh-CN", "zh-TW"], "zh-CN"),
    ("ja", &["ja-JP"], "ja-JP"),
    ("ko", &["ko-KR"], "ko-KR"),
];

/// `true` when `tag` is a supported locale (case-insensitive, canonicalized).
pub fn is_supported_locale(tag: &str) -> bool {
    let Some(tag) = canonical_locale(tag) else {
        return false;
    };
    LOCALE_TABLE
        .iter()
        .any(|(_, locales, _)| locales.contains(&tag.as_str()))
}

/// Documented default locale for a speech language, if it has rules.
pub fn default_locale_for(language: &str) -> Option<&'static str> {
    let language = canonical_language(language)?;
    LOCALE_TABLE
        .iter()
        .find(|(lang, _, _)| *lang == language)
        .map(|(_, _, default)| *default)
}

/// Supported locales for a speech language (empty when none).
pub fn locales_for(language: &str) -> &'static [&'static str] {
    let Some(language) = canonical_language(language) else {
        return &[];
    };
    LOCALE_TABLE
        .iter()
        .find(|(lang, _, _)| *lang == language)
        .map(|(_, locales, _)| *locales)
        .unwrap_or(&[])
}

/// Canonical lowercase language code: 2 or 3 ASCII letters after trimming.
pub(crate) fn canonical_language(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if (2..=3).contains(&raw.len()) && raw.bytes().all(|b| b.is_ascii_alphabetic()) {
        Some(raw.to_ascii_lowercase())
    } else {
        None
    }
}

/// Canonicalize a well-formed BCP-47-style tag: hyphen-separated subtags of
/// 1..=8 ASCII alphanumerics, a 2..=3 letter primary language subtag.
/// Casing follows BCP-47 conventions (language lowercase, 4-letter script
/// titlecase, 2-letter region uppercase). Underscores are rejected.
pub(crate) fn canonical_locale(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > 64 {
        return None;
    }
    let mut out = String::with_capacity(raw.len());
    for (index, subtag) in raw.split('-').enumerate() {
        if subtag.is_empty()
            || subtag.len() > 8
            || !subtag.bytes().all(|b| b.is_ascii_alphanumeric())
        {
            return None;
        }
        if index == 0 {
            if !(2..=3).contains(&subtag.len()) || !subtag.bytes().all(|b| b.is_ascii_alphabetic())
            {
                return None;
            }
            out.push_str(&subtag.to_ascii_lowercase());
            continue;
        }
        out.push('-');
        if subtag.len() == 2 && subtag.bytes().all(|b| b.is_ascii_alphabetic()) {
            out.push_str(&subtag.to_ascii_uppercase());
        } else if subtag.len() == 4 && subtag.bytes().all(|b| b.is_ascii_alphabetic()) {
            let lower = subtag.to_ascii_lowercase();
            let mut chars = lower.chars();
            if let Some(first) = chars.next() {
                out.push(first.to_ascii_uppercase());
                out.push_str(chars.as_str());
            }
        } else {
            out.push_str(&subtag.to_ascii_lowercase());
        }
    }
    Some(out)
}
