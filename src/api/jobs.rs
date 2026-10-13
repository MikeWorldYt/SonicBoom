//! Private, bounded, process-local synthesis jobs. SSE carries snapshots, not audio.
use crate::{AppState, auth::AuthenticatedToken, error::AppError};
use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
};
use serde::{Deserialize, Serialize};
use sonicboom::tts::control::InferenceControl;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

tokio::task_local! { static ACTIVE: Arc<InferenceControl>; }
pub fn active_control() -> Option<Arc<InferenceControl>> {
    ACTIVE.try_with(Arc::clone).ok()
}

#[derive(Debug, Clone, Serialize)]
pub struct JobStatus {
    pub id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub stage: String,
    pub frames: usize,
    pub elapsed_secs: f64,
    pub error: Option<String>,
}
impl JobStatus {
    fn terminal(&self) -> bool {
        matches!(self.stage.as_str(), "completed" | "failed" | "cancelled")
    }
}
#[derive(Debug)]
struct Job {
    owner: String,
    control: Arc<InferenceControl>,
    status: JobStatus,
    finished: Option<Instant>,
    audio: Option<(String, Vec<u8>)>,
}
#[derive(Debug, Default)]
pub struct JobStore {
    entries: Mutex<HashMap<String, Job>>,
}
impl JobStore {
    fn reap(entries: &mut HashMap<String, Job>, retention: Duration) {
        entries.retain(|_, j| j.finished.is_none_or(|t| t.elapsed() < retention));
    }
    fn snapshot(&self, id: &str, owner: &str) -> Result<JobStatus, AppError> {
        let entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let j = entries
            .get(id)
            .filter(|j| j.owner == owner)
            .ok_or_else(|| AppError::NotFound("unknown synthesis job".into()))?;
        let mut status = j.status.clone();
        if !status.terminal() {
            let p = j.control.snapshot();
            status.stage = p.stage;
            status.frames = p.frames;
            status.elapsed_secs = p.elapsed_secs;
        }
        Ok(status)
    }
    pub fn cancel_all(&self) {
        for j in self
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
        {
            if !j.status.terminal() {
                j.control.cancel();
            }
        }
    }
}

#[derive(Deserialize)]
pub struct JobQuery {
    pub voice: Option<String>,
    pub lang: Option<String>,
    pub format: Option<String>,
    /// An owned cloning profile; omitted selects regular Supertonic synthesis.
    pub profile: Option<String>,
}

