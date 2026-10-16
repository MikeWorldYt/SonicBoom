# Inference jobs, cancellation and ownership

Long native inference should use the background job API. Existing synchronous
`POST /api/tts`, `POST /api/voices/{id}/preview` and `POST /v1/audio/speech`
remain supported. They still have `server.request_timeout_secs` (default 120).
When an HTTP inference future disappears, cooperative work is cancelled and
its resource permits stay with the actual computation until it ends.
Aborting a `spawn_blocking` handle does not stop a native kernel.

## HTTP contract

Every job operation uses the existing authentication policy and enforces the
requesting principal's ownership. Tokens are represented in job metadata only
by their existing non-secret principal hash. Foreign, expired and unknown IDs
are indistinguishable (`404`). Security middleware supplies `Cache-Control:
no-store`; completed audio additionally uses `private, no-store`.

| Method and path | Behavior |
| --- | --- |
| `POST /api/tts/jobs?voice=F1&lang=en&format=wav` | `text/plain`; regular Supertonic synthesis; returns `202` with job ID/status |
| `POST /api/tts/jobs?profile={owned-profile-id}&lang=en&format=wav` | `text/plain`; uses the authorized profile's provider, model, mode and reference |
| `GET /api/tts/jobs/{id}` | Current status, timestamps, measured frame count, elapsed time, stage durations and actual execution context |
| `GET /api/tts/jobs/{id}/events` | SSE `progress` snapshots; each connection receives current state, including terminal state |
| `POST /api/tts/jobs/{id}/cancel` | Idempotent `200`; acknowledges cancellation, without claiming computation already stopped |
| `GET /api/tts/jobs/{id}/audio` | Completed audio only; `503` while unavailable or after failure/cancellation |

The format defaults to Opus as on synchronous routes. Cloning requires a stored,
owned profile; arbitrary server-side recording filenames are not accepted.
The creation request charges the existing rate limiter once and reserves
bounded inference admission before accepting the job. Reference preparation
also has a process-wide bound (16 concurrent reference operations), with permits
retained by blocking preparation and inference if the HTTP future disappears.

Errors: `400` invalid text/options/reference, `401` missing/invalid credentials,
`404` unknown/foreign/expired job or voice, `413` excessive upload/output,
`429` rate/admission/retention/preparation capacity, `503` model unavailable,
busy profile storage or max-frame termination, and sanitized `500` for other
inference failures. Accepted requests can subsequently fail; a `202` is not an
audio-success response. Inspect terminal status before fetching audio.

```sh
# API_TOKEN is your existing API credential; never put it in the URL.
curl -H "Authorization: Bearer $API_TOKEN" -H 'Content-Type: text/plain' \
  --data 'Hello, this is a test.' \
  'http://127.0.0.1:17842/api/tts/jobs?voice=F1&format=wav'
# Use the returned ID for status/events/cancel/audio, with the same credential.
```

## Lifecycle and deadlines

Jobs begin `queued`, progress through observable `preparing`, `generating`,
`decoding` and `encoding` stages, and end `completed`, `failed` or `cancelled`.
Backend stages without an available hook remain within the enclosing measured
native stage: Pocket TTS reports sentence-level generation, including its
Mimi decoding, rather than inventing fine-grained progress. Frame counts refer to
Qwen codec frames; Supertonic denoising iterations are not labeled frames.

`cancellation_requested` and `deadline_exceeded` describe stop requests.
The terminal `cancelled` state is published only after execution has actually
returned. Cancelling completed/failed/cancelled jobs preserves terminal state.
No successful audio can be retrieved from failed or cancelled jobs.

```toml
[inference]
max_concurrent = 1
max_pending = 8
generation_deadline_secs = 1800
job_retention_secs = 600
max_retained_jobs = 64

[voices]
max_profiles = 256
```

Deadlines include queue/preparation time for background jobs, independently of
the short initial HTTP request. Deadlines and retention must be 1..=86400 seconds;
retained jobs 1..=1024; stored profiles 1..=4096. Existing configurations acquire
these defaults when fields are omitted. Changes affect new jobs; existing jobs
keep their issued deadline/retention. Audio storage is additionally bounded to
16 MiB per output and 64 MiB process-wide. The profile manifest has a 64 MiB
bound; oversized WAV headers are rejected before sample allocation.

Jobs and their outputs are process-local. Restart/shutdown/retention expiry
makes old IDs unavailable (`404`); jobs do not resume after restart. Shutdown
requests cancellation of active jobs. Graceful shutdown and resource ownership
wait for native work to return. Native kernels cannot be forcibly interrupted;
Qwen checks between frames, acoustic predictions and preparation layers;
Supertonic checks between denoising steps/chunks; Pocket TTS checks between
autoregressive frames and sentences. A blocked native kernel must
be diagnosed through elapsed state and worker warnings, never killed unsafely.

## Progress and streaming

SSE carries latest snapshots once per second, with bounded buffering and no
retained per-frame event log. Reconnection receives current state, so missed
updates do not prevent observing completion. No percentage, remaining-time
estimate or queue position is fabricated. The browser reconnects, reports
terminal errors, restores controls, preserves Cancel/Resume monitoring after
connection failures, and fetches playable/downloadable audio
only after completion.

This is progress streaming, not incremental cloned audio streaming. The pinned
Qwen streaming sessions do not expose a safe reference-cloning API equivalent
to full ICL decoding with reference context. Audio remains a completed artifact.

Completed status includes measured `audio_secs` and `rtf` when available.
Job RTF uses total elapsed wall time, including queue and preparation, divided
by output duration; benchmark generation-only timings are reported separately.
