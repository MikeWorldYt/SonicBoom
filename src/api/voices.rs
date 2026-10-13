//! Managed voice-profile endpoints.
//!
//! Authenticated management of cloning voice profiles. Profiles are scoped
//! to the owning token (via its non-secret hash): one token's profiles are
//! invisible to another (unknown IDs report `404`, never `403` with
//! existence disclosure). Reference transcripts and audio bytes are never
//! logged.

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::IntoResponse,
};
use serde::Deserialize;

use sonicboom::tts::providers::types::{
    CloneMode, DevicePreference, ProviderRequest, ReferenceAudio,
};
use sonicboom::tts::voices::{NewVoiceProfile, VoiceProfileStore};

use super::providers::{build_registry, provider_error_to_app, supertonic_handle};
use super::tts::{check_rate_limit, synthesize_provider_bounded};
use crate::auth::AuthenticatedToken;
use crate::error::AppError;

#[derive(Deserialize)]
pub struct CloneQuery {
    pub name: String,
    pub provider: String,
    pub model: String,
    pub language: String,
    #[serde(default = "default_clone_mode")]
    pub mode: String,
    #[serde(default)]
    pub transcript: Option<String>,
    pub consent_basis: String,
    #[serde(default)]
    pub consent_note: Option<String>,
}

fn default_clone_mode() -> String {
    "icl".to_string()
}

pub(crate) fn parse_clone_mode_public(raw: &str) -> Result<CloneMode, AppError> {
    parse_clone_mode(raw)
}

fn parse_clone_mode(raw: &str) -> Result<CloneMode, AppError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "icl" => Ok(CloneMode::Icl),
        "x_vector" | "x-vector" | "xvector" => Ok(CloneMode::XVector),
        other => Err(AppError::BadRequest(format!(
            "unsupported clone mode '{other}' (expected one of: icl, x_vector)"
        ))),
    }
}

/// Decode an uploaded WAV body to mono `f32` PCM (MIME **and** actual format
/// are validated; anything undecodable is `400`, non-WAV media is `415`).
fn decode_wav_bytes(bytes: &[u8]) -> Result<(Vec<f32>, u32), AppError> {
    let mut reader = hound::WavReader::new(bytes).map_err(|_| {
        AppError::BadRequest(
            "reference audio is not a decodable WAV file (check the file contents, not just the extension)"
                .to_string(),
        )
    })?;
    let spec = reader.spec();
    if spec.channels == 0 || spec.channels > 2 {
        return Err(AppError::BadRequest(format!(
            "reference WAV has {} channels; mono or stereo is required",
            spec.channels
        )));
    }
    if spec.sample_rate == 0 {
        return Err(AppError::BadRequest(
            "reference WAV sample rate must be nonzero".to_string(),
        ));
    }
    let pcm: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| AppError::BadRequest("reference WAV samples are corrupt".to_string()))?,
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample.max(1) as f32;
            let scale = 2f32.powf(bits - 1.0);
            reader
                .samples::<i32>()
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| AppError::BadRequest("reference WAV samples are corrupt".to_string()))?
                .into_iter()
                .map(|s| s as f32 / scale)
                .collect()
        }
    };
    // Stereo is mixed down to mono explicitly (documented, not silent: the
    // stored duration/hash reflect the mono mix).
    let mono = if spec.channels == 2 {
        let mut mixed = Vec::with_capacity(pcm.len() / 2);
        let mut pairs = pcm.into_iter();
        while let (Some(left), Some(right)) = (pairs.next(), pairs.next()) {
            mixed.push((left + right) / 2.0);
        }
        mixed
    } else {
        pcm
    };
    Ok((mono, spec.sample_rate))
}

