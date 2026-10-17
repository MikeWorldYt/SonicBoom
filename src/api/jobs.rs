//! Private, bounded, process-local synthesis jobs. SSE carries snapshots, not audio.
use crate::{AppState, auth::AuthenticatedToken, error::AppError};
use axum::{
    Json,
    body::{Body, Bytes},
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

tokio::task_local! {
    static ACTIVE: Arc<InferenceControl>;
    static REFERENCE_LEASE: Arc<tokio::sync::OwnedSemaphorePermit>;
    static COMPLETION_LEASE: Arc<tokio::sync::OwnedSemaphorePermit>;
    static ADMISSION: std::cell::RefCell<Option<super::gate::InferenceAdmission>>;
}
pub fn take_admission() -> Option<super::gate::InferenceAdmission> {
    ADMISSION.try_with(|a| a.borrow_mut().take()).ok().flatten()
}
pub fn preparation_lease() -> Option<Arc<tokio::sync::OwnedSemaphorePermit>> {
    ADMISSION
        .try_with(|a| a.borrow().as_ref().map(|a| a.admitted.clone()))
        .ok()
        .flatten()
        .or_else(|| COMPLETION_LEASE.try_with(Arc::clone).ok())
}
pub fn reference_lease() -> Option<Arc<tokio::sync::OwnedSemaphorePermit>> {
    REFERENCE_LEASE.try_with(Arc::clone).ok()
}
pub async fn with_reference_memory<T>(
    state: &AppState,
    work: impl std::future::Future<Output = Result<T, AppError>>,
) -> Result<T, AppError> {
    let lease = state
        .inference_gate
        .reference_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| AppError::TooManyRequests("Reference preparation capacity reached".into()))?;
    REFERENCE_LEASE.scope(Arc::new(lease), work).await
}
/// Reserve before cloning/decoding references, so even preparation has bounded
/// admission. Existing background-job reservations are reused by sync handlers.
pub async fn with_admission<T>(
    state: &AppState,
    work: impl std::future::Future<Output = Result<T, AppError>>,
) -> Result<T, AppError> {
    if let Some(lease) = preparation_lease() {
        return COMPLETION_LEASE.scope(lease, work).await;
    }
    let admission = state
        .inference_gate
        .reserve()
        .map_err(|_| AppError::TooManyRequests("Inference admission is full".into()))?;
    let lease = admission.admitted.clone();
    COMPLETION_LEASE
        .scope(
            lease,
            ADMISSION.scope(std::cell::RefCell::new(Some(admission)), work),
        )
        .await
}
pub async fn with_deadline<T>(secs: u64, work: impl std::future::Future<Output = T>) -> T {
    let control =
        active_control().unwrap_or_else(|| InferenceControl::new(Duration::from_secs(secs)));
    ACTIVE.scope(control, work).await
}
pub fn active_control() -> Option<Arc<InferenceControl>> {
    ACTIVE.try_with(Arc::clone).ok()
}

#[derive(Debug, Clone, Serialize)]
pub struct JobStatus {
    pub id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub context: Option<sonicboom::tts::control::ExecutionContext>,
    pub cancellation_requested: bool,
    pub deadline_exceeded: bool,
    pub stage: String,
    pub frames: usize,
    pub elapsed_secs: f64,
    pub audio_secs: Option<f64>,
    pub rtf: Option<f64>,
    pub error: Option<String>,
    pub stage_secs: std::collections::BTreeMap<String, f64>,
    /// Playback disposition when the job requested `playback=enqueue`. Absent
    /// for synthesis-only jobs, so the legacy contract is unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playback: Option<PlaybackStatus>,
}
impl JobStatus {
    fn terminal(&self) -> bool {
        matches!(self.stage.as_str(), "completed" | "failed" | "cancelled")
    }
}

/// Playback outcome for a completed synthesis job.
///
/// Deliberately separate from `stage`: `completed` still means "synthesis
/// finished and the audio is downloadable", never "audible playback
/// finished". The device backend cannot acknowledge completion, so the only
/// honest terminal states are [`queued`](Self::state) / [`started`]:
///
/// - `queued` — admitted to the server playback queue; completion unverified.
/// - `started` — `play_now` accepted the interruption and began playback;
///   completion unverified.
/// - `failed` — admission was refused; [`error`](Self::error) is sanitized.
/// - `skipped` — synthesis was cancelled or failed, so nothing was enqueued.
#[derive(Debug, Clone, Serialize)]
pub struct PlaybackStatus {
    pub state: String,
    pub play_now: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
/// Per-job completed-audio cap enforced when buffering the response body.
pub const MAX_JOB_AUDIO_BYTES: usize = 16 * 1024 * 1024;
/// Total retained completed-audio cap across all jobs.
pub const MAX_RETAINED_AUDIO_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug)]
struct Job {
    owner: String,
    control: Arc<InferenceControl>,
    status: JobStatus,
    finished: Option<Instant>,
    retention: Duration,
    /// Reference-counted audio: concurrent fetches share the buffer with an
    /// O(1) handle clone instead of copying the whole payload, and expiry
    /// drops the store's handle without touching in-flight responses.
    audio: Option<(String, Bytes)>,
}
/// Hard cap on remembered idempotency keys. Bounds memory independently of
/// the job store: the oldest record is evicted first.
const MAX_IDEMPOTENCY_ENTRIES: usize = 1024;

/// One remembered idempotency key: the request fingerprint it was first
/// submitted with and the job it produced. Records expire on the same clock
/// as job retention, so a retry can never outlive its job by much.
#[derive(Debug)]
struct IdempotencyEntry {
    fingerprint: String,
    job_id: String,
    recorded: Instant,
    retention: Duration,
}

#[derive(Debug, Default)]
pub struct JobStore {
    entries: Mutex<HashMap<String, Job>>,
    /// Principal-scoped idempotency keys. Keyed by `owner\x1f<key>` so two
    /// tenants can never observe or collide with each other's keys. Never
    /// nested inside the `entries` lock, so the lock order stays acyclic.
    idempotency: Mutex<HashMap<String, IdempotencyEntry>>,
    /// Incrementally maintained sum of resident `audio` payload bytes.
    /// Every mutation happens while the `entries` lock is held, so relaxed
    /// accesses under that lock are exact and completion no longer scans
    /// all jobs to recompute the total.
    retained_bytes: std::sync::atomic::AtomicUsize,
}
impl JobStore {
    /// Remove expired jobs, subtracting their audio from the retained total.
    /// Callers must hold the `entries` lock (same discipline as before).
    fn reap_locked(&self, entries: &mut HashMap<String, Job>) {
        let mut freed = 0usize;
        entries.retain(|_, j| {
            let keep = j.finished.is_none_or(|t| t.elapsed() < j.retention);
            if !keep {
                freed = freed.saturating_add(j.audio.as_ref().map(|(_, b)| b.len()).unwrap_or(0));
            }
            keep
        });
        let owed = self
            .retained_bytes
            .load(std::sync::atomic::Ordering::Relaxed)
            .min(freed);
        self.retained_bytes
            .fetch_sub(owed, std::sync::atomic::Ordering::Relaxed);
    }

