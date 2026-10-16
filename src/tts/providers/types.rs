//! Shared provider types: PCM audio, requests, capabilities, status.

use serde::{Deserialize, Serialize};

/// Normalized synthesis output: mono `f32` PCM plus sample rate.
#[derive(Debug, Clone)]
pub struct PcmAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl PcmAudio {
    pub fn mono(samples: Vec<f32>, sample_rate: u32) -> Self {
        Self {
            samples,
            sample_rate,
            channels: 1,
        }
    }

    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.samples.len() as f64 / f64::from(self.sample_rate)
    }

    pub fn is_audible(&self) -> bool {
        self.samples.iter().any(|s| s.abs() > 1e-4)
    }
}

/// Voice-cloning conditioning mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloneMode {
    /// In-context learning: reference WAV plus the exact reference transcript.
    Icl,
    /// X-vector embedding: reference WAV without a transcript (Qwen only).
    XVector,
    /// Audio-prompt conditioning: reference WAV without a transcript,
    /// encoded to voice embeddings (Pocket TTS only).
    AudioPrompt,
}

impl CloneMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Icl => "icl",
            Self::XVector => "x_vector",
            Self::AudioPrompt => "audio_prompt",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "icl" => Some(Self::Icl),
            "x_vector" | "xvector" => Some(Self::XVector),
            "audio_prompt" | "audioprompt" => Some(Self::AudioPrompt),
            _ => None,
        }
    }
}

/// Requested compute device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DevicePreference {
    Cpu,
    #[default]
    Auto,
    Cuda,
}

impl DevicePreference {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "cpu" => Some(Self::Cpu),
            "auto" => Some(Self::Auto),
            "cuda" => Some(Self::Cuda),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Auto => "auto",
            Self::Cuda => "cuda",
        }
    }
}

/// Device actually selected for inference (always reported explicitly).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EffectiveDevice {
    Cpu,
    Cuda,
}

impl EffectiveDevice {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Cuda => "cuda",
        }
    }
}

/// Reference audio carried with a cloning request.
///
/// The PCM travels with the request itself — never through a shared mutable
/// slot — so concurrent requests with distinct voices cannot leak reference
/// data into each other. Providers additionally cache derived conditioning
/// under `(provider, model revision, reference hash, transcript, mode)`.
#[derive(Debug, Clone, Default)]
pub struct ReferenceAudio {
    pub pcm: Vec<f32>,
    pub sample_rate: u32,
}

/// Extensible per-request synthesis input.
///
/// `voice` is either a built-in style name (Supertonic `F1`–`F5`/`M1`–`M5`,
/// OpenAI aliases) or an authorized voice-profile ID (Qwen/Pocket TTS).
/// Provider-specific options stay out of the global Supertonic constraints:
/// each provider validates its own voice names, languages and options.
#[derive(Debug, Clone)]
pub struct ProviderRequest {
    pub model: String,
    pub voice: String,
    pub text: String,
    pub language: String,
    pub device: DevicePreference,
    pub clone_mode: Option<CloneMode>,
    /// Exact reference transcript (required for ICL; unused by Pocket TTS).
    pub reference_transcript: Option<String>,
    /// Decoded reference PCM for cloning providers (`None` for Supertonic).
    pub reference_audio: Option<ReferenceAudio>,
    /// Reserved for documented generation options (seed, sampling, ...).
    pub seed: Option<u64>,
}

impl ProviderRequest {
    pub fn new(
        model: impl Into<String>,
        voice: impl Into<String>,
        text: impl Into<String>,
        language: impl Into<String>,
    ) -> Self {
        Self {
            model: model.into(),
            voice: voice.into(),
            text: text.into(),
            language: language.into(),
            device: DevicePreference::Auto,
            clone_mode: None,
            reference_transcript: None,
            reference_audio: None,
            seed: None,
        }
    }
}

/// Static capability metadata exposed via `/api/providers`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    pub provider_id: String,
    pub models: Vec<String>,
    pub languages: Vec<String>,
    pub clone_modes: Vec<String>,
    pub devices: Vec<String>,
    pub requires_reference_audio: bool,
    pub requires_reference_transcript: bool,
    pub sample_rate: Option<u32>,
}

/// Lifecycle status snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderStatusState {
    Disabled,
    NotInstalled,
    Loading,
    Ready,
    Error,
}

impl ProviderStatusState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::NotInstalled => "not_installed",
            Self::Loading => "loading",
            Self::Ready => "ready",
            Self::Error => "error",
        }
    }
}

/// Runtime status for one provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub provider_id: String,
    pub enabled: bool,
    pub state: ProviderStatusState,
    pub active_model: Option<String>,
    pub effective_device: Option<String>,
    pub model_revision: Option<String>,
    pub detail: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_audio_reports_audibility_and_duration() {
        let silent = PcmAudio::mono(vec![0.0; 24000], 24000);
        assert!(!silent.is_audible());
        assert!((silent.duration_secs() - 1.0).abs() < 1e-9);
        let tone = PcmAudio::mono(vec![0.5; 12000], 24000);
        assert!(tone.is_audible());
        assert!((tone.duration_secs() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn device_preference_round_trips() {
        assert_eq!(DevicePreference::parse("cpu"), Some(DevicePreference::Cpu));
        assert_eq!(
            DevicePreference::parse("AUTO"),
            Some(DevicePreference::Auto)
        );
        assert_eq!(
            DevicePreference::parse("cuda"),
            Some(DevicePreference::Cuda)
        );
        assert_eq!(DevicePreference::parse("rocm"), None);
    }
}
