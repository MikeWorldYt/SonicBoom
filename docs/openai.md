# OpenAI-Compatible API

SonicBoom provides OpenAI TTS API-compatible endpoints, allowing you to use it as a drop-in replacement for OpenAI's TTS service.

## Overview

The OpenAI-compatible API follows the same request/response format as OpenAI's `/v1/audio/speech` endpoint. This allows existing applications using OpenAI TTS to switch to SonicBoom with minimal code changes.

## Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/v1/audio/speech` | Generate TTS audio |
| `GET` | `/v1/models` | List available models |
| `GET` | `/v1/models/list` | List available models (alias) |
| `GET` | `/v1/voices` | List available voices |

---

## Generate Speech

### POST /v1/audio/speech

Synthesizes text to speech using OpenAI-compatible request format.

**Authentication:** Bearer token required

**Request Body:**

```json
{
  "model": "tts-1",
  "input": "Hello, world!",
  "voice": "alloy",
  "response_format": "opus",
  "speed": 1.0
}
```

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `model` | string | No | `tts-1` | Model ID: `tts-1`, `tts-1-hd`, `supertonic-3` (Supertonic), `qwen3-tts-1.7b-base` (Qwen3-TTS), `pocket-tts-en`, `pocket-tts-es` (Pocket TTS). Anything else → `400` (retired GPT-SoVITS IDs → explicit retired-model `400`) |
| `input` | string | Yes | - | Text to synthesize |
| `voice` | string | No | `alloy` | Supertonic voice name/alias, or (for cloning models) an authorized voice-profile ID from `POST /api/voices/clones` (unknown → `400`, never a silent fallback) |
| `response_format` | string | No | `opus` | Output format: `opus`, `mp3`, `wav`, `flac` (unknown → `400`) |
| `speed` | number | No | `1.0` | Only `1.0` (or omitted) is accepted; anything else → `400` (speed adjustment is not implemented) |
| `language` | string | No | profile language (`en` for legacy) | Synthesis language for cloning models; validated per provider (Pocket TTS supports `en`/`es`; anything else → `400`). Legacy Supertonic requests always synthesize English |
| `seed` | integer | No | - | Deterministic seed for cloning providers, when supported |
| `normalization` | string | No | server default (`auto`) | SonicBoom extension: text normalization `auto` or `off` (anything else → `400`) |
| `locale` | string | No | language default | SonicBoom extension: pronunciation locale such as `es-PE`, or `auto`. Must belong to the synthesis language (legacy Supertonic aliases synthesize English, so only `en-US`/`en-GB` apply there); unsupported or mismatched → `400` |