    /// Current retained audio bytes. Exact when called with the `entries`
    /// lock held (all mutations take that lock).
    fn retained_locked(&self) -> usize {
        self.retained_bytes
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Retain a completed job's audio under the total cap. Returns `false`
    /// (retaining nothing) when the entry is gone or the cap is reached;
    /// saturating addition keeps a pathological length from wrapping the
    /// check. Callers must hold the `entries` lock.
    fn store_completed_audio_locked(
        &self,
        entries: &mut HashMap<String, Job>,
        job_id: &str,
        kind: String,
        bytes: Bytes,
    ) -> bool {
        if self.retained_locked().saturating_add(bytes.len()) > MAX_RETAINED_AUDIO_BYTES {
            return false;
        }
        let Some(job) = entries.get_mut(job_id) else {
            return false;
        };
        // A completed result is immutable. Late duplicate completion must not
        // replace audio or double-count its retained bytes; stopped jobs never
        // acquire a downloadable payload.
        if job.audio.is_some() || job.status.terminal() || job.control.stopped() {
            return false;
        }
        self.retained_bytes
            .fetch_add(bytes.len(), std::sync::atomic::Ordering::Relaxed);
        job.audio = Some((kind, bytes));
        true
    }
    /// Drop idempotency records past their retention window. Callers must hold
    /// the `idempotency` lock.
    fn reap_idempotency_locked(&self, idem: &mut HashMap<String, IdempotencyEntry>) {
        idem.retain(|_, entry| entry.recorded.elapsed() < entry.retention);
    }

    /// Remember one admitted idempotency key, evicting the oldest record when
    /// the store is full so the map can never grow without bound.
    fn remember_idempotency_locked(
        &self,
        idem: &mut HashMap<String, IdempotencyEntry>,
        key: String,
        fingerprint: String,
        job_id: String,
        retention: Duration,
    ) {
        if idem.len() >= MAX_IDEMPOTENCY_ENTRIES
            && let Some(oldest) = idem
                .iter()
                .min_by_key(|(_, entry)| entry.recorded)
                .map(|(key, _)| key.clone())
        {
            idem.remove(&oldest);
        }
        idem.insert(
            key,
            IdempotencyEntry {
                fingerprint,
                job_id,
                recorded: Instant::now(),
                retention,
            },
        );
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
            status.cancellation_requested = j.control.cancellation_requested();
            status.deadline_exceeded = j.control.deadline_exceeded();
            status.context = p.context;
            status.stage = p.stage;
            status.frames = p.frames;
            status.elapsed_secs = p.elapsed_secs;
            status.audio_secs = p.audio_secs;
            status.rtf = p.audio_secs.map(|audio| p.elapsed_secs / audio);
            status.updated_at = p.updated_at;
            status.stage_secs = p.stage_secs;
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
    /// Provider routing for synthesis, gated exactly like the synchronous
    /// `/api/tts/play` surface: provider selection needs the `playback` feature.
    #[cfg(feature = "playback")]
    pub provider: Option<String>,
    /// Ordered playback: `enqueue` admits the completed audio to the server
    /// playback queue; omitted, empty or `none` keeps the legacy synthesis-only
    /// contract. Any other value is rejected with `400`.
    pub playback: Option<String>,
    /// Interrupt current audio and play as soon as the audio is queued. Only
    /// valid together with `playback=enqueue`.
    pub play_now: Option<bool>,
}

pub async fn create(
    token: AuthenticatedToken,
    State(state): State<AppState>,
    Query(query): Query<JobQuery>,
    headers: axum::http::HeaderMap,
    body: String,
) -> Result<Response, AppError> {
    create_with_executor(
        token,
        State(state),
        Query(query),
        idempotency_key(&headers),
        body,
        execute_request,
    )
    .await
}

/// Extracts a usable `Idempotency-Key` header. A missing, empty, over-long or
/// non-ASCII-printable value is treated as absent: a malformed header degrades
/// to the legacy non-idempotent behavior instead of failing the request.
fn idempotency_key(headers: &axum::http::HeaderMap) -> Option<String> {
    const MAX_IDEMPOTENCY_KEY_LEN: usize = 200;
    let raw = headers.get("idempotency-key")?.to_str().ok()?;
    let key = raw.trim();
    if key.is_empty() || key.len() > MAX_IDEMPOTENCY_KEY_LEN {
        return None;
    }
    if !key.bytes().all(|byte| byte.is_ascii_graphic()) {
        return None;
    }
    Some(key.to_owned())
}

/// Effective synthesis provider id, for fingerprinting only.
#[cfg(feature = "playback")]
fn job_provider(query: &JobQuery) -> &str {
    query.provider.as_deref().unwrap_or("")
}
#[cfg(not(feature = "playback"))]
fn job_provider(_query: &JobQuery) -> &str {
    ""
}

/// Stable fingerprint of everything that changes a job's request. Two
/// submissions with the same idempotency key must fingerprint identically,
/// otherwise the key is being reused for a different request. The spoken text
/// is hashed, never stored, so diagnostics carry no user content.
fn request_fingerprint(query: &JobQuery, body: &str, playback: &PlaybackRequest) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(body.as_bytes());
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}",
        job_provider(query),
        query.profile.as_deref().unwrap_or(""),
        query.voice.as_deref().unwrap_or(""),
        query.lang.as_deref().unwrap_or(""),
        query.format.as_deref().unwrap_or(""),
        if playback.requested {
            "enqueue"
        } else {
            "none"
        },
        playback.play_now,
        hex::encode(hasher.finalize()),
    )
}

/// Validated playback request derived from the job query.
#[derive(Debug, Clone, Copy)]
struct PlaybackRequest {
    requested: bool,
    play_now: bool,
}

impl PlaybackRequest {
    fn parse(query: &JobQuery) -> Result<Self, AppError> {
        let requested = match query.playback.as_deref().map(str::trim) {
            None | Some("") | Some("none") => false,
            Some("enqueue") => true,
            Some(other) => {
                return Err(AppError::BadRequest(format!(
                    "unknown playback mode `{other}`; expected `enqueue` or `none`"
                )));
            }
        };
        let play_now = query.play_now.unwrap_or(false);
        if play_now && !requested {
            return Err(AppError::BadRequest(
                "play_now requires playback=enqueue".into(),
            ));
        }
        if requested {
            #[cfg(not(feature = "playback"))]
            return Err(AppError::ServiceUnavailable(
                "audio playback is not enabled in this build".into(),
            ));
        }
        Ok(Self {
            requested,
            play_now,
        })
    }
}