/// One resident manifest and bounded decoded-reference LRU. All disk work
/// and mutation is serialized on blocking threads, never on Tokio executors.
#[derive(Debug, Default)]
pub struct CachedVoices {
    dir: String,
    store: Option<VoiceProfileStore>,
    references: std::collections::VecDeque<(String, ReferenceAudio)>,
}
pub(crate) async fn with_store<T: Send + 'static>(
    state: &crate::AppState,
    work: impl FnOnce(&mut VoiceProfileStore) -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    let dir = state.config.get().await.voices.store_dir.clone();
    let cache = state.inference_gate.voices.clone();
    tokio::task::spawn_blocking(move || {
        let mut cached = cache
            .lock()
            .map_err(|_| AppError::internal("voice store lock poisoned"))?;
        if cached.store.is_none() || cached.dir != dir {
            cached.store = Some(
                VoiceProfileStore::open(std::path::PathBuf::from(&dir))
                    .map_err(AppError::internal)?,
            );
            cached.dir = dir;
            cached.references.clear();
        }
        work(cached.store.as_mut().expect("store initialized"))
    })
    .await
    .map_err(|e| AppError::internal(format!("voice store task failed: {e}")))?
}
/// Authorization is checked against resident metadata before looking in the
/// decoded cache. The ID, owner and reference hash scope the cache identity.
pub(crate) async fn owned_reference(
    state: &crate::AppState,
    id: String,
    owner: String,
) -> Result<(sonicboom::tts::voices::VoiceProfile, ReferenceAudio), AppError> {
    let profile = with_store(state, move |store| {
        store
            .get(&id)
            .filter(|p| p.owner_token_hash.as_deref() == Some(owner.as_str()))
            .cloned()
            .ok_or_else(|| AppError::NotFound("unknown voice profile".into()))
    })
    .await?;
    let cache = state.inference_gate.voices.clone();
    let profile_copy = profile.clone();
    let reference = tokio::task::spawn_blocking(move || {
        let mut cached = cache
            .lock()
            .map_err(|_| AppError::internal("voice cache lock poisoned"))?;
        // Recheck after acquiring the lock: deletion or store reconfiguration
        // may have occurred since the first authorization check.
        let store = cached
            .store
            .as_ref()
            .ok_or_else(|| AppError::NotFound("unknown voice profile".into()))?;
        if !store.get(&profile_copy.id).is_some_and(|p| {
            p.reference_hash == profile_copy.reference_hash
                && p.owner_token_hash == profile_copy.owner_token_hash
        }) {
            return Err(AppError::NotFound("unknown voice profile".into()));
        }
        let key = format!(
            "{}:{}:{}",
            profile_copy.id,
            profile_copy.owner_token_hash.as_deref().unwrap_or(""),
            profile_copy.reference_hash
        );
        if let Some(pos) = cached.references.iter().position(|(k, _)| k == &key) {
            let entry = cached.references.remove(pos).expect("cache index exists");
            let result = entry.1.clone();
            cached.references.push_back(entry);
            return Ok(result);
        }
        let path = cached
            .store
            .as_ref()
            .expect("store initialized")
            .reference_path(&profile_copy.id)
            .map_err(AppError::internal)?;
        let bytes =
            std::fs::read(path).map_err(|_| AppError::internal("reference audio unavailable"))?;
        let (pcm, sample_rate) = decode_wav_bytes(&bytes)?;
        let reference = ReferenceAudio { pcm, sample_rate };
        let size = reference.pcm.len() * 4;
        while cached.references.len() >= 16
            || cached
                .references
                .iter()
                .map(|(_, r)| r.pcm.len() * 4)
                .sum::<usize>()
                + size
                > 64 * 1024 * 1024
        {
            if cached.references.pop_front().is_none() {
                break;
            }
        }
        if size <= 64 * 1024 * 1024 {
            cached.references.push_back((key, reference.clone()));
        }
        Ok::<_, AppError>(reference)
    })
    .await
    .map_err(|e| AppError::internal(format!("reference decoding task failed: {e}")))??;
    Ok((profile, reference))
}

