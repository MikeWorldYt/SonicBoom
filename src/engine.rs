use std::{path::PathBuf, sync::Arc};

use anyhow::{Result, anyhow};

use crate::tts::{audio, download, inference, model::ModelHandle};

const MAX_TEXT_CHARS: usize = 10_000;
const MAX_INFERENCE_STEPS: usize = 50;

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub model_dir: PathBuf,
    pub hf_token: Option<String>,
    pub inference_steps: usize,
    /// Pinned HuggingFace revision. Defaults to [`download::DEFAULT_MODEL_REVISION`].
    pub model_revision: Option<String>,
    /// Optional expected-hashes manifest (see [`download::load_expected_hashes`]).
    pub model_hashes_path: Option<PathBuf>,
}

impl EngineConfig {
    pub fn new(model_dir: impl Into<PathBuf>) -> Self {
        Self {
            model_dir: model_dir.into(),
            hf_token: None,
            inference_steps: 5,
            model_revision: None,
            model_hashes_path: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SynthesisRequest {
    pub text: String,
    pub language: String,
    pub voice: String,
    pub inference_steps: usize,
}

/// Shared TTS input validation used by both the reusable engine and the
/// HTTP API so the rules cannot drift apart. Lengths are measured in Unicode
/// characters, not UTF-8 bytes.
pub fn validate_tts_input(
    text: &str,
    language: &str,
    voice: &str,
    inference_steps: usize,
    max_text_chars: usize,
) -> Result<()> {
    let text = text.trim();
    if text.is_empty() {
        return Err(anyhow!("text cannot be empty"));
    }
    if text.chars().count() > max_text_chars {
        return Err(anyhow!("text exceeds {max_text_chars} characters"));
    }
    if language.is_empty()
        || language.len() > 16
        || !language
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(anyhow!("language must be a valid language tag"));
    }
    if voice.is_empty() || voice.len() > 16 {
        return Err(anyhow!("voice must be a valid voice name"));
    }
    if !(1..=MAX_INFERENCE_STEPS).contains(&inference_steps) {
        return Err(anyhow!(
            "inferenceSteps must be between 1 and {MAX_INFERENCE_STEPS}"
        ));
    }
    Ok(())
}

impl SynthesisRequest {
    pub fn validate(&self) -> Result<()> {
        validate_tts_input(
            &self.text,
            &self.language,
            &self.voice,
            self.inference_steps,
            MAX_TEXT_CHARS,
        )
    }

    /// Adapt a legacy request to the extensible provider contract without
    /// changing legacy validation or defaults. The returned request targets
    /// `model` (a Supertonic alias by default) and carries no reference
    /// audio: cloning providers reject it explicitly until a voice profile
    /// supplies authorized reference material.
    pub fn to_provider_request(
        &self,
        model: impl Into<String>,
    ) -> crate::tts::providers::types::ProviderRequest {
        crate::tts::providers::types::ProviderRequest::new(
            model,
            self.voice.clone(),
            self.text.clone(),
            self.language.clone(),
        )
    }
}

pub struct SynthesisResult {
    pub wav: Vec<u8>,
    pub sample_rate: u32,
}

pub struct SonicBoomEngine {
    model: Arc<ModelHandle>,
    inference_lock: std::sync::Mutex<()>,
}

impl SonicBoomEngine {
    pub async fn prepare<F>(config: EngineConfig, on_progress: F) -> Result<Arc<Self>>
    where
        F: Fn(f32) + Send + Sync + 'static,
    {
        let revision = config
            .model_revision
            .clone()
            .unwrap_or_else(|| download::DEFAULT_MODEL_REVISION.to_string());
        let custom_manifest = config
            .model_hashes_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned());
        // Same trust policy as the HTTP server: a custom revision requires
        // an explicit trusted manifest for that exact revision.
        let (revision, expected_trust) =
            download::resolve_trust(&revision, custom_manifest.as_deref())?;
        let paths = download::download_models_with_options(
            &config.model_dir,
            config.hf_token.as_deref(),
            &revision,
            &expected_trust,
            &download::DownloadLimits::default(),
            on_progress,
        )
        .await?;
        let model = tokio::task::spawn_blocking(move || ModelHandle::load(&paths))
            .await
            .map_err(|error| anyhow!("model loading task failed: {error}"))??;
        Ok(Arc::new(Self {
            model: Arc::new(model),
            inference_lock: std::sync::Mutex::new(()),
        }))
    }

    pub fn voices(&self) -> impl Iterator<Item = &str> {
        self.model.voice_styles.keys().map(String::as_str)
    }

    /// Provider identity of this engine in the native registry
    /// (`supertonic`). The engine itself is unchanged: model download,
    /// voice styles, inference steps and output encoding behave exactly as
    /// before; the registry adapter only routes requests to it.
    pub fn provider_id(&self) -> &'static str {
        "supertonic"
    }

    pub fn synthesize(&self, request: &SynthesisRequest) -> Result<SynthesisResult> {
        request.validate()?;
        if !self.model.voice_styles.contains_key(&request.voice) {
            return Err(anyhow!("voice '{}' is not available", request.voice));
        }
        // The lock guards the ONNX sessions, which are touched only by
        // `inference::synthesize`. WAV encoding touches no session state, so
        // the lock is released first: encoding must never block the next
        // model inference.
        let samples = {
            let _guard = self
                .inference_lock
                .lock()
                .map_err(|_| anyhow!("inference lock poisoned"))?;
            inference::synthesize(
                &self.model,
                request.text.trim(),
                &request.language,
                &request.voice,
                request.inference_steps,
                crate::tts::text::DEFAULT_MAX_CHUNK_CHARS,
            )?
        };
        let sample_rate = self.model.sample_rate();
        let wav = audio::encode_audio(&samples, sample_rate, audio::AudioFormat::Wav)?;
        Ok(SynthesisResult { wav, sample_rate })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> SynthesisRequest {
        SynthesisRequest {
            text: "hello".to_owned(),
            language: "en".to_owned(),
            voice: "F1".to_owned(),
            inference_steps: 5,
        }
    }

    #[test]
    fn legacy_requests_adapt_to_the_provider_contract() {
        let adapted = request().to_provider_request("supertonic-3");
        assert_eq!(adapted.model, "supertonic-3");
        assert_eq!(adapted.voice, "F1");
        assert_eq!(adapted.text, "hello");
        assert_eq!(adapted.language, "en");
        assert!(adapted.reference_audio.is_none());
    }

    #[test]
    fn validates_synthesis_inputs_without_loading_a_model() {
        assert!(request().validate().is_ok());

        let mut invalid = request();
        invalid.text.clear();
        assert!(invalid.validate().is_err());

        let mut invalid = request();
        invalid.language = "en_US".to_owned();
        assert!(invalid.validate().is_err());

        let mut invalid = request();
        invalid.inference_steps = MAX_INFERENCE_STEPS + 1;
        assert!(invalid.validate().is_err());
    }
}