/// Client-safe text for a playback admission failure. Internal details stay
/// server-side, matching how job synthesis errors are reported.
#[cfg(feature = "playback")]
fn playback_error_text(error: &AppError) -> String {
    match error {
        AppError::Internal(_) => "playback failed; consult server logs".into(),
        other => other.to_string(),
    }
}

/// Why a requested playback never reached the queue. Never carries payload.
#[cfg(feature = "playback")]
fn playback_skip_reason(stage: &str) -> String {
    match stage {
        "cancelled" => "synthesis cancelled".into(),
        "failed" => "synthesis failed".into(),
        other => format!("synthesis did not complete ({other})"),
    }
}

/// Resolves an idempotency key to its original job. Acquires the idempotency
/// lock only, so it can run before admission is reserved.
fn lookup_idempotent(
    store: &JobStore,
    scoped_key: Option<&String>,
    fingerprint: &str,
    owner: &str,
) -> Result<Option<JobStatus>, AppError> {
    let Some(key) = scoped_key else {
        return Ok(None);
    };
    let mut idem = store.idempotency.lock().unwrap_or_else(|e| e.into_inner());
    store.reap_idempotency_locked(&mut idem);
    lookup_idempotent_locked(store, &mut idem, Some(key), fingerprint, owner)
}

/// Shared lookup used by both the fast path and the atomic admission check.
/// Callers must hold the `idempotency` lock and have reaped it. A key reused
/// for a different request is a `409` conflict; a remembered job that has
/// already been reaped is forgotten and reported as absent.
fn lookup_idempotent_locked(
    store: &JobStore,
    idem: &mut HashMap<String, IdempotencyEntry>,
    scoped_key: Option<&String>,
    fingerprint: &str,
    owner: &str,
) -> Result<Option<JobStatus>, AppError> {
    let Some(key) = scoped_key else {
        return Ok(None);
    };
    let Some(entry) = idem.get(key) else {
        return Ok(None);
    };
    if entry.fingerprint != fingerprint {
        return Err(AppError::Conflict(
            "idempotency key reused with a different request".into(),
        ));
    }
    let job_id = entry.job_id.clone();
    match store.snapshot(&job_id, owner) {
        Ok(status) => Ok(Some(status)),
        Err(_) => {
            idem.remove(key);
            Ok(None)
        }
    }
}

async fn execute_request(
    token: AuthenticatedToken,
    state: AppState,
    query: JobQuery,
    body: String,
) -> Result<Response, AppError> {
    if let Some(profile) = query.profile {
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
                // Synthesis stays playback-free here: the job admits playback
                // itself once, after generation, under its own idempotency and
                // cancellation checks.
                #[cfg(feature = "playback")]
                provider: query.provider.clone(),
                voice: query.voice,
                lang: query.lang,
                format: query.format,
                #[cfg(feature = "playback")]
                play_now: None,
            }),
            body,
        )
        .await
    }
}