/// `POST /api/voices/clones?name=..&provider=..&model=..&language=..`: upload
/// a reference WAV (`Content-Type: audio/wav`) and create a voice profile.
pub async fn post_clones(
    token: AuthenticatedToken,
    State(state): State<crate::AppState>,
    Query(query): Query<CloneQuery>,
    headers: axum::http::HeaderMap,
    body: Body,
) -> Result<impl IntoResponse, AppError> {
    check_rate_limit(&state, &token)?;
    let config = state.config.get().await;
    let limit = config.voices.max_upload_bytes + 1;
    let body = axum::body::to_bytes(body, limit).await.map_err(|_| {
        AppError::PayloadTooLarge(format!(
            "reference upload exceeds the configured limit of {} bytes",
            config.voices.max_upload_bytes
        ))
    })?;
    if body.len() > config.voices.max_upload_bytes {
        return Err(AppError::PayloadTooLarge(format!(
            "reference upload is {} bytes; the configured limit is {} bytes",
            body.len(),
            config.voices.max_upload_bytes
        )));
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if content_type != "audio/wav" && content_type != "audio/x-wav" {
        return Err(AppError::UnsupportedMediaType(format!(
            "unsupported reference media '{content_type}' \
             (expected Content-Type: audio/wav with WAV contents; \
             FLAC/MP3 uploads are not supported by this build)"
        )));
    }
    if query.name.trim().is_empty() || query.name.chars().count() > 64 {
        return Err(AppError::BadRequest(
            "profile name must be 1-64 characters".to_string(),
        ));
    }
    let provider = query.provider.trim().to_ascii_lowercase();
    if provider != "qwen3" && provider != "gpt-sovits" {
        return Err(AppError::BadRequest(format!(
            "unsupported provider '{}' for voice cloning \
             (expected one of: qwen3, gpt-sovits; supertonic uses built-in style voices)",
            query.provider
        )));
    }
    let mode = parse_clone_mode(&query.mode)?;
    if provider == "gpt-sovits" && mode != CloneMode::Icl {
        return Err(AppError::BadRequest(
            "gpt-sovits supports only icl mode (reference WAV plus exact transcript)".to_string(),
        ));
    }
    let transcript = query
        .transcript
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty());
    if mode == CloneMode::Icl && transcript.is_none() {
        return Err(AppError::BadRequest(format!(
            "provider '{provider}' icl mode requires the exact reference transcript \
             (transcript query parameter)"
        )));
    }
    if mode == CloneMode::XVector && transcript.is_some() {
        return Err(AppError::BadRequest(
            "x_vector mode does not accept a transcript \
             (use icl mode for transcript-conditioned cloning)"
                .to_string(),
        ));
    }
    if query.consent_basis.trim().is_empty() {
        return Err(AppError::BadRequest(
            "consent_basis is required (e.g. owner, licensed, authorized)".to_string(),
        ));
    }
    // Validate model/language against provider capabilities (no silent
    // fallback to another provider's pipeline).
    let registry = build_registry(&config, supertonic_handle(&state).await, &state.providers);
    let capabilities = registry
        .get(&provider)
        .map(|p| p.capabilities())
        .ok_or_else(|| AppError::internal("cloning provider not registered"))?;
    if !capabilities.models.iter().any(|m| m == &query.model) {
        return Err(AppError::BadRequest(format!(
            "unsupported model '{}' for provider '{provider}' (expected one of: {})",
            query.model,
            capabilities.models.join(", ")
        )));
    }
    if !capabilities.languages.iter().any(|l| l == &query.language) {
        return Err(AppError::BadRequest(format!(
            "provider '{provider}' does not support language '{}' (supported: {})",
            query.language,
            capabilities.languages.join(", ")
        )));
    }
    let (pcm, rate) = decode_wav_bytes(&body)?;
    let input = NewVoiceProfile {
        name: query.name,
        provider: provider.clone(),
        model: query.model.clone(),
        model_revision: config_revision_for(&config, &provider),
        language: query.language.clone(),
        clone_mode: mode.as_str().to_string(),
        reference_pcm: pcm,
        reference_sample_rate: rate,
        transcript: transcript.map(str::to_string),
        owner_token_hash: Some(token.rate_limit_key()),
        consent_basis: query.consent_basis,
        consent_note: query.consent_note.unwrap_or_default(),
    };
    let profile = with_store(&state, move |store| {
        store.create(input).map_err(AppError::BadRequest)
    })
    .await?;
    let body = serde_json::to_string(&profile).map_err(|e| AppError::internal(e.to_string()))?;
    Ok((
        StatusCode::CREATED,
        [(header::CONTENT_TYPE, "application/json")],
        body,
    ))
}

fn config_revision_for(config: &crate::config::model::AppConfig, provider: &str) -> String {
    match provider {
        "qwen3" => config.providers.qwen3.revision.clone(),
        _ => config.providers.gpt_sovits.revision.clone(),
    }
}

/// `GET /api/voices`: list the caller's own profiles (private metadata
/// requires authorization; unauthenticated callers never reach here).
pub async fn get_voices(
    token: AuthenticatedToken,
    State(state): State<crate::AppState>,
) -> Result<impl IntoResponse, AppError> {
    let owner = token.rate_limit_key();
    let profiles = with_store(&state, move |store| {
        Ok(store
            .list()
            .into_iter()
            .filter(|p| p.owner_token_hash.as_deref() == Some(owner.as_str()))
            .cloned()
            .collect::<Vec<_>>())
    })
    .await?;
    let body = serde_json::json!({ "object": "list", "data": profiles }).to_string();
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        body,
    ))
}