pub async fn create(
    token: AuthenticatedToken,
    State(state): State<AppState>,
    Query(query): Query<JobQuery>,
    body: String,
) -> Result<impl IntoResponse, AppError> {
    super::tts::check_rate_limit(&state, &token)?;
    let config = state.config.get().await;
    if body.trim().is_empty() || body.chars().count() > config.tts.max_text_length {
        return Err(AppError::BadRequest(
            "text is empty or exceeds configured length".into(),
        ));
    }
    let retention = Duration::from_secs(config.inference.job_retention_secs);
    let control = InferenceControl::new(Duration::from_secs(
        config.inference.generation_deadline_secs,
    ));
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now();
    let status = JobStatus {
        id: id.clone(),
        created_at: now,
        updated_at: now,
        stage: "queued".into(),
        frames: 0,
        elapsed_secs: 0.0,
        error: None,
    };
    let store = state.inference_gate.jobs.clone();
    {
        let mut entries = store.entries.lock().unwrap_or_else(|e| e.into_inner());
        JobStore::reap(&mut entries, retention);
        if entries.len() >= config.inference.max_retained_jobs
            || entries.values().filter(|j| !j.status.terminal()).count()
                >= config
                    .inference
                    .max_concurrent
                    .saturating_add(config.inference.max_pending)
        {
            return Err(AppError::TooManyRequests(
                "Synthesis job capacity reached; retry later".into(),
            ));
        }
        entries.insert(
            id.clone(),
            Job {
                owner: token.rate_limit_key(),
                control: control.clone(),
                status: status.clone(),
                finished: None,
                audio: None,
            },
        );
    }
    tokio::spawn(async move {
        let job_id = id.clone();
        let execution = tokio::spawn(ACTIVE.scope(control.clone(), async move {
            let response = if let Some(profile) = query.profile {
                super::voices::post_preview(
                    token,
                    State(state),
                    Path(profile),
                    Query(super::voices::PreviewQuery {
                        format: query.format,
                        language: query.lang,
                    }),
                    body,
                )
                .await
                .map(IntoResponse::into_response)
            } else {
                super::tts::post_tts(
                    token,
                    State(state),
                    Query(super::tts::TtsQuery {
                        voice: query.voice,
                        lang: query.lang,
                        format: query.format,
                        #[cfg(feature = "playback")]
                        play_now: None,
                    }),
                    body,
                )
                .await
            }?;
            let content_type = response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|h| h.to_str().ok())
                .unwrap_or("application/octet-stream")
                .to_owned();
            control.update("encoding", control.snapshot().frames);
            let bytes = axum::body::to_bytes(response.into_body(), 16 * 1024 * 1024)
                .await
                .map_err(|_| AppError::internal("job audio exceeds 16 MiB"))?;
            Ok::<_, AppError>((content_type, bytes.to_vec()))
        }))
        .await;
        {
            let mut entries = store.entries.lock().unwrap_or_else(|e| e.into_inner());
            let used: usize = entries
                .values()
                .filter_map(|j| j.audio.as_ref())
                .map(|(_, b)| b.len())
                .sum();
            if let Some(j) = entries.get_mut(&job_id) {
                let p = j.control.snapshot();
                j.status.frames = p.frames;
                j.status.elapsed_secs = p.elapsed_secs;
                if j.control.stopped() {
                    j.status.stage = "cancelled".into();
                } else {
                    match execution {
                        Ok(Ok((kind, bytes))) if used + bytes.len() <= 64 * 1024 * 1024 => {
                            j.audio = Some((kind, bytes));
                            j.status.stage = "completed".into();
                        }
                        Ok(Err(error)) => {
                            let detail = match &error {
                                AppError::Internal(_) => {
                                    "Inference failed; consult server logs".to_string()
                                }
                                _ => error.to_string(),
                            };
                            tracing::error!(job_id, error = %error, "synthesis job failed");
                            j.status.stage = "failed".into();
                            j.status.error = Some(detail);
                        }
                        _ => {
                            j.status.stage = "failed".into();
                            j.status.error =
                                Some("Job failed or audio retention capacity reached".into());
                        }
                    }
                }
                j.status.updated_at = chrono::Utc::now();
                j.finished = Some(Instant::now());
            }
        }
        tokio::time::sleep(retention).await;
        let mut entries = store.entries.lock().unwrap_or_else(|e| e.into_inner());
        JobStore::reap(&mut entries, retention);
    });
    Ok((StatusCode::ACCEPTED, Json(status)))
}

pub async fn status(
    token: AuthenticatedToken,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<JobStatus>, AppError> {
    Ok(Json(
        state
            .inference_gate
            .jobs
            .snapshot(&id, &token.rate_limit_key())?,
    ))
}
pub async fn cancel(
    token: AuthenticatedToken,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<JobStatus>, AppError> {
    let store = &state.inference_gate.jobs;
    let status = store.snapshot(&id, &token.rate_limit_key())?;
    if !status.terminal() {
        if let Some(j) = store
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
        {
            j.control.cancel();
        }
    }
    // Cancellation is acknowledged here; terminal state is published only after computation ends.
    Ok(Json(status))
}
pub async fn audio(
    token: AuthenticatedToken,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let entries = state
        .inference_gate
        .jobs
        .entries
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let j = entries
        .get(&id)
        .filter(|j| j.owner == token.rate_limit_key())
        .ok_or_else(|| AppError::NotFound("unknown synthesis job".into()))?;
    let (kind, bytes) = j
        .audio
        .as_ref()
        .filter(|_| j.status.stage == "completed")
        .ok_or_else(|| AppError::ServiceUnavailable("job has no completed audio".into()))?;
    Ok((
        [
            (header::CONTENT_TYPE, kind.as_str()),
            (header::CACHE_CONTROL, "private, no-store"),
        ],
        Body::from(bytes.clone()),
    )
        .into_response())
}
pub async fn events(
    token: AuthenticatedToken,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let store = state.inference_gate.jobs.clone();
    let owner = token.rate_limit_key();
    store.snapshot(&id, &owner)?;
    let stream = futures_util::stream::unfold(
        (store, id, owner, false),
        |(store, id, owner, done)| async move {
            if done {
                return None;
            }
            let status = store.snapshot(&id, &owner).ok()?;
            let terminal = status.terminal();
            let event = Event::default()
                .event("progress")
                .id(format!("{}:{}", status.stage, status.frames))
                .json_data(status)
                .ok()?;
            tokio::time::sleep(Duration::from_secs(1)).await;
            Some((
                Ok::<_, std::convert::Infallible>(event),
                (store, id, owner, terminal),
            ))
        },
    );
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
