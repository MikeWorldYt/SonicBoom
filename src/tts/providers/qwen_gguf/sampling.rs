//! Typed, validated Qwen sampling resolution.
//!
//! The shared [`SamplingRequest`] contract lives in `providers::types` so it
//! compiles without the `qwen-gguf` feature; this module holds the Qwen
//! preset values and the resolution into concrete [`Sampling`] options.
//!
//! The `balanced` preset reproduces the pinned upstream defaults exactly, so
//! with identical artifacts, seed, and device the output is unchanged. Other
//! presets are experimental until benchmarked; explicit parameters imply
//! `custom`. Validation rejects NaN/Inf, out-of-range values, and invalid
//! seeds before any inference runs.

use super::generation::Sampling;
use crate::tts::providers::types::{SamplingParams, SamplingPreset, SamplingRequest};

/// Pinned upstream defaults (compatibility baseline).
pub const BALANCED_TALKER: SamplingParams = SamplingParams::new(0.9, 50, 1.0, 1.05);
pub const BALANCED_PREDICTOR: SamplingParams = SamplingParams::new(0.9, 50, 1.0, 1.0);
/// Experimental: lower variance than `balanced`, stronger repetition control.
pub const STABLE_TALKER: SamplingParams = SamplingParams::new(0.7, 40, 0.9, 1.10);
pub const STABLE_PREDICTOR: SamplingParams = SamplingParams::new(0.7, 40, 0.9, 1.0);
/// Experimental: greater acoustic variation.
pub const EXPRESSIVE_TALKER: SamplingParams = SamplingParams::new(1.0, 60, 0.98, 1.03);
pub const EXPRESSIVE_PREDICTOR: SamplingParams = SamplingParams::new(0.95, 50, 0.98, 1.0);

/// Preset parameters for both samplers. `Custom` falls back to the balanced
/// values; explicit fields (if any) are applied on top by the resolver.
pub fn preset_params(preset: SamplingPreset) -> (SamplingParams, SamplingParams) {
    match preset {
        SamplingPreset::Balanced | SamplingPreset::Custom => (BALANCED_TALKER, BALANCED_PREDICTOR),
        SamplingPreset::Stable => (STABLE_TALKER, STABLE_PREDICTOR),
        SamplingPreset::Expressive => (EXPRESSIVE_TALKER, EXPRESSIVE_PREDICTOR),
    }
}

/// Fully resolved sampling for one synthesis, with the effective preset name
/// for diagnostics.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedSampling {
    pub talker: Sampling,
    pub predictor: Sampling,
    /// Effective seed after applying the request/preset override.
    pub seed: u64,
    /// Effective preset name (`balanced`/`stable`/`expressive`/`custom`).
    pub preset: &'static str,
}

impl SamplingRequest {
    /// Resolve, validate, and produce concrete sampler options.
    ///
    /// `request_seed` is the legacy per-request seed; an explicit
    /// `sampling.seed` takes precedence when present.
    pub fn resolve_qwen(&self, request_seed: Option<u64>) -> Result<ResolvedSampling, String> {
        let preset = self.preset.unwrap_or_default();
        let (default_talker, default_predictor) = preset_params(preset);
        let talker_params = self.talker.unwrap_or(default_talker);
        let predictor_params = self.predictor.unwrap_or(default_predictor);
        talker_params.validate("talker")?;
        predictor_params.validate("predictor")?;
        // Explicit parameters make the effective mode `custom` regardless of
        // the named preset, because the emitted sampler options no longer
        // correspond to that preset.
        let effective_preset = if self.talker.is_some() || self.predictor.is_some() {
            SamplingPreset::Custom.as_str()
        } else {
            preset.as_str()
        };
        let seed = self.seed.or(request_seed).unwrap_or_else(rand::random);
        Ok(ResolvedSampling {
            talker: Sampling {
                temperature: talker_params.temperature,
                top_k: talker_params.top_k,
                top_p: talker_params.top_p,
                repetition_penalty: talker_params.repetition_penalty,
                seed,
            },
            predictor: Sampling {
                temperature: predictor_params.temperature,
                top_k: predictor_params.top_k,
                top_p: predictor_params.top_p,
                repetition_penalty: predictor_params.repetition_penalty,
                seed: seed.wrapping_add(1),
            },
            seed,
            preset: effective_preset,
        })
    }
}