/// `GET /api/voices/{id}`: one profile, or `404` (foreign/missing IDs are
/// indistinguishable to avoid existence disclosure).
pub async fn get_voice(
    token: AuthenticatedToken,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let owner = token.rate_limit_key();
    let profile = with_store(&state, move |store| {
        Ok(store
            .get(id.trim())
            .filter(|p| p.owner_token_hash.as_deref() == Some(owner.as_str()))
            .cloned())
    })
    .await?;
    match profile.as_ref() {
        Some(profile)
            if profile.owner_token_hash.as_deref() == Some(token.rate_limit_key().as_str()) =>
        {
            let body =
                serde_json::to_string(profile).map_err(|e| AppError::internal(e.to_string()))?;
            Ok((
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json")],
                body,
            ))
        }
        _ => Err(AppError::NotFound("unknown voice profile".to_string())),
    }
}

/// `DELETE /api/voices/{id}`: delete a profile and its reference audio.
pub async fn delete_voice(
    token: AuthenticatedToken,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let owner = token.rate_limit_key();
    let delete_id = id.clone();
    with_store(&state, move |store| {
        if !store
            .get(delete_id.trim())
            .is_some_and(|p| p.owner_token_hash.as_deref() == Some(owner.as_str()))
        {
            return Err(AppError::NotFound("unknown voice profile".into()));
        }
        store.delete(delete_id.trim()).map_err(AppError::internal)
    })
    .await?;
    state
        .inference_gate
        .voices
        .lock()
        .map_err(|_| AppError::internal("voice cache lock poisoned"))?
        .references
        .retain(|(key, _)| !key.starts_with(&format!("{}:", id.trim())));
    let body = serde_json::json!({ "deleted": id.trim() }).to_string();
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        body,
    ))
}

#[derive(Deserialize)]
pub struct PreviewQuery {
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
}

/// `POST /api/voices/{id}/preview`: synthesize `text/plain` with the
/// profile's provider/model/mode/reference. Shares the bounded-inference
/// gate, rate limits, body limits and timeout semantics.
pub async fn post_preview(
    token: AuthenticatedToken,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
    Query(query): Query<PreviewQuery>,
    body: String,
) -> Result<impl IntoResponse, AppError> {
    check_rate_limit(&state, &token)?;
    let config = state.config.get().await;
    let text = body.trim().to_string();
    if text.is_empty() {
        return Err(AppError::BadRequest("text cannot be empty".to_string()));
    }
    if text.chars().count() > config.tts.max_text_length {
        return Err(AppError::BadRequest(format!(
            "text exceeds {} characters",
            config.tts.max_text_length
        )));
    }
    let (profile, reference) =
        owned_reference(&state, id.trim().to_string(), token.rate_limit_key()).await?;
    let language = query.language.unwrap_or(profile.language.clone());
    let mode = parse_clone_mode(&profile.clone_mode)?;
    let format = query
        .format
        .as_deref()
        .map(sonicboom::tts::audio::AudioFormat::parse)
        .transpose()
        .map_err(|e| AppError::BadRequest(e.to_string()))?
        .unwrap_or(sonicboom::tts::audio::AudioFormat::Opus);
    let mut request = ProviderRequest::new(&profile.model, &profile.id, text, language);
    request.device = DevicePreference::Auto;
    request.clone_mode = Some(mode);
    request.reference_transcript = profile.transcript.clone();
    request.reference_audio = Some(reference);
    let registry = build_registry(&config, supertonic_handle(&state).await, &state.providers);
    let provider = registry
        .resolve_model(&profile.model)
        .map_err(provider_error_to_app)?;
    // The provider validates ownership: a profile bound to one provider can
    // never be synthesized through another provider's pipeline.
    if provider.id() != profile.provider {
        return Err(AppError::BadRequest(format!(
            "voice profile '{}' belongs to provider '{}' and cannot be used as '{}'",
            profile.id,
            profile.provider,
            provider.id()
        )));
    }
    let audio = synthesize_provider_bounded(&state, provider, request)
        .await
        .map_err(provider_error_to_app)?;
    let bytes = super::tts::encode_output(audio.samples, audio.sample_rate, format).await?;
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, format.content_type())],
        Body::from(bytes),
    )
        .into_response())
}
