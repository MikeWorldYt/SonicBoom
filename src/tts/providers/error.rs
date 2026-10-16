//! Typed provider errors with explicit, actionable messages.
//!
//! Every variant maps deterministically to the existing HTTP error policy:
//! `400` invalid input, `413` oversize, `429` busy/rate-limited, `503`
//! unavailable, sanitized `500` internal. No silent fallbacks: unknown
//! provider/model/voice/language, incompatible variants, missing assets and
//! unavailable devices are all explicit errors.

use std::fmt;

/// Machine-readable error kind (serialized as `code` in diagnostics).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderErrorKind {
    UnsupportedModel,
    UnsupportedLanguage,
    InvalidVoice,
    ModelMissing,
    Loading,
    Busy,
    DeviceUnavailable,
    InferenceFailed,
}

impl fmt::Display for ProviderErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = match self {
            Self::UnsupportedModel => "unsupported_model",
            Self::UnsupportedLanguage => "unsupported_language",
            Self::InvalidVoice => "invalid_voice",
            Self::ModelMissing => "model_missing",
            Self::Loading => "loading",
            Self::Busy => "busy",
            Self::DeviceUnavailable => "device_unavailable",
            Self::InferenceFailed => "inference_failed",
        };
        f.write_str(code)
    }
}

/// Typed provider failure.
#[derive(Debug, Clone)]
pub struct ProviderError {
    kind: ProviderErrorKind,
    message: String,
}

impl ProviderError {
    pub fn new(kind: ProviderErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> ProviderErrorKind {
        self.kind
    }

    /// Machine-readable code (`unsupported_model`, ...).
    pub fn code(&self) -> &'static str {
        match self.kind {
            ProviderErrorKind::UnsupportedModel => "unsupported_model",
            ProviderErrorKind::UnsupportedLanguage => "unsupported_language",
            ProviderErrorKind::InvalidVoice => "invalid_voice",
            ProviderErrorKind::ModelMissing => "model_missing",
            ProviderErrorKind::Loading => "loading",
            ProviderErrorKind::Busy => "busy",
            ProviderErrorKind::DeviceUnavailable => "device_unavailable",
            ProviderErrorKind::InferenceFailed => "inference_failed",
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn unsupported_model(provider: &str, model: &str, detail: impl Into<String>) -> Self {
        Self::new(
            ProviderErrorKind::UnsupportedModel,
            format!(
                "provider '{provider}' does not support model '{model}': {}",
                detail.into()
            ),
        )
    }

    pub fn unsupported_language(provider: &str, language: &str, supported: &[&str]) -> Self {
        Self::new(
            ProviderErrorKind::UnsupportedLanguage,
            format!(
                "provider '{provider}' does not support language '{language}' (supported: {})",
                supported.join(", ")
            ),
        )
    }

    pub fn invalid_voice(detail: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::InvalidVoice, detail.into())
    }

    pub fn model_missing(detail: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::ModelMissing, detail.into())
    }

    pub fn loading(detail: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Loading, detail.into())
    }

    pub fn busy(detail: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Busy, detail.into())
    }

    pub fn device_unavailable(detail: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::DeviceUnavailable, detail.into())
    }

    pub fn inference_failed(detail: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::InferenceFailed, detail.into())
    }

    /// HTTP status for the existing error policy.
    pub fn http_status(&self) -> u16 {
        match self.kind {
            ProviderErrorKind::UnsupportedModel
            | ProviderErrorKind::UnsupportedLanguage
            | ProviderErrorKind::InvalidVoice => 400,
            ProviderErrorKind::ModelMissing
            | ProviderErrorKind::Loading
            | ProviderErrorKind::DeviceUnavailable => 503,
            ProviderErrorKind::Busy => 429,
            ProviderErrorKind::InferenceFailed => 500,
        }
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code(), self.message)
    }
}

impl std::error::Error for ProviderError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_map_to_documented_http_status() {
        let cases = [
            (
                ProviderError::unsupported_model("qwen3", "nope", "unknown"),
                400u16,
                "unsupported_model",
            ),
            (
                ProviderError::unsupported_language("pocket-tts", "fr", &["en", "es"]),
                400,
                "unsupported_language",
            ),
            (
                ProviderError::invalid_voice("bad voice"),
                400,
                "invalid_voice",
            ),
            (
                ProviderError::model_missing("no bundle"),
                503,
                "model_missing",
            ),
            (ProviderError::loading("still loading"), 503, "loading"),
            (
                ProviderError::device_unavailable("no cuda"),
                503,
                "device_unavailable",
            ),
            (ProviderError::busy("saturated"), 429, "busy"),
            (
                ProviderError::inference_failed("onnx error"),
                500,
                "inference_failed",
            ),
        ];
        for (error, status, code) in cases {
            assert_eq!(error.http_status(), status, "{code}");
            assert_eq!(error.code(), code);
        }
    }

    #[test]
    fn french_rejection_names_supported_languages() {
        let error = ProviderError::unsupported_language("pocket-tts", "fr", &["en", "es"]);
        assert!(error.message().contains("fr"));
        assert!(error.message().contains("en"));
    }
}