/// Resolve sampling for a request: honors an explicit [`SamplingRequest`] when
/// present, otherwise the balanced baseline with the legacy request seed.
pub fn resolve(
    sampling: Option<&SamplingRequest>,
    request_seed: Option<u64>,
) -> Result<ResolvedSampling, String> {
    match sampling {
        Some(sampling) => sampling.resolve_qwen(request_seed),
        None => SamplingRequest::default().resolve_qwen(request_seed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balanced_preset_matches_upstream_defaults_exactly() {
        let resolved = resolve(None, Some(7)).unwrap();
        let default = Sampling::default();
        assert_eq!(resolved.preset, "balanced");
        assert_eq!(resolved.talker.temperature, default.temperature);
        assert_eq!(resolved.talker.top_k, default.top_k);
        assert_eq!(resolved.talker.top_p, default.top_p);
        assert_eq!(
            resolved.talker.repetition_penalty,
            default.repetition_penalty
        );
        assert_eq!(resolved.seed, 7);
        assert_eq!(resolved.predictor.seed, 8);
        assert_eq!(resolved.predictor.repetition_penalty, 1.0);
    }

    #[test]
    fn explicit_seed_overrides_request_seed() {
        let config = SamplingRequest {
            seed: Some(99),
            ..Default::default()
        };
        assert_eq!(config.resolve_qwen(Some(7)).unwrap().seed, 99);
    }

    #[test]
    fn presets_parse_and_apply() {
        let stable = SamplingRequest {
            preset: Some(SamplingPreset::Stable),
            ..Default::default()
        }
        .resolve_qwen(Some(1))
        .unwrap();
        assert_eq!(stable.preset, "stable");
        assert_eq!(stable.talker.temperature, 0.7);
        assert!(stable.talker.repetition_penalty > BALANCED_TALKER.repetition_penalty);
        let expressive = SamplingRequest {
            preset: Some(SamplingPreset::Expressive),
            ..Default::default()
        }
        .resolve_qwen(Some(1))
        .unwrap();
        assert_eq!(expressive.preset, "expressive");
        assert_eq!(expressive.talker.top_k, 60);
    }

    #[test]
    fn explicit_parameters_force_custom_and_take_precedence() {
        let config = SamplingRequest {
            preset: Some(SamplingPreset::Stable),
            talker: Some(SamplingParams::new(0.5, 20, 0.8, 1.2)),
            ..Default::default()
        };
        let resolved = config.resolve_qwen(None).unwrap();
        assert_eq!(resolved.preset, "custom");
        assert_eq!(resolved.talker.temperature, 0.5);
        // The predictor still follows the named preset's values.
        assert_eq!(resolved.predictor.temperature, STABLE_PREDICTOR.temperature);
    }

    #[test]
    fn invalid_parameters_are_rejected() {
        let cases = [
            SamplingParams::new(f32::NAN, 50, 1.0, 1.05),
            SamplingParams::new(6.0, 50, 1.0, 1.05),
            SamplingParams::new(0.9, 4096, 1.0, 1.05),
            SamplingParams::new(0.9, 50, 0.0, 1.05),
            SamplingParams::new(0.9, 50, 1.5, 1.05),
            SamplingParams::new(0.9, 50, 1.0, 0.5),
            SamplingParams::new(0.9, 50, 1.0, f32::INFINITY),
        ];
        for params in cases {
            let config = SamplingRequest {
                talker: Some(params),
                ..Default::default()
            };
            assert!(
                config.resolve_qwen(None).is_err(),
                "{params:?} must be rejected"
            );
        }
    }

    #[test]
    fn seed_parses_from_decimal_string_and_number() {
        let from_string: SamplingRequest =
            serde_json::from_str(r#"{"seed":"18446744073709551615"}"#).unwrap();
        assert_eq!(from_string.seed, Some(u64::MAX));
        let from_number: SamplingRequest = serde_json::from_str(r#"{"seed":42}"#).unwrap();
        assert_eq!(from_number.seed, Some(42));
        assert!(
            serde_json::from_str::<SamplingRequest>(r#"{"seed":"abc"}"#).is_err(),
            "non-numeric seed strings are rejected"
        );
    }

    #[test]
    fn unknown_fields_and_presets_are_rejected() {
        assert!(serde_json::from_str::<SamplingRequest>(r#"{"bogus":1}"#).is_err());
        assert!(serde_json::from_str::<SamplingRequest>(r#"{"preset":"turbo"}"#).is_err());
    }
}