// The executor boundary keeps admission, immutable input ownership, terminal
// publication and downloads identical in model-independent HTTP regressions.
async fn create_with_executor<F, Fut>(
    token: AuthenticatedToken,
    State(state): State<AppState>,
    Query(query): Query<JobQuery>,
    idem_key: Option<String>,
    body: String,
    execute: F,
) -> Result<Response, AppError>
where
    F: FnOnce(AuthenticatedToken, AppState, JobQuery, String) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<Response, AppError>> + Send + 'static,
{
    super::tts::check_rate_limit(&state, &token)?;
    let config = state.config.get().await;
    if body.trim().is_empty() || body.chars().count() > config.tts.max_text_length {
        return Err(AppError::BadRequest(
            "text is empty or exceeds configured length".into(),
        ));
    }
    // Playback is validated before admission so an unsupported mode never
    // creates (and then abandons) a job.
    let playback = PlaybackRequest::parse(&query)?;
    let owner = token.rate_limit_key();
    let fingerprint = request_fingerprint(&query, &body, &playback);
    let scoped_key = idem_key.map(|key| format!("{owner}\u{1f}{key}"));
    let retention = Duration::from_secs(config.inference.job_retention_secs);
    let store = state.inference_gate.jobs.clone();

    // Fast path: an already-admitted retry returns the original job without
    // reserving admission, so idempotency still holds while the gate is
    // saturated. Repeat submissions never launch a second synthesis or
    // playback; a key reused for a different request is a conflict.
    if let Some(status) = lookup_idempotent(&store, scoped_key.as_ref(), &fingerprint, &owner)? {
        return Ok((StatusCode::OK, Json(status)).into_response());
    }

    let admission = state
        .inference_gate
        .reserve()
        .map_err(|_| AppError::TooManyRequests("Inference admission is full".into()))?;
    let control = InferenceControl::new(Duration::from_secs(
        config.inference.generation_deadline_secs,
    ));
    let id = control.id.clone();
    let now = chrono::Utc::now();
    let status = JobStatus {
        id: id.clone(),
        created_at: now,
        updated_at: now,
        context: None,
        cancellation_requested: false,
        deadline_exceeded: false,
        stage: "queued".into(),
        frames: 0,
        elapsed_secs: 0.0,
        audio_secs: None,
        rtf: None,
        error: None,
        stage_secs: std::collections::BTreeMap::new(),
        playback: None,
    };
    {
        // Re-check under the idempotency lock and insert atomically: two
        // concurrent identical submissions cannot both admit a job.
        let mut idem = store.idempotency.lock().unwrap_or_else(|e| e.into_inner());
        store.reap_idempotency_locked(&mut idem);
        if let Some(existing) =
            lookup_idempotent_locked(&store, &mut idem, scoped_key.as_ref(), &fingerprint, &owner)?
        {
            return Ok((StatusCode::OK, Json(existing)).into_response());
        }
        let mut entries = store.entries.lock().unwrap_or_else(|e| e.into_inner());
        store.reap_locked(&mut entries);
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
                owner: owner.clone(),
                control: control.clone(),
                status: status.clone(),
                finished: None,
                retention,
                audio: None,
            },
        );
        if let Some(key) = scoped_key.clone() {
            store.remember_idempotency_locked(
                &mut idem,
                key,
                fingerprint.clone(),
                id.clone(),
                retention,
            );
        }
    }
    tokio::spawn(async move {
        let job_id = id.clone();
        #[cfg(feature = "playback")]
        let playback_state = state.clone();
        let execution = tokio::spawn(ADMISSION.scope(
            std::cell::RefCell::new(Some(admission)),
            ACTIVE.scope(control.clone(), async move {
                let response = execute(token, state, query, body).await?;
                let content_type = response
                    .headers()
                    .get(header::CONTENT_TYPE)
                    .and_then(|h| h.to_str().ok())
                    .unwrap_or("application/octet-stream")
                    .to_owned();
                control.update("encoding", control.snapshot().frames);
                let bytes = axum::body::to_bytes(response.into_body(), MAX_JOB_AUDIO_BYTES)
                    .await
                    .map_err(|_| AppError::internal("job audio exceeds 16 MiB"))?;
                // `Bytes` moves by handle: no copy of the audio payload.
                Ok::<_, AppError>((content_type, bytes))
            }),
        ))
        .await;
        // Audio handle for a playback job that just completed; admitted after
        // the terminal state is published so a job that reported failure never
        // plays. `None` under the synthesis-only feature set.
        #[cfg(feature = "playback")]
        let mut pending_playback: Option<Bytes> = None;
        {
            let mut entries = store.entries.lock().unwrap_or_else(|e| e.into_inner());
            // Snapshot first (short borrow), then resolve the terminal state:
            // the audio-store helper needs `&mut entries`, so no entry
            // borrow survives across it.
            let stopped = if let Some(j) = entries.get_mut(&job_id) {
                let p = j.control.snapshot();
                j.status.cancellation_requested = j.control.cancellation_requested();
                j.status.deadline_exceeded = j.control.deadline_exceeded();
                j.status.context = p.context;
                j.status.frames = p.frames;
                j.status.elapsed_secs = p.elapsed_secs;
                j.status.audio_secs = p.audio_secs;
                j.status.rtf = p.audio_secs.map(|audio| p.elapsed_secs / audio);
                j.status.stage_secs = p.stage_secs;
                Some(j.control.stopped())
            } else {
                // Entry vanished mid-completion (only expiry removes entries,
                // and only finished ones, so this is defensive): skip terminal
                // publication but still run the retention sleep below.
                None
            };
            if stopped.is_some_and(|s| s) {
                if let Some(j) = entries.get_mut(&job_id) {
                    j.status.stage = "cancelled".into();
                }
            } else {
                match execution {
                    Ok(Ok((kind, bytes))) => {
                        if store.store_completed_audio_locked(&mut entries, &job_id, kind, bytes) {
                            if let Some(j) = entries.get_mut(&job_id) {
                                j.status.stage = "completed".into();
                            }
                        } else if let Some(j) = entries.get_mut(&job_id) {
                            if j.control.stopped() {
                                j.status.stage = "cancelled".into();
                                j.status.cancellation_requested =
                                    j.control.cancellation_requested();
                                j.status.deadline_exceeded = j.control.deadline_exceeded();
                            } else {
                                j.status.stage = "failed".into();
                                j.status.error =
                                    Some("Job failed or audio retention capacity reached".into());
                            }
                        }
                    }
                    Ok(Err(error)) => {
                        let detail = match &error {
                            AppError::Internal(_) => {
                                "Inference failed; consult server logs".to_string()
                            }
                            _ => error.to_string(),
                        };
                        tracing::error!(job_id, error = %error, "synthesis job failed");
                        if let Some(j) = entries.get_mut(&job_id) {
                            j.status.stage = "failed".into();
                            j.status.error = Some(detail);
                        }
                    }
                    _ => {
                        if let Some(j) = entries.get_mut(&job_id) {
                            j.status.stage = "failed".into();
                            j.status.error =
                                Some("Job failed or audio retention capacity reached".into());
                        }
                    }
                }
            }
            if let Some(j) = entries.get_mut(&job_id) {
                j.status.updated_at = chrono::Utc::now();
                j.finished = Some(Instant::now());
                // Publish the playback intent before admitting it so a fast
                // poller sees `pending` rather than a misleading `null`. A job
                // that did not complete is reported `skipped`, never enqueued.
                #[cfg(feature = "playback")]
                if playback.requested {
                    let completed = j.status.stage == "completed";
                    j.status.playback = Some(PlaybackStatus {
                        state: if completed { "pending" } else { "skipped" }.into(),
                        play_now: playback.play_now,
                        queue_id: None,
                        error: if completed {
                            None
                        } else {
                            Some(playback_skip_reason(&j.status.stage))
                        },
                    });
                    if completed {
                        pending_playback = j.audio.as_ref().map(|(_, bytes)| bytes.clone());
                    }
                }
            }
        }
        // Playback admission runs exactly once, outside the store lock, only
        // for a job that produced retained audio. A failure here is recorded on
        // the job; it never rewrites the synthesis result. The temp file is
        // owned by the audio thread on success and deleted on rejection.
        #[cfg(feature = "playback")]
        if let Some(bytes) = pending_playback {
            let resolved =
                match super::tts::enqueue_encoded_wav(&playback_state, &bytes, playback.play_now)
                    .await
                {
                    Ok(outcome) => PlaybackStatus {
                        state: if outcome.played_now {
                            "started"
                        } else {
                            "queued"
                        }
                        .into(),
                        play_now: playback.play_now,
                        queue_id: Some(outcome.id),
                        error: None,
                    },
                    Err(error) => PlaybackStatus {
                        state: "failed".into(),
                        play_now: playback.play_now,
                        queue_id: None,
                        error: Some(playback_error_text(&error)),
                    },
                };
            let mut entries = store.entries.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(j) = entries.get_mut(&job_id) {
                j.status.playback = Some(resolved);
                j.status.updated_at = chrono::Utc::now();
            }
        }
        tokio::time::sleep(retention).await;
        let mut entries = store.entries.lock().unwrap_or_else(|e| e.into_inner());
        store.reap_locked(&mut entries);
    });
    Ok((StatusCode::ACCEPTED, Json(status)).into_response())
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
    if !status.terminal()
        && let Some(j) = store
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
        && !j.status.terminal()
    {
        j.control.cancel();
    }
    // Cancellation is acknowledged here; terminal state is published only after computation ends.
    Ok(Json(store.snapshot(&id, &token.rate_limit_key())?))
}
pub async fn audio(
    token: AuthenticatedToken,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    audio_response(&state.inference_gate.jobs, &id, &token.rate_limit_key())
}

