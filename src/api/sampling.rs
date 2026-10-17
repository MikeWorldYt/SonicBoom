//! Query-string sampling quality overrides shared by the job and preview
//! endpoints.
//!
//! Every field is optional and defaults to the existing behavior. Parsing is
//! strict: an unknown preset, an unparseable seed, or an out-of-range parameter
//! is an explicit `400`, never a silent default. Partial parameter groups are
//! filled from the pinned `balanced` values so a single tuned knob is
//! meaningful; the effective mode then reports as `custom`.

use crate::error::AppError;
use sonicboom::tts::providers::types::{SamplingParams, SamplingPreset, SamplingRequest};
use sonicboom::tts::quality::DetectionMode;

/// Balanced (pinned upstream) talker defaults, mirrored from
/// `qwen_gguf::sampling::BALANCED_TALKER`.
const BALANCED_TALKER: SamplingParams = SamplingParams::new(0.9, 50, 1.0, 1.05);
/// Balanced predictor defaults, mirrored from
/// `qwen_gguf::sampling::BALANCED_PREDICTOR`.
const BALANCED_PREDICTOR: SamplingParams = SamplingParams::new(0.9, 50, 1.0, 1.0);

/// Raw optional sampling query fields (unparsed).
#[derive(Debug, Default, Clone)]
pub struct SamplingInput {
    pub preset: Option<String>,
    pub seed: Option<String>,
    pub quality: Option<String>,
    pub talker_temperature: Option<f32>,
    pub talker_top_k: Option<usize>,
    pub talker_top_p: Option<f32>,
    pub talker_repetition_penalty: Option<f32>,
    pub predictor_temperature: Option<f32>,
    pub predictor_top_k: Option<usize>,
    pub predictor_top_p: Option<f32>,
    pub predictor_repetition_penalty: Option<f32>,
}

fn trimmed(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn group(
    temperature: Option<f32>,
    top_k: Option<usize>,
    top_p: Option<f32>,
    repetition_penalty: Option<f32>,
    defaults: SamplingParams,
    label: &str,
) -> Result<Option<SamplingParams>, AppError> {
    if temperature.is_none() && top_k.is_none() && top_p.is_none() && repetition_penalty.is_none() {
        return Ok(None);
    }
    let params = SamplingParams {
        temperature: temperature.unwrap_or(defaults.temperature),
        top_k: top_k.unwrap_or(defaults.top_k),
        top_p: top_p.unwrap_or(defaults.top_p),
        repetition_penalty: repetition_penalty.unwrap_or(defaults.repetition_penalty),
    };
    params.validate(label).map_err(AppError::BadRequest)?;
    Ok(Some(params))
}

impl SamplingInput {
    /// Parse and validate. Returns the sampling request (if any parameter was
    /// given) and the per-request quality mode (if given).
    pub fn into_parts(self) -> Result<(Option<SamplingRequest>, Option<DetectionMode>), AppError> {
        let quality = match trimmed(self.quality.clone()) {
            None => None,
            Some(raw) => Some(DetectionMode::parse(&raw).ok_or_else(|| {
                AppError::BadRequest(format!(
                    "unknown quality mode '{raw}' (expected off, report, retry, experimental_trim)"
                ))
            })?),
        };
        let preset = match trimmed(self.preset.clone()) {
            None => None,
            Some(raw) => Some(SamplingPreset::parse(&raw).ok_or_else(|| {
                AppError::BadRequest(format!(
                    "unknown sampling preset '{raw}' (expected balanced, stable, expressive, custom)"
                ))
            })?),
        };
        let seed = match trimmed(self.seed.clone()) {
            None => None,
            Some(raw) => Some(raw.parse::<u64>().map_err(|_| {
                AppError::BadRequest(format!("seed must be an unsigned integer, got '{raw}'"))
            })?),
        };
        let talker = group(
            self.talker_temperature,
            self.talker_top_k,
            self.talker_top_p,
            self.talker_repetition_penalty,
            BALANCED_TALKER,
            "talker",
        )?;
        let predictor = group(
            self.predictor_temperature,
            self.predictor_top_k,
            self.predictor_top_p,
            self.predictor_repetition_penalty,
            BALANCED_PREDICTOR,
            "predictor",
        )?;
        if preset.is_none() && seed.is_none() && talker.is_none() && predictor.is_none() {
            return Ok((None, quality));
        }
        Ok((
            Some(SamplingRequest {
                preset,
                talker,
                predictor,
                seed,
            }),
            quality,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_produces_no_override() {
        let (sampling, quality) = SamplingInput::default().into_parts().unwrap();
        assert!(sampling.is_none());
        assert!(quality.is_none());
    }

    #[test]
    fn preset_and_seed_parse() {
        let input = SamplingInput {
            preset: Some("stable".into()),
            seed: Some("18446744073709551615".into()),
            ..Default::default()
        };
        let (sampling, quality) = input.into_parts().unwrap();
        let sampling = sampling.unwrap();
        assert_eq!(sampling.preset, Some(SamplingPreset::Stable));
        assert_eq!(sampling.seed, Some(u64::MAX));
        assert!(quality.is_none());
    }

    #[test]
    fn quality_mode_parses_and_rejects_unknown() {
        let (_, quality) = SamplingInput {
            quality: Some("retry".into()),
            ..Default::default()
        }
        .into_parts()
        .unwrap();
        assert_eq!(quality, Some(DetectionMode::Retry));
        assert!(
            SamplingInput {
                quality: Some("loud".into()),
                ..Default::default()
            }
            .into_parts()
            .is_err()
        );
    }

    #[test]
    fn partial_parameter_group_is_filled_from_balanced_and_validated() {
        let (sampling, _) = SamplingInput {
            talker_temperature: Some(0.6),
            ..Default::default()
        }
        .into_parts()
        .unwrap();
        let talker = sampling.unwrap().talker.unwrap();
        assert_eq!(talker.temperature, 0.6);
        assert_eq!(talker.top_k, BALANCED_TALKER.top_k);
        // Out-of-range values are rejected, not clamped.
        assert!(
            SamplingInput {
                talker_top_p: Some(5.0),
                ..Default::default()
            }
            .into_parts()
            .is_err()
        );
    }

    #[test]
    fn unparseable_seed_and_unknown_preset_are_rejected() {
        assert!(
            SamplingInput {
                seed: Some("abc".into()),
                ..Default::default()
            }
            .into_parts()
            .is_err()
        );
        assert!(
            SamplingInput {
                preset: Some("turbo".into()),
                ..Default::default()
            }
            .into_parts()
            .is_err()
        );
    }
}
