# SonicBoom API Reference

Complete API documentation for SonicBoom TTS server.

## Table of Contents

- [Original TTS API](#original-tts-api)
- [Text normalization](#text-normalization)
- [Audio Queue API](#audio-queue-api)
- [Audio Output Device API](#audio-output-device-api)
- [Web Routes](#web-routes)
- [Authentication](#authentication)
- [Response Formats](#response-formats)
- [Error Codes](#error-codes)

---

## Original TTS API

### Generate TTS Audio

Synthesizes text to speech audio.

**Endpoint:** `POST /api/tts`

**Authentication:** Bearer token required

**Query Parameters:**

| Parameter | Type   | Required | Default | Description                            |
| --------- | ------ | -------- | ------- | -------------------------------------- |
| `voice`   | string | No       | `M1`    | Voice style (M1-M5, F1-F5)             |
| `lang`    | string | No       | `en`    | Language code                          |
| `format`  | string | No       | `opus`  | Output format: `opus`, `wav`, `mp3`, or `flac` (unknown → `400`) |
| `normalization` | string | No | server default (`auto`) | Text normalization: `auto` or `off` (case-insensitive; see [Text normalization](#text-normalization)) |
| `locale`  | string | No       | language default | Pronunciation locale such as `es-PE`; `auto` selects the language default |

**Request Body:** Plain text string (max `MAX_TEXT_LENGTH` Unicode chars,
body capped at `TTS_MAX_BODY_BYTES`)

> The built-in web UI (`/`) requires you to paste an API token, which it
> sends as `Authorization: Bearer <token>`. The token stays in the page's
> memory only (never `localStorage`, never logged).

**Response:** Audio data (format based on `format` parameter)

**Example:**

```bash
# Get WAV output
curl -X POST "http://localhost:17842/api/tts?voice=F1&format=wav" \
  -H "Authorization: Bearer YOUR_TOKEN" \
  -d "Hello, world!" \
  --output audio.wav
```

---

### Synthesize and Play

Synthesizes text and adds it directly to the server's playback queue in a single request.

**Endpoint:** `POST /api/tts/play`

**Authentication:** Bearer token required

**Query Parameters:**

| Parameter  | Type    | Required | Default | Description                            |
| ---------- | ------- | -------- | ------- | -------------------------------------- |
| `provider` | string | No | `supertonic` | `supertonic`, `pocket-tts`, or `qwen3-gguf`; empty preserves legacy routing |
| `voice` | string | For cloning | `M1` (Supertonic) | Built-in style or token-owned clone profile UUID; matching `provider:voice` option IDs accepted |
| `lang`     | string  | No       | `en`    | Language code                          |
| `play_now` | boolean | No       | `false` | If `true`, clears queue and plays now  |
| `normalization` | string | No | server default | `auto` or `off` |
| `locale`   | string  | No       | language default | Pronunciation locale (independent of the profile's base language) |

**Request Body:** Plain text string

**Response (JSON):**

```json
{
  "success": true,
  "message": "Added to queue",
  "id": "generated-uuid"
}
```

**Example:**

```bash
curl -X POST "http://localhost:17842/api/tts/play?voice=F1&play_now=true" \
  -H "Authorization: Bearer YOUR_TOKEN" \
  -d "Synthesize and play this immediately on the server speakers."
```

Cloning playback uses the profile's stored model, revision, reference audio and clone
mode through the existing bounded inference gate. `lang` defaults to the profile
language when omitted; Pocket requires its language-specific profile language.
Qwen supports the languages advertised by its provider. No provider fallback occurs.
Unknown/mismatched providers or voices and incompatible revisions return `400`;
foreign/deleted profiles return `404`; unavailable providers return `503`. Existing
rate limits (`429`), deadlines, secure temporary WAV ownership and queue semantics
apply to every provider. Omitted/empty `provider` retains legacy Supertonic routing.

### Plugin catalogs

Authenticated `GET /api/tts/plugin/providers` returns a root array of enabled
providers: `{id,name,state,disabled}`. Enabled-but-loading/error/not-installed
providers remain visible with `disabled: true`; only `ready` providers are selectable.
Disabled configuration entries are omitted. Count enabled rows to decide selector
visibility, including enabled rows that are not ready. Provider labels are human
readable and availability comes from the live resident registry.

Authenticated `GET /api/tts/plugin/voices` returns a root array of
`{id,name,disabled:false}` options, with IDs such as `supertonic:M1` or
`pocket-tts:<uuid>`. Only ready built-in styles and the requesting token's compatible
clone profiles are returned; models, revisions, language and clone mode are checked.
Only the currently resident model is usable (Pocket language models are separate;
select the matching model in server configuration). The array is sorted by qualified ID, limited to 500 items, with profile names limited
to 128 characters. Both catalogs send `Cache-Control: private, no-store`. Refreshing
these routes re-reads live state/profile manifests; it does not reload resident models.
Clients must strictly filter voices by provider and must not submit an empty clone
voice. The playback route validates qualified IDs against the authoritative provider.

Example: `POST /api/tts/play?provider=pocket-tts&voice=pocket-tts%3A<uuid>&lang=en&play_now=false`.

---

## Text normalization

Before synthesis, SonicBoom turns written forms (numbers, dates, times,
prices, percentages, units, abbreviations) into speakable text with an
offline, rule-based normalizer. It runs **once per request**, before
chunking and before any provider-specific preprocessing, on every
synthesis path: `/api/tts`, `/api/tts/play`, `/api/tts/jobs`,
`/api/voices/{id}/preview`, `/v1/audio/speech`, the reusable library engine
and the TikTools plugin. Ambiguous input (for example `01/02/2026` without
an explicit locale) is preserved and only reported. Only the target text is
prepared: a Qwen voice profile's reference transcript and every reference
audio are passed to the provider unchanged. Coverage per language is listed
in [normalization.md](normalization.md) and in `GET /api/info`.

> **Behavior change.** The default mode is `auto`. Requests that send no
> normalization parameters now hear numbers, dates, prices and similar
> forms spoken as words (for example `26/10/2026` in Spanish becomes
> "veintiséis de octubre de dos mil veintiséis"). Send `normalization=off`
> (or `"normalization": "off"` in JSON) to get the previous audio, or set
> `[tts.normalization] mode = "off"` / `enabled = false` server-wide.

Request options (query parameters on `POST /api/tts`, `POST /api/tts/play`,
`POST /api/tts/jobs` and `POST /api/voices/{id}/preview`; JSON fields on
`POST /v1/audio/speech`):

| Option | Values | Absent / empty |
| ------ | ------ | -------------- |
| `normalization` | `auto` or `off` (case-insensitive) | `[tts.normalization]` effective mode (`auto`; `off` when `enabled = false`) |
| `locale` | a supported tag (`es-ES`, `es-MX`, `es-PE`, `en-US`, `en-GB`, `pt-BR`, `pt-PT`, `fr-FR`, `de-DE`, `it-IT`, `ru-RU`, `zh-CN`, `zh-TW`, `ja-JP`, `ko-KR`) or `auto` | `[tts.normalization] default_locale` when it belongs to the speech language, otherwise the language default |

- `off` hands the provider exactly the text it received before this
  feature (each route's existing trimming still applies).
- `locale=auto` always selects the speech language's documented default
  (`es`→`es-ES`, `en`→`en-US`, `pt`→`pt-BR`, ...), never the server location.
- The speech language is the route's existing language (`lang`, the
  profile language, or `en` for Supertonic aliases on `/v1/audio/speech`).
  The locale must belong to it.
- A speech language without rules, or a legacy tag such as `en-US` passed as
  the language, passes through unchanged.

Errors (all use the standard JSON error envelope and never echo the text):

| Condition | Status |
| --------- | ------ |
| `normalization` other than `auto`/`off` | `400` |
| Malformed locale, unsupported locale (e.g. `es-AR`), or locale of another language | `400` |
| Text over `max_text_length`, or spoken text over `max_normalized_chars` / `max_expansion_ratio` | `400` |
| Missing or invalid bearer token | `401` |

### Preview normalization

**Endpoint:** `POST /api/tts/normalize`

**Authentication:** Bearer token required. Rate-limited like the TTS
routes; body capped at `TTS_MAX_BODY_BYTES`. No model inference, no file
access, no job is created. Responses (success and errors) carry
`Cache-Control: private, no-store`. The server never logs `original` or
`spoken`.

**Request Body (JSON, unknown fields rejected):**

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `text` | string | Yes | Trimmed; non-empty; at most `max_text_length` characters |
| `language` | string | Yes | Speech language (base code such as `es`) |
| `locale` | string | No | Supported tag or `auto` (default: as for synthesis) |
| `mode` | string | No | `auto` or `off` (default: server default) |
| `model` | string | No | When present, `language` must be valid for that model (e.g. `pocket-tts-es` accepts only `es`) |

```bash
curl -X POST "http://localhost:17842/api/tts/normalize"   -H "Authorization: Bearer YOUR_TOKEN"   -H "Content-Type: application/json"   -d '{"text":"La reunión es el 26/10/2026.","model":"pocket-tts-es","language":"es","locale":"es-PE","mode":"auto"}'
```

**Response (`200`):**

```json
{
  "original": "La reunión es el 26/10/2026.",
  "spoken": "La reunión es el veintiséis de octubre de dos mil veintiséis.",
  "language": "es",
  "locale": "es-PE",
  "mode": "auto",
  "normalizer_version": "tn-v1",
  "changes": [
    { "kind": "date", "before": "26/10/2026", "after": "veintiséis de octubre de dos mil veintiséis",
      "start": 18, "end": 28, "confidence": "high" }
  ],
  "warnings": [],
  "coverage": { "cardinal_numbers": "full", "dates": "partial", "...": "..." }
}
```

(Coverage values above are illustrative; the response lists all eleven
feature keys with the levels documented in [normalization.md](normalization.md).)

`start`/`end` are UTF-8 byte offsets into `original` (`before` is
`original[start..end]`). `warnings` entries have `{code, before, start,
end}` with codes `ambiguous_number`, `ambiguous_date`, `invalid_date`,
`ambiguous_currency`, `ambiguous_time`, `ambiguous_fraction`,
`number_too_large`; they are empty when `report_ambiguities = false`.
`coverage` maps each feature key to `full`, `partial` or `none` for the
language. Errors: `400` for empty/too long text, invalid language, locale,
mode or model/language mismatch, malformed JSON; `401` without a valid
bearer token; `429` when rate-limited.

`GET /api/info` adds a `normalization` block:
`{ "ruleset_version": "tn-v1", "default_mode": "auto", "languages": [{ "language", "locales", "default_locale", "rules", "features" }] }`.
Speech-language support is still reported by provider capabilities
(`/api/providers`); normalization coverage is separate.

---

### Check Model Status

Check if the model is loaded and ready.

**Endpoint:** `GET /api/status`

**Authentication:** None (public metadata endpoint exposing only
non-sensitive load status; see [OpenAI docs](openai.md#public-metadata-endpoints))

**Response:** JSON object with model status

**Response Fields:**

| Field      | Type   | Description                                                          |
| ---------- | ------ | -------------------------------------------------------------------- |
| `status`   | string | Model status: `idle`, `downloading`, `loading`, `ready`, or `failed` |
| `progress` | float  | Download progress percentage (only when downloading)                 |
| `error`    | string | Error message (only when failed)                                     |

**Example:**

```bash
curl http://localhost:17842/api/status
```

**Example Response:**

```json
{
  "status": "ready",
  "progress": null,
  "error": null
}
```

---

## Audio Queue API

The Audio Queue API allows you to play audio files directly on the server's audio output (e.g., speakers) and manage a playback queue.

### Queue Audio File

Adds an audio file to the playback queue.

**Endpoint:** `POST /api/queue`

**Authentication:** Bearer token required

**Request Body (JSON):**

| Field      | Type    | Required | Description                                                |
| ---------- | ------- | -------- | ---------------------------------------------------------- |
| `path`     | string  | Yes      | Path inside `ALLOWED_AUDIO_DIR` (relative or absolute)     |
| `id`       | string  | No       | Unique identifier 1–128 chars, no control chars (generated if missing) |
| `play_now` | boolean | No       | If `true`, clears the queue and starts playing immediately |

Queue files must live inside the server's `ALLOWED_AUDIO_DIR` (canonicalized;
`..` escapes, symlink escapes, directories, missing files, and non-audio
extensions `.wav/.mp3/.flac/.ogg/.opus` are rejected). All queue endpoints
require bearer authentication. Error responses use `success: false` with an
appropriate status (`400`/`401`/`403`/`429`/`503`) and never echo server
paths. A full playback queue (`MAX_PLAYBACK_QUEUE_ITEMS`) returns `429`;
a dead audio thread returns `503`.

**Example:**

```bash
curl -X POST http://localhost:17842/api/queue \
  -H "Authorization: Bearer YOUR_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "path": "./audio/welcome.wav",
    "play_now": false
  }'
```

---

### Control Playback

Control the current playback state.

| Endpoint            | Method | Description                              |
| ------------------- | ------ | ---------------------------------------- |
| `/api/queue/next`   | `POST` | Skip to the next item in the queue       |
| `/api/queue/pause`  | `POST` | Pause current playback                   |
| `/api/queue/resume` | `POST` | Resume paused playback                   |
| `/api/queue/stop`   | `POST` | Stop playback and clear the entire queue |

**Response (JSON):**

```json
{
  "success": true,
  "message": "Playback paused",
  "id": null
}
```

---

### Set Volume

Adjust the master volume for audio playback.

**Endpoint:** `POST /api/queue/volume`

**Request Body (JSON):**

| Field    | Type  | Required | Description                                   |
| -------- | ----- | -------- | --------------------------------------------- |
| `volume` | float | Yes      | Volume level from `0.0` (mute) to `1.0` (max); out-of-range or non-finite values → `400` (never silently clamped) |

**Example:**

```bash
curl -X POST http://localhost:17842/api/queue/volume \
  -H "Authorization: Bearer YOUR_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"volume": 0.5}'
```

---

### Get Queue Status

Retrieve information about current playback and the queue.

**Endpoint:** `GET /api/queue/status`

**Response Fields:**

| Field          | Type    | Description                          |
| -------------- | ------- | ------------------------------------ |
| `current`      | object  | Currently playing item (or `null`)   |
| `queue_length` | integer | Number of items waiting in the queue |
| `is_playing`   | boolean | Whether audio is currently playing   |
| `is_paused`    | boolean | Whether audio is currently paused    |
| `volume`       | float   | Current volume level                 |

**Example Response:**

```json
{
  "current": {
    "id": "item-123",
    "path": "/path/to/audio/file.wav"
  },
  "queue_length": 2,
  "is_playing": true,
  "is_paused": false,
  "volume": 0.5
}
```

---

## Audio Output Device API

Playback builds can discover, report, and switch the server's audio
output device at runtime — e.g. speakers, headphones, HDMI, VB-Audio
cables, or loopback devices. HTTP clients cannot enumerate the host's
devices themselves, so SonicBoom owns discovery. All three endpoints
require Bearer [REDACTED] (device names are host metadata) and exist only
when the `playback` feature is enabled (otherwise `404`). No paths or
filesystem data are ever returned.

### List Output Devices

**Endpoint:** `GET /api/audio/devices`

Output devices only (never input-only), with the logical `default` entry
always first. A backend enumeration failure returns `500`, never an
empty success.

**Example Response:**

```json
{
  "devices": [
    {
      "id": "default",
      "name": "System Default",
      "is_default": true,
      "is_selected": false
    },
    {
      "id": "Speakers (Realtek(R) Audio)",
      "name": "Speakers (Realtek(R) Audio)",
      "is_default": false,
      "is_selected": true
    }
  ],
  "selected": "Speakers (Realtek(R) Audio)"
}
```

Device ids are the OS-reported names (backend/OS dependent — CPAL
exposes no stable identifiers). Duplicate names gain ` #2`, ` #3`, ...
suffixes on `id`. `selected` is the configured selection, which may name
a device that is currently missing (it then matches no entry).

### Get Active Output Device

**Endpoint:** `GET /api/audio/output`

Distinguishes the configured selection from the live hardware state, so
a temporarily missing device is visible instead of silently substituted.

**Example Response:**

```json
{
  "device": "CABLE Input (VB-Audio Virtual Cable)",
  "resolved_name": "CABLE Input (VB-Audio Virtual Cable)",
  "available": true
}
```

While the selection is unavailable (missing device, no stream yet),
`available` is `false` and `resolved_name` is `null` — the `device`
selection itself is retained.

### Set Output Device

**Endpoint:** `POST /api/audio/output`

**Request Body (JSON):** `{ "device": "<id or \"default\">" }`

`{"device": "default"}` returns to OS-default behavior. Unknown devices
return `400` with no fallback to another device.

**Example:**

```bash
curl -X POST http://localhost:17842/api/audio/output \
  -H "Authorization: Bearer [REDACTED]" \
  -H "Content-Type: application/json" \
  -d '{"device": "CABLE Input (VB-Audio Virtual Cable)"}'
```

**Example Response:**

```json
{
  "success": true,
  "device": "CABLE Input (VB-Audio Virtual Cable)",
  "resolved_name": "CABLE Input (VB-Audio Virtual Cable)"
}
```

**Switching behavior:** the target is validated against live enumeration
and the new stream is opened before anything is committed — a failed
switch leaves the previous working output active. On success the current
item stops (its temp file is cleaned) and queued items continue on the
new device; the waiting queue is never cleared by a switch. Switching
while paused keeps the queue for `Resume`.

**Disconnect behavior:** a selected device that disappears does not
crash the playback thread and is never auto-switched to another output.
The selection is retained, pending items stay queued, and playback
resumes once the device reappears (immediate retry on commands, throttled
background retry otherwise).

**Persistence:** runtime selection is process-local and does not survive
restarts (SonicBoom never rewrites `.env`). Set `AUDIO_OUTPUT_DEVICE`
for the startup selection.

---

## Web Routes

### Health Check

Liveness endpoint for container `HEALTHCHECK` and monitoring.

**Endpoint:** `GET /health`

**Authentication:** None

**Response:** Plain text "OK" with status 200 (process alive)

**Example:**

```bash
curl http://localhost:17842/health
```

### Readiness Check

Readiness endpoint for load balancers and orchestrators.

**Endpoint:** `GET /ready`

**Authentication:** None

**Response:** `200 ready` when the model is loaded, otherwise `503`
(downloading/loading/idle/failed — no internal details exposed)

**Example:**

```bash
curl http://localhost:17842/ready
```

---

## Authentication

SonicBoom uses Bearer token authentication. Include your API token in the `Authorization` header:

```bash
-H "Authorization: Bearer YOUR_API_TOKEN"
```

The syntax is strict: only `Bearer <token>` is accepted (scheme name is
case-insensitive). Raw tokens without the scheme, `Basic`, `Token`, empty
bearer values, and extra whitespace are all rejected with `401`.

### Optional Authentication

For development or public APIs, you can disable authentication:

```bash
export SONICBOOM_AUTH_REQUIRED=0
```

When disabled, API requests work without any token.

### Getting a Token

1. Access the admin panel at `/admin`
2. Login with admin credentials
3. Navigate to Tokens section
4. Create a new token

### Sample Token

For local development only, you can enable a sample token (the server
logs a prominent warning while it is enabled; never use in production):

```bash
export ENABLE_SAMPLE_TOKEN=1
```

Then use `SAMPLE_TOKEN` for testing.

---

## Response Formats

### Supported Formats

SonicBoom supports multiple audio output formats:

| Format | Content-Type | Description                              |
| ------ | ------------ | ---------------------------------------- |
| `opus` | `audio/ogg; codecs=opus` | Default Opus/OGG format (recommended) |
| `wav`  | `audio/wav`  | WAV format (PCM 16-bit)                  |
| `mp3`  | `audio/mpeg` | MP3 format                               |
| `flac` | `audio/flac` | FLAC format                              |

Unknown formats return `400`; each response is one fully encoded audio
buffer (not an incremental stream).

### Using Format Parameter

**Original API:**

```bash
# Get WAV output
curl -X POST "http://localhost:17842/api/tts?format=wav" \
  -H "Authorization: Bearer YOUR_TOKEN" \
  -d "Hello, world!" \
  --output audio.wav
```

**OpenAI API:**

```bash
curl -X POST http://localhost:17842/v1/audio/speech \
  -H "Authorization: Bearer YOUR_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"input": "Hello, world!", "voice": "alloy", "response_format": "wav"}' \
  --output audio.wav
```

### Success Response

- **Content-Type:** `audio/ogg; codecs=opus`, `audio/wav`, `audio/mpeg`, or `audio/flac` (based on format)
- **Body:** Raw audio data

### Error Response

- **Content-Type:** `application/json`
- **Body:** Stable error object (never paths, secrets, or internals)

```json
{
  "error": "service_unavailable",
  "message": "Model is downloading (50% complete).",
  "status": 503,
  "request_id": "01234567-89ab-cdef-0123-456789abcdef"
}
```

---

## Error Codes

| Status Code | Description                                         |
| ----------- | --------------------------------------------------- |
| `200`       | Success                                             |
| `400`       | Bad request (invalid input, including invalid `normalization`/`locale` or text that exceeds the normalization budgets) |
| `401`       | Unauthorized (invalid/missing token)                |
| `403`       | Forbidden (queue path outside allowed directory)    |
| `404`       | Not found (unknown voice profile IDs report `404` without disclosing ownership) |
| `413`       | Payload too large (body limit exceeded)             |
| `415`       | Unsupported media type (non-WAV reference uploads)  |
| `422`       | Unprocessable entity                                |
| `429`       | Too many requests (rate limit / inference saturated / playback queue full)|
| `500`       | Internal server error (generic message + `request_id`) |
| `503`       | Service unavailable (model loading)                 |

Error responses are stable JSON (`error`, `message`, `status`,
`request_id`) and never include filesystem paths, model internals, or
secrets. Use `request_id` to correlate with server logs.

---

## Rate Limiting

Expensive TTS endpoints (`POST /api/tts`, `POST /api/tts/play`,
`POST /v1/audio/speech`) are rate-limited per API token with a
burst-friendly token bucket: `TTS_RATE_LIMIT_REQUESTS` per
`TTS_RATE_LIMIT_WINDOW_SECS` is the sustained refill rate (default
300/min) and `TTS_RATE_LIMIT_BURST` is the bucket capacity for event
bursts (defaults to the request budget). Set
`TTS_RATE_LIMIT_REQUESTS=0` to disable limiting (not recommended).

Exceeded budgets return `429` with live retry metadata computed from the
actual bucket state:

```http
Retry-After: 12
X-RateLimit-Limit: 300
X-RateLimit-Remaining: 0
X-RateLimit-Reset: 48
```

`Retry-After` is seconds until one request is affordable;
`X-RateLimit-Reset` is seconds until the bucket refills completely. The
JSON body keeps the stable `AppError` shape (`error:
"too_many_requests"`).

SonicBoom returns `429` from three independent protections — never merge
them when debugging:

| Source | Message | Headers |
| ------ | ------- | ------- |
| Token bucket (`RateLimiter`) | `Rate limit exceeded. Try again later.` | `Retry-After` + `X-RateLimit-*` |
| Inference admission (`InferenceGate`) | `Server is busy. Too many pending inference requests.` | none |
| Playback queue bound (`AudioQueue`) | `Playback queue is full. Try again later.` | none |

Inference itself is concurrency-bounded (`MAX_CONCURRENT_INFERENCE`,
`MAX_PENDING_INFERENCE`); saturated requests are rejected with `429`
instead of queueing unboundedly.

The admin panel includes login attempt tracking to prevent brute-force attacks.

- Maximum 5 failed attempts per IP in 10 minutes
- 15-minute lockout after threshold exceeded (expires automatically)

## Reliable native synthesis jobs

Use the authenticated [background job API](inference-jobs.md) for long CPU
inference, with progress and cooperative cancellation. The synchronous routes
remain supported. See [ownership/cache architecture](inference-architecture.md)
for model residency, integrity verification, reloads and cache bounds.

### Resuming synthesis jobs

A job ID identifies the original immutable submission, including its text and
voice/profile selection. Fetching an old job does not submit edited input. The
studio therefore exposes a separate resume action and displays the prior input;
finish or cancel that job before submitting another request. SSE reconnection
continues monitoring the same ID. Cancellation is a request until a terminal
server status confirms that computation stopped. Completed job audio is private
and served with `Cache-Control: private, no-store`.

The offline quality runner's `job_id: null` denotes a direct-provider call;
its request UUID must not be mistaken for an HTTP synthesis job ID. Detailed
benchmark manifests are opt-in and hash text/reference identity; status endpoints
do not acquire raw speech or transcripts as diagnostic fields.

Voice creation responses also include additive `reference_health` and `warnings`
fields. Health reports mono-mix duration, source channels/rate, peak, RMS, DC,
near-full-scale fraction (absolute amplitude ≥0.999) and fraction below amplitude 1e-4 **before resampling**.
The latter is a low-amplitude proxy, not speech/VAD coverage. Warning codes are
`reference_nearly_silent`, `reference_heavily_clipped`, and
`reference_large_dc_offset`. They are conservative advisories and do not reject,
trim, normalize or modify an otherwise valid reference. Health data is returned
to the authorized creator, not written into default telemetry or job status.

Qwen/Pocket job `context.seed`, when present, is the effective request seed as a
decimal **string**, so browser clients preserve all 64 bits. It is read-only
metadata, not a new HTTP generation parameter. Reproduce it through the native
`ProviderRequest.seed` API or offline runner `AUDIT_SEED=<decimal-u64>`. Omitted
seeds are selected once per request and sampling state remains request-local.

### Voice cloning multipart migration

The studio sends `POST /api/voices/clones` as multipart form data. Include exactly
one `metadata` JSON part (`name`, `provider`, `model`, `language`, `mode`,
optional `transcript`, `consent_basis`, optional `consent_note`, optional
`expected_revision`) and one `file`
part containing WAV bytes. Let the HTTP client generate the multipart boundary.
Transcript and consent belong in this body, never in a URL.

The raw-WAV/query protocol remains supported for existing clients, but is
deprecated for new integrations. No removal date is set. Migrate sensitive
metadata to multipart before legacy query semantics are retired.

Authenticated `GET /api/voices/constraints` returns version 1 constraints,
including the current upload cap and registered provider modes/models/languages.
Its `revision` may be sent as `expected_revision`; a changed configuration
returns 409 so clients can refresh and revalidate their retained draft. Creation
also returns the observed revision. This is optimistic conflict detection,
not a transaction spanning configuration and profile storage.
The upload byte cap applies immediately to each request; the clone route does
not capture a stale router-level body limit. The complete multipart envelope is
bounded to the current WAV cap plus 64 KiB, with a 60,000-byte metadata part cap.
The WAV itself must still fit `voices.max_upload_bytes`. Duplicate, missing and
unexpected parts are rejected. Rust remains authoritative.

List and create responses contain public summary metadata and no transcript,
consent declaration, owner hash or reference hash. Creation additionally returns
the existing reference health, warning, defect and guidance report. The explicit
owner-authorized detail endpoint includes the transcript for editing, but omits
internal owner and reference hashes.

```sh
curl -H "Authorization: Bearer YOUR_TOKEN" \
  -F 'metadata={"name":"Reference","provider":"pocket-tts","model":"pocket-tts-en","language":"en","mode":"audio_prompt","consent_basis":"owner"};type=application/json' \
  -F 'file=@reference.wav;type=audio/wav' \
  http://127.0.0.1:17842/api/voices/clones
```

For ICL, include the exact reference transcript inside the metadata JSON body.
Do not log that body or embed it in command history when it contains private
material; applications should construct the multipart body in memory.