fn audio_response(store: &JobStore, id: &str, owner: &str) -> Result<Response, AppError> {
    let entries = store.entries.lock().unwrap_or_else(|e| e.into_inner());
    let j = entries
        .get(id)
        .filter(|j| j.owner == owner)
        .ok_or_else(|| AppError::NotFound("unknown synthesis job".into()))?;
    let (kind, bytes) = j
        .audio
        .as_ref()
        .filter(|_| j.status.stage == "completed")
        .ok_or_else(|| AppError::ServiceUnavailable("job has no completed audio".into()))?;
    // `Bytes::clone` bumps the refcount only: concurrent fetches share the
    // retained buffer with no copy, and expiry never touches in-flight data.
    // (`kind` borrows the locked entry, so the guard stays live through the
    // synchronous response construction, exactly as before.)
    let body = Body::from(bytes.clone());
    Ok((
        [
            (header::CONTENT_TYPE, kind.as_str()),
            (header::CACHE_CONTROL, "private, no-store"),
        ],
        body,
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

#[cfg(test)]
mod tests {
    use super::*;
    fn mock_wav(text: &str, query: &JobQuery) -> Vec<u8> {
        use sha2::{Digest, Sha256};
        let identity = format!(
            "{}|{}|{}|{}",
            text,
            query.lang.as_deref().unwrap_or(""),
            query.profile.as_deref().unwrap_or(""),
            query.voice.as_deref().unwrap_or("")
        );
        let samples: Vec<f32> = Sha256::digest(identity)
            .iter()
            .map(|b| (*b as f32 - 128.0) / 256.0)
            .collect();
        sonicboom::tts::audio::encode_audio(
            &samples,
            24000,
            sonicboom::tts::audio::AudioFormat::Wav,
        )
        .unwrap()
    }

    async fn mock_create(
        token: AuthenticatedToken,
        state: State<AppState>,
        query: Query<JobQuery>,
        headers: axum::http::HeaderMap,
        body: String,
    ) -> Result<Response, AppError> {
        create_with_executor(
            token,
            state,
            query,
            idempotency_key(&headers),
            body,
            |_token, state, query, body| async move {
                state
                    .inference_gate
                    .run_bounded(move || {
                        if body == "fail" {
                            return Err(AppError::BadRequest("synthetic failure".into()));
                        }
                        if body == "wait" {
                            let control = sonicboom::tts::control::current().unwrap();
                            control.update("mock", 1);
                            while !control.stopped() {
                                std::thread::sleep(Duration::from_millis(1));
                            }
                        } else {
                            // Overlapping callers must serialize at the real model
                            // gate, with the permit owned by this blocking worker.
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        Ok((
                            [(header::CONTENT_TYPE, "audio/wav")],
                            mock_wav(&body, &query),
                        )
                            .into_response())
                    })
                    .await
                    .map_err(|e| AppError::internal(format!("mock execution: {e:?}")))?
            },
        )
        .await
    }

    #[tokio::test]
    async fn mock_http_jobs_keep_inputs_owners_failures_and_cancellation_isolated() {
        use crate::auth::store::TokenStore;
        use axum::{
            Router,
            routing::{get, post},
        };
        use tower::ServiceExt;
        let path =
            std::env::temp_dir().join(format!("sonicboom-job-mock-{}.json", uuid::Uuid::new_v4()));
        let tokens = TokenStore::load(path.to_str().unwrap()).await.unwrap();
        let (_, alice) = tokens.create(None).await.unwrap();
        let (_, bob) = tokens.create(None).await.unwrap();
        let mut config = crate::api::tests::test_config();
        config.inference.generation_deadline_secs = 1;
        let state = crate::api::tests::test_state(config, Arc::new(tokens), Arc::new(None));
        let app = Router::new()
            .route("/jobs", post(mock_create))
            .route("/jobs/{id}", get(status))
            .route("/jobs/{id}/audio", get(audio))
            .route("/jobs/{id}/cancel", post(cancel))
            .with_state(state);
        let send = |method: &str, uri: String, token: &str, body: &str| {
            let request = axum::http::Request::builder()
                .method(method)
                .uri(uri)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "text/plain")
                .body(Body::from(body.to_string()))
                .unwrap();
            let app = app.clone();
            async move { app.oneshot(request).await.unwrap() }
        };
        let json = |response: Response| async move {
            serde_json::from_slice::<serde_json::Value>(
                &axum::body::to_bytes(response.into_body(), 8192)
                    .await
                    .unwrap(),
            )
            .unwrap()
        };
        let (first, second) = tokio::join!(
            send(
                "POST",
                "/jobs?lang=es&voice=M1&format=wav".into(),
                &alice,
                "hola"
            ),
            send(
                "POST",
                "/jobs?lang=es&profile=voice-B&format=wav".into(),
                &bob,
                "test"
            )
        );
        let a = json(first).await["id"].as_str().unwrap().to_string();
        let b = json(second).await["id"].as_str().unwrap().to_string();
        let c = json(
            send(
                "POST",
                "/jobs?lang=es&voice=M1&format=wav".into(),
                &alice,
                "mundo",
            )
            .await,
        )
        .await["id"]
            .as_str()
            .unwrap()
            .to_string();
        let d = json(
            send(
                "POST",
                "/jobs?lang=es&voice=M1&format=wav".into(),
                &alice,
                "test",
            )
            .await,
        )
        .await["id"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(
            [&a, &b, &c, &d]
                .into_iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            4
        );
        for (id, owner, text, profile, voice) in [
            (&a, &alice, "hola", None, Some("M1")),
            (&b, &bob, "test", Some("voice-B"), None),
            (&c, &alice, "mundo", None, Some("M1")),
            (&d, &alice, "test", None, Some("M1")),
        ] {
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    let snapshot = json(send("GET", format!("/jobs/{id}"), owner, "").await).await;
                    assert!(snapshot.get("text").is_none());
                    if snapshot["stage"] == "completed" {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            let expected = mock_wav(
                text,
                &JobQuery {
                    lang: Some("es".into()),
                    profile: profile.map(str::to_string),
                    voice: voice.map(str::to_string),
                    format: Some("wav".into()),
                    #[cfg(feature = "playback")]
                    provider: None,
                    playback: None,
                    play_now: None,
                },
            );
            let response = send("GET", format!("/jobs/{id}/audio"), owner, "").await;
            assert_eq!(
                axum::body::to_bytes(response.into_body(), 8192)
                    .await
                    .unwrap()
                    .as_ref(),
                expected
            );
        }
        assert_eq!(
            send("GET", format!("/jobs/{b}/audio"), &alice, "")
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
        for (body, cancelled, expected_stage) in [
            ("fail", false, "failed"),
            ("wait", true, "cancelled"),
            ("wait", false, "cancelled"),
        ] {
            let id = json(send("POST", "/jobs?lang=es&voice=M1".into(), &alice, body).await).await
                ["id"]
                .as_str()
                .unwrap()
                .to_string();
            if cancelled {
                tokio::time::timeout(Duration::from_secs(1), async {
                    loop {
                        let snapshot =
                            json(send("GET", format!("/jobs/{id}"), &alice, "").await).await;
                        if snapshot["stage"] == "mock" {
                            break;
                        }
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
                send("POST", format!("/jobs/{id}/cancel"), &alice, "").await;
            }
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    let snapshot = json(send("GET", format!("/jobs/{id}"), &alice, "").await).await;
                    if snapshot["stage"] == expected_stage {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert_eq!(
                send("GET", format!("/jobs/{id}/audio"), &alice, "")
                    .await
                    .status(),
                StatusCode::SERVICE_UNAVAILABLE
            );
        }
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn separate_http_downloads_preserve_job_and_owner_identity() {
        use axum::{Router, routing::get};
        use tower::ServiceExt;
        let store = Arc::new(JobStore::default());
        let (a, job_a) = unfinished_job("alice", Duration::from_secs(60));
        let (b, job_b) = unfinished_job("bob", Duration::from_secs(60));
        let pcm_a = vec![0.1f32; 24];
        let pcm_b = vec![-0.2f32; 48];
        let wav = |pcm: &[f32]| {
            sonicboom::tts::audio::encode_audio(pcm, 24000, sonicboom::tts::audio::AudioFormat::Wav)
                .unwrap()
        };
        let bytes_a = wav(&pcm_a);
        let bytes_b = wav(&pcm_b);
        {
            let mut entries = store.entries.lock().unwrap();
            entries.insert(a.clone(), job_a);
            entries.insert(b.clone(), job_b);
            for (id, bytes) in [(&a, &bytes_a), (&b, &bytes_b)] {
                assert!(store.store_completed_audio_locked(
                    &mut entries,
                    id,
                    "audio/wav".into(),
                    Bytes::from(bytes.clone())
                ));
                entries.get_mut(id).unwrap().status.stage = "completed".into();
            }
        }
        let app = Router::new().route(
            "/{owner}/{id}",
            get(move |Path((owner, id)): Path<(String, String)>| {
                let store = store.clone();
                async move { audio_response(&store, &id, &owner) }
            }),
        );
        let fetch = |path: String| {
            let app = app.clone();
            async move {
                app.oneshot(
                    axum::http::Request::builder()
                        .uri(path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
            }
        };
        let (response_a, response_b, foreign) = tokio::join!(
            fetch(format!("/alice/{a}")),
            fetch(format!("/bob/{b}")),
            fetch(format!("/alice/{b}"))
        );
        assert_eq!(foreign.status(), StatusCode::NOT_FOUND);
        for (response, expected) in [(response_a, bytes_a), (response_b, bytes_b)] {
            assert_eq!(
                response.headers()[header::CACHE_CONTROL],
                "private, no-store"
            );
            assert_eq!(response.headers()[header::CONTENT_TYPE], "audio/wav");
            assert_eq!(
                axum::body::to_bytes(response.into_body(), 4096)
                    .await
                    .unwrap()
                    .as_ref(),
                expected
            );
        }
    }

    #[test]
    fn snapshots_enforce_owner_and_retention() {
        let store = JobStore::default();
        let control = InferenceControl::new(Duration::from_secs(1));
        let now = chrono::Utc::now();
        store.entries.lock().unwrap().insert(
            "id".into(),
            Job {
                owner: "alice".into(),
                control,
                status: JobStatus {
                    id: "id".into(),
                    created_at: now,
                    updated_at: now,
                    context: None,
                    cancellation_requested: false,
                    deadline_exceeded: false,
                    stage: "failed".into(),
                    frames: 0,
                    elapsed_secs: 0.0,
                    audio_secs: None,
                    rtf: None,
                    error: Some("test failure".into()),
                    stage_secs: Default::default(),
                    playback: None,
                },
                finished: Some(Instant::now()),
                retention: Duration::ZERO,
                audio: None,
            },
        );
        assert!(matches!(
            store.snapshot("id", "bob"),
            Err(AppError::NotFound(_))
        ));
        assert_eq!(store.snapshot("id", "alice").unwrap().stage, "failed");
        store.reap_locked(&mut store.entries.lock().unwrap());
        assert!(store.snapshot("id", "alice").is_err());
    }

    fn unfinished_job(owner: &str, retention: Duration) -> (String, Job) {
        let control = InferenceControl::new(Duration::from_secs(60));
        let id = control.id.clone();
        let now = chrono::Utc::now();
        let job = Job {
            owner: owner.into(),
            control,
            status: JobStatus {
                id: id.clone(),
                created_at: now,
                updated_at: now,
                context: None,
                cancellation_requested: false,
                deadline_exceeded: false,
                stage: "queued".into(),
                frames: 0,
                elapsed_secs: 0.0,
                audio_secs: None,
                rtf: None,
                error: None,
                stage_secs: Default::default(),
                playback: None,
            },
            finished: None,
            retention,
            audio: None,
        };
        (id, job)
    }

    #[test]
    fn late_completion_cannot_replace_audio_or_publish_after_cancel() {
        let store = JobStore::default();
        let (id, job) = unfinished_job("alice", Duration::from_secs(60));
        let (cancelled, stopped) = unfinished_job("bob", Duration::from_secs(60));
        stopped.control.cancel();
        let mut entries = store.entries.lock().unwrap();
        entries.insert(id.clone(), job);
        entries.insert(cancelled.clone(), stopped);
        assert!(store.store_completed_audio_locked(
            &mut entries,
            &id,
            "audio/wav".into(),
            Bytes::from_static(b"first")
        ));
        assert!(!store.store_completed_audio_locked(
            &mut entries,
            &id,
            "audio/wav".into(),
            Bytes::from_static(b"second")
        ));
        assert!(!store.store_completed_audio_locked(
            &mut entries,
            &cancelled,
            "audio/wav".into(),
            Bytes::from_static(b"partial")
        ));
        assert_eq!(
            entries[&id].audio.as_ref().unwrap().1,
            Bytes::from_static(b"first")
        );
        assert!(entries[&cancelled].audio.is_none());
        assert_eq!(store.retained_locked(), 5);
    }

    #[test]
    fn retained_audio_is_shared_without_copying() {
        let store = JobStore::default();
        let (id, job) = unfinished_job("alice", Duration::from_secs(3600));
        store.entries.lock().unwrap().insert(id.clone(), job);
        let payload = Bytes::from(vec![7u8; 1024]);
        {
            let mut entries = store.entries.lock().unwrap();
            assert!(store.store_completed_audio_locked(
                &mut entries,
                &id,
                "audio/ogg".into(),
                payload
            ));
        }
        // Concurrent fetches share the backing store: pointer-identical.
        let first = {
            let entries = store.entries.lock().unwrap();
            entries.get(&id).unwrap().audio.as_ref().unwrap().1.clone()
        };
        let second = {
            let entries = store.entries.lock().unwrap();
            entries.get(&id).unwrap().audio.as_ref().unwrap().1.clone()
        };
        assert_eq!(first.as_ptr(), second.as_ptr());
        assert_eq!(first.len(), 1024);
        assert_eq!(store.retained_locked(), 1024);
    }

    #[test]
    fn retained_counter_tracks_store_cap_and_expiry() {
        let store = JobStore::default();
        let (a, job_a) = unfinished_job("alice", Duration::from_secs(3600));
        let (b, job_b) = unfinished_job("alice", Duration::ZERO);
        {
            let mut entries = store.entries.lock().unwrap();
            entries.insert(a.clone(), job_a);
            entries.insert(b.clone(), job_b);
            assert!(store.store_completed_audio_locked(
                &mut entries,
                &a,
                "audio/ogg".into(),
                Bytes::from(vec![0u8; 100])
            ));
            assert!(store.store_completed_audio_locked(
                &mut entries,
                &b,
                "audio/ogg".into(),
                Bytes::from(vec![0u8; 50])
            ));
        }
        assert_eq!(store.retained_locked(), 150);
        // Cap: a payload that would exceed the total is refused and retains
        // nothing (no partial accounting, no counter change).
        {
            let mut entries = store.entries.lock().unwrap();
            assert!(!store.store_completed_audio_locked(
                &mut entries,
                &a,
                "audio/ogg".into(),
                Bytes::from(vec![0u8; MAX_RETAINED_AUDIO_BYTES]),
            ));
        }
        assert_eq!(store.retained_locked(), 150);
        // Missing entries retain nothing.
        {
            let mut entries = store.entries.lock().unwrap();
            assert!(!store.store_completed_audio_locked(
                &mut entries,
                "ghost",
                "audio/ogg".into(),
                Bytes::from(vec![0u8; 8])
            ));
        }
        // Expiry subtracts exactly the reaped payloads; the counter never
        // underflows, even across repeated reaps.
        {
            let mut entries = store.entries.lock().unwrap();
            // Mark `b` finished long ago with zero retention so it expires.
            entries.get_mut(&b).unwrap().finished = Some(Instant::now());
            store.reap_locked(&mut entries);
        }
        assert_eq!(store.retained_locked(), 100);
        {
            let mut entries = store.entries.lock().unwrap();
            store.reap_locked(&mut entries);
        }
        assert_eq!(store.retained_locked(), 100);
    }

    #[test]
    fn retention_cap_boundary_is_exact() {
        let store = JobStore::default();
        let (a, job_a) = unfinished_job("alice", Duration::from_secs(3600));
        let (b, job_b) = unfinished_job("alice", Duration::from_secs(3600));
        let mut entries = store.entries.lock().unwrap();
        entries.insert(a.clone(), job_a);
        entries.insert(b.clone(), job_b);
        // Exactly the cap is accepted…
        assert!(store.store_completed_audio_locked(
            &mut entries,
            &a,
            "audio/ogg".into(),
            Bytes::from(vec![0u8; MAX_RETAINED_AUDIO_BYTES]),
        ));
        assert_eq!(store.retained_locked(), MAX_RETAINED_AUDIO_BYTES);
        // …and one byte more is refused with no accounting change. The
        // saturating check (not wrapping addition) is what makes this exact.
        assert!(!store.store_completed_audio_locked(
            &mut entries,
            &b,
            "audio/ogg".into(),
            Bytes::from(vec![0u8; 1]),
        ));
        assert_eq!(store.retained_locked(), MAX_RETAINED_AUDIO_BYTES);
    }

    // --- Idempotency and playback-aware job admission ---------------------

    /// App over the deterministic mock executor with **no** audio device, so
    /// playback admission fails honestly without hardware.
    async fn idempotency_app() -> (axum::Router, AppState, String) {
        use crate::auth::store::TokenStore;
        use axum::routing::{get, post};
        let path =
            std::env::temp_dir().join(format!("sonicboom-job-idem-{}.json", uuid::Uuid::new_v4()));
        let tokens = Arc::new(TokenStore::load(path.to_str().unwrap()).await.unwrap());
        let (_, token) = tokens.create(None).await.unwrap();
        let state =
            crate::api::tests::test_state(crate::api::tests::test_config(), tokens, Arc::new(None));
        let app = axum::Router::new()
            .route("/jobs", post(mock_create))
            .route("/jobs/{id}", get(status))
            .route("/jobs/{id}/audio", get(audio))
            .route("/jobs/{id}/cancel", post(cancel))
            .with_state(state.clone());
        let _ = std::fs::remove_file(&path);
        (app, state, token)
    }

    async fn send_job(
        app: &axum::Router,
        method: &str,
        uri: &str,
        token: &str,
        idem: Option<&str>,
        body: &str,
    ) -> Response {
        use tower::ServiceExt;
        let mut builder: axum::http::request::Builder = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "text/plain");
        if let Some(key) = idem {
            builder = builder.header("idempotency-key", key);
        }
        let request = builder.body(Body::from(body.to_string())).unwrap();
        app.clone().oneshot(request).await.unwrap()
    }

    async fn json_body(response: Response) -> serde_json::Value {
        serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 8192)
                .await
                .unwrap(),
        )
        .unwrap()
    }

    /// Polls until the synthesis terminal state is published and any requested
    /// playback has left `pending`, so assertions never race the background
    /// task.
    async fn wait_settled(app: &axum::Router, token: &str, id: &str) -> serde_json::Value {
        for _ in 0..400 {
            let value =
                json_body(send_job(app, "GET", &format!("/jobs/{id}"), token, None, "").await)
                    .await;
            let terminal = matches!(
                value["stage"].as_str(),
                Some("completed" | "failed" | "cancelled")
            );
            let playback_settled = value
                .get("playback")
                .and_then(|playback| playback.get("state"))
                .and_then(|state| state.as_str())
                .is_none_or(|state| state != "pending");
            if terminal && playback_settled {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("job {id} never settled");
    }

    fn job_count(state: &AppState) -> usize {
        state.inference_gate.jobs.entries.lock().unwrap().len()
    }

    #[tokio::test]
    async fn idempotent_retry_reuses_the_job_and_never_synthesizes_twice() {
        let (app, state, token) = idempotency_app().await;
        let uri = "/jobs?lang=es&voice=M1&format=wav";
        let first = send_job(&app, "POST", uri, &token, Some("key-1"), "hola").await;
        assert_eq!(first.status(), StatusCode::ACCEPTED);
        let first_id = json_body(first).await["id"].as_str().unwrap().to_string();
        wait_settled(&app, &token, &first_id).await;

        let retry = send_job(&app, "POST", uri, &token, Some("key-1"), "hola").await;
        assert_eq!(
            retry.status(),
            StatusCode::OK,
            "a retry is answered, not re-created"
        );
        assert_eq!(json_body(retry).await["id"].as_str().unwrap(), first_id);
        assert_eq!(job_count(&state), 1, "no second job was admitted");
    }

    #[tokio::test]
    async fn idempotency_key_reuse_with_a_different_request_conflicts() {
        let (app, state, token) = idempotency_app().await;
        let uri = "/jobs?lang=es&voice=M1&format=wav";
        let first = send_job(&app, "POST", uri, &token, Some("key-2"), "hola").await;
        assert_eq!(first.status(), StatusCode::ACCEPTED);
        let first_id = json_body(first).await["id"].as_str().unwrap().to_string();
        wait_settled(&app, &token, &first_id).await;

        // Same key, different text: rejected, never a second synthesis.
        let changed_text = send_job(&app, "POST", uri, &token, Some("key-2"), "mundo").await;
        assert_eq!(changed_text.status(), StatusCode::CONFLICT);
        // Same key, different voice: also a conflict.
        let changed_voice = send_job(
            &app,
            "POST",
            "/jobs?lang=es&voice=F1&format=wav",
            &token,
            Some("key-2"),
            "hola",
        )
        .await;
        assert_eq!(changed_voice.status(), StatusCode::CONFLICT);
        // Same key, different playback semantics. Where playback is compiled in
        // this is a conflict; a synthesis-only build rejects the mode as
        // unsupported before the key is even considered.
        let changed_playback = send_job(
            &app,
            "POST",
            "/jobs?lang=es&voice=M1&format=wav&playback=enqueue",
            &token,
            Some("key-2"),
            "hola",
        )
        .await;
        #[cfg(feature = "playback")]
        assert_eq!(changed_playback.status(), StatusCode::CONFLICT);
        #[cfg(not(feature = "playback"))]
        assert_eq!(changed_playback.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(job_count(&state), 1);
    }

    #[tokio::test]
    async fn idempotency_keys_are_scoped_per_principal() {
        use crate::auth::store::TokenStore;
        use axum::routing::{get, post};
        let path =
            std::env::temp_dir().join(format!("sonicboom-job-scope-{}.json", uuid::Uuid::new_v4()));
        let tokens = Arc::new(TokenStore::load(path.to_str().unwrap()).await.unwrap());
        let (_, alice) = tokens.create(None).await.unwrap();
        let (_, bob) = tokens.create(None).await.unwrap();
        let state =
            crate::api::tests::test_state(crate::api::tests::test_config(), tokens, Arc::new(None));
        let app = axum::Router::new()
            .route("/jobs", post(mock_create))
            .route("/jobs/{id}", get(status))
            .with_state(state.clone());
        let uri = "/jobs?lang=es&voice=M1&format=wav";
        // The same key string from two tenants yields two independent jobs.
        let a = json_body(send_job(&app, "POST", uri, &alice, Some("shared"), "hola").await).await
            ["id"]
            .as_str()
            .unwrap()
            .to_string();
        let b =
            json_body(send_job(&app, "POST", uri, &bob, Some("shared"), "hola").await).await["id"]
                .as_str()
                .unwrap()
                .to_string();
        assert_ne!(a, b, "keys never leak across principals");
        // Neither tenant can read the other's job.
        let foreign = send_job(&app, "GET", &format!("/jobs/{a}"), &bob, None, "").await;
        assert_eq!(foreign.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn playback_modes_are_validated_before_a_job_exists() {
        let (app, state, token) = idempotency_app().await;
        let unknown = send_job(&app, "POST", "/jobs?playback=loud", &token, None, "hola").await;
        assert_eq!(unknown.status(), StatusCode::BAD_REQUEST);
        let play_now_only =
            send_job(&app, "POST", "/jobs?play_now=true", &token, None, "hola").await;
        assert_eq!(play_now_only.status(), StatusCode::BAD_REQUEST);
        let empty = send_job(&app, "POST", "/jobs?playback=", &token, None, "hola").await;
        assert_eq!(
            empty.status(),
            StatusCode::ACCEPTED,
            "an empty value means synthesis-only"
        );
        assert_eq!(job_count(&state), 1, "rejected modes never create jobs");
    }

    #[tokio::test]
    async fn synthesis_only_jobs_omit_the_playback_block() {
        let (app, _state, token) = idempotency_app().await;
        let created = send_job(
            &app,
            "POST",
            "/jobs?lang=es&voice=M1&format=wav",
            &token,
            None,
            "hola",
        )
        .await;
        let id = json_body(created).await["id"].as_str().unwrap().to_string();
        let status = wait_settled(&app, &token, &id).await;
        assert_eq!(status["stage"], "completed");
        assert!(
            status.get("playback").is_none(),
            "the legacy contract carries no playback block"
        );
    }

    #[cfg(feature = "playback")]
    #[tokio::test]
    async fn playback_failure_never_fails_synthesis_or_withholds_audio() {
        let (app, _state, token) = idempotency_app().await;
        let created = send_job(
            &app,
            "POST",
            "/jobs?lang=es&voice=M1&format=wav&playback=enqueue",
            &token,
            None,
            "hola",
        )
        .await;
        assert_eq!(created.status(), StatusCode::ACCEPTED);
        let id = json_body(created).await["id"].as_str().unwrap().to_string();
        let status = wait_settled(&app, &token, &id).await;
        assert_eq!(status["stage"], "completed", "synthesis still succeeds");
        assert_eq!(
            status["playback"]["state"], "failed",
            "no device must be reported honestly, never as heard"
        );
        assert_eq!(status["playback"]["play_now"], false);
        assert!(
            status["playback"]["error"]
                .as_str()
                .unwrap()
                .contains("Audio playback")
        );
        // Retained audio is still downloadable: a playback failure is not a
        // synthesis failure.
        let audio = send_job(&app, "GET", &format!("/jobs/{id}/audio"), &token, None, "").await;
        assert_eq!(audio.status(), StatusCode::OK);
    }

    #[cfg(feature = "playback")]
    #[tokio::test]
    async fn cancelled_playback_job_is_reported_skipped_not_enqueued() {
        let (app, _state, token) = idempotency_app().await;
        // "wait" blocks inside the mock until the control is stopped.
        let created = send_job(
            &app,
            "POST",
            "/jobs?lang=es&voice=M1&format=wav&playback=enqueue",
            &token,
            None,
            "wait",
        )
        .await;
        let id = json_body(created).await["id"].as_str().unwrap().to_string();
        let cancelled = send_job(
            &app,
            "POST",
            &format!("/jobs/{id}/cancel"),
            &token,
            None,
            "",
        )
        .await;
        assert_eq!(cancelled.status(), StatusCode::OK);
        let status = wait_settled(&app, &token, &id).await;
        assert_eq!(status["stage"], "cancelled");
        assert_eq!(
            status["playback"]["state"], "skipped",
            "a cancelled job is never enqueued for playback"
        );
    }
}