Both extension fields are optional and additive: standard OpenAI clients
that omit them keep working, and unknown fields are still ignored. With the
default `auto` mode, numbers, dates, prices and similar written forms are
spoken as words; send `"normalization": "off"` for the previous audio.
Only the `input` text is prepared (once, before synthesis); a cloning
profile's reference transcript and reference audio are never modified. See
[api.md](api.md#text-normalization) for details and the
`POST /api/tts/normalize` preview.

Cloning example (Qwen3-TTS, Japanese):

```json
{
  "model": "qwen3-tts-0.6b-base",
  "voice": "<authorized-voice-profile-id>",
  "language": "ja",
  "input": "こんにちは。今日は音声合成のテストです。",
  "response_format": "wav"
}
```

See [providers.md](providers.md) for profile creation, language support,
and hardware-gated smoke tests.

**Response:** Audio data in specified format

**Example:**

```bash
curl -X POST http://localhost:17842/v1/audio/speech \
  -H "Authorization: Bearer YOUR_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "tts-1",
    "input": "Hello, world!",
    "voice": "alloy",
    "response_format": "opus"
  }' \
  --output audio.ogg
```

---

## Voice Mapping

SonicBoom maps OpenAI voice names to Supertonic 3 voice styles.
Unknown voice values return `400 Bad Request`.

| OpenAI Voice | Supertonic 3 | Description |
|--------------|---------------|-------------|
| `alloy` | M1 | Male voice 1 |
| `echo` | M2 | Male voice 2 |
| `fable` | M3 | Male voice 3 |
| `onyx` | M4 | Male voice 4 |
| `nova` | F1 | Female voice 1 |
| `shimmer` | F2 | Female voice 2 |

### Direct Voice Names

You can also use Supertonic 3 voice names directly (`M1`–`M5`, `F1`–`F5`):

| Voice | Gender |
|-------|--------|
| M1 | Male |
| M2 | Male |
| M3 | Male |
| M4 | Male |
| M5 | Male |
| F1 | Female |
| F2 | Female |
| F3 | Female |
| F4 | Female |
| F5 | Female |

---

## List Models

### GET /v1/models

Returns a list of available models.

**Authentication:** Not required (public metadata endpoint exposing only
non-sensitive model identifiers; see below)

**Response:**

```json
{
  "object": "list",
  "data": [
    {
      "id": "supertonic-3",
      "object": "model",
      "created": 1704067200,
      "owned_by": "local"
    }
  ]
}
```

**Example:**

```bash
curl http://localhost:17842/v1/models
```

---

## List Voices

### GET /v1/voices

Returns a list of available voices.

**Authentication:** Not required (public metadata endpoint exposing only
non-sensitive voice names; see below)

**Response:**

```json
{
  "object": "list",
  "data": [
    {
      "id": "M1",
      "name": "M1",
      "object": "voice"
    },
    {
      "id": "F1",
      "name": "F1",
      "object": "voice"
    }
  ]
}
```

**Example:**

```bash
curl http://localhost:17842/v1/voices
```

---

## Public Metadata Endpoints

`GET /v1/models`, `GET /v1/models/list`, `GET /v1/voices` (and
`GET /api/status`) intentionally require no authentication. They expose
only non-sensitive data — model identifiers, voice names, load
progress — and never tokens, credentials, paths, or model internals.
Compatibility with OpenAI-style clients expects these to be public, so
authentication is deliberately not required for them.

---

## Migration from OpenAI

To migrate from OpenAI to SonicBoom:

1. Change the base URL from `https://api.openai.com` to your SonicBoom server
2. Update the endpoint from `/v1/audio/speech` to `/v1/audio/speech`
3. Keep the same authentication token (if using SonicBoom's token system)
4. Update voice names if using different voices

### Python Example

**Before (OpenAI):**

```python
from openai import OpenAI

client = OpenAI(api_key="sk-...")
response = client.audio.speech.create(
    model="tts-1",
    voice="alloy",
    input="Hello, world!"
)
response.stream_to_file("output.mp3")
```

**After (SonicBoom):**

```python
import requests

url = "http://localhost:17842/v1/audio/speech"
headers = {
    "Authorization": "Bearer YOUR_TOKEN",
    "Content-Type": "application/json"
}
data = {
    "model": "tts-1",
    "voice": "alloy",
    "input": "Hello, world!",
    "response_format": "opus"
}

response = requests.post(url, headers=headers, json=data)
with open("output.ogg", "wb") as f:
    f.write(response.content)
```

---

## Response Formats

| Format | Content-Type | File Extension |
|--------|--------------|----------------|
| opus | `audio/ogg; codecs=opus` | .ogg |
| mp3 | `audio/mpeg` | .mp3 |
| wav | `audio/wav` | .wav |
| flac | `audio/flac` | .flac |

Unknown `response_format` values return `400 Bad Request`. Omitted
formats default to `opus`. Each response is one fully encoded audio
buffer (not an incremental stream).

### Model identity and availability

`/v1/models` lists recognized identifiers and their stable owning provider,
independently of build features or downloaded weights. It is not a readiness
guarantee. Speech requests for a recognized provider omitted at compile time
return HTTP 503 with an explicit provider-unavailable message. They never fall
back to Supertonic. Compiled providers still validate runtime enablement, model
installation and readiness through the provider registry. Unknown and retired
model identifiers remain HTTP 400 errors.
