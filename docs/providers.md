# SonicBoom Native Voice-Cloning Providers

Local, in-process voice cloning with no Python, PyTorch, `tch`, upstream
CLI, network TTS service, or sidecar process at runtime. Supertonic 3
(`ort`-based ONNX) stays the default provider; Qwen3-TTS (Candle) and
GPT-SoVITS (ONNX Runtime) are optional and disabled by default.

Upstream implementations under review (revalidate before pinning a
dependency revision):

| Provider | Source | Reviewed revision | Reality |
|---|---|---|---|
| Qwen3-TTS | https://github.com/TrevorS/qwen3-tts-rs | `711ceee07cad92673f86de8997bdf54c30caa49f` | Candle, native Rust; Base variants support reference-audio ICL and x-vector cloning; CPU/CUDA/Metal features |
| GPT-SoVITS | https://github.com/bailu-obj/gpt-sovits-onnx-rs | `204ae4abcd218113aad86f6f9a66bdba92adcfd7` | Rust ONNX runtime; v2/v2Pro/v2ProPlus; CPU sessions; reference WAV plus transcript |

ONNX Runtime's packaged native library is the only native-code exception
and is shared by all providers (initialized once). Python may be used
**offline only** to export a GPT-SoVITS ONNX bundle, never by SonicBoom.

## What is implemented in this tree

- `src/tts/providers/`: object-safe `TtsProvider` contract (`id`,
  capabilities, `health/status`, `load/unload`, `prepare_reference`,
  `synthesize` → normalized `PcmAudio`), `TtsProviderRegistry` keyed by
  provider ID + model ID, typed errors (`unsupported_model`,
  `unsupported_language`, `invalid_voice`, `model_missing`, `loading`,
  `busy`, `device_unavailable`, `inference_failed`) mapped to the existing
  HTTP policy (`400`/`413`/`429`/`503`/sanitized `500`).
- `SupertonicProvider`: adapter preserving all ten style voices, `alloy`
  aliases, formats, auth, rate limits, and the engine API. No second
  synthesis path: the registry routes to the same inference code.
- `QwenProvider` / `GptSovitsProvider`: asset validation, language/mode
  validation, device resolution with effective-device reporting,
  per-model serialization so concurrent voices never swap reference data,
  and a bounded conditioning cache keyed by
  `(provider, model revision, reference hash, transcript, mode)`.
- `src/tts/voices.rs`: persistent `VoiceProfileStore` (opaque UUID, consent
  metadata, `<uuid>.wav` references, no arbitrary client filenames, no
  transcript/audio logging).
- Config `[providers.qwen3]` / `[providers.gpt_sovits]` (`enabled=false`
  by default) and `[voices]`; schema, validation, hot reload (per-request
  reads, last-healthy state retained on failure).
- Endpoints: `GET /api/providers`, `GET /api/providers/{id}/status`,
  `POST /api/voices/clones`, `GET /api/voices`, `GET /api/voices/{id}`,
  `DELETE /api/voices/{id}`, `POST /api/voices/{id}/preview`; extended
  `/v1/audio/speech` (new model IDs, profile voices, optional `language`)
  and dynamic `/v1/models`. Legacy `/api/tts` and OpenAI aliases behave
  exactly as before.
- Web UI provider table (server-rendered, no new framework) and an
  optional TikTools `provider`/`model` selection that preserves the
  Supertonic default.

## Build features

| Feature | Effect |
|---|---|
| (default) | Supertonic 3 only; optional providers disabled, no weights fetched |
| `qwen` | Qwen3-TTS provider, CPU (`device=cpu\|auto`) |
| `qwen-cuda` | Additionally allow Qwen `device=cuda` (NVIDIA only; never implies `ort/cuda`) |
| `gpt-sovits` | GPT-SoVITS ONNX provider, CPU execution provider only |
| `cuda` / `rocm` / `coreml` | Existing ORT flags for Supertonic; they do **not** accelerate GPT-SoVITS |

AMD BC-250 (`gfx1013`) is CPU-only for the acceptance criteria. Do not
imply CUDA runs on AMD, and do not claim ROCm/MIGraphX support without
proof. `coreml`/`cuda`/`rocm` stay mutually exclusive.

## Language support (explicit, no silent substitution)

- Qwen3-TTS Base: `en`, `ja`, `es`, `zh`, `ko`, `fr`, `de`. Japanese
  (`language=ja`) synthesizes through the Base model in clone mode.
- GPT-SoVITS: `zh` and `en` only. The reviewed port exposes
  `Lang::{Zh,En}`: `language=ja` and `language=es` return an explicit
  `unsupported_language` error — never Chinese phonemes, never a
  Supertonic fallback. No V5, no Japanese phonemization claim.
- Supertonic: unchanged (English synthesis; legacy voice aliases).

Unknown provider/model/voice/language, incompatible variants (Qwen
CustomVoice/VoiceDesign for cloning, raw GPT-SoVITS `.pth` checkpoints),
missing assets, and unavailable devices all return explicit, actionable
errors. Configured `cpu` fallback from `auto` is allowed where
supported, and the effective device is always reported.

## Qwen3-TTS setup (CPU smoke test)

1. Build with the provider: `cargo build --features qwen` (default
   Cargo features stay on for the server).
2. Set `enabled = true` and hot-reload (or restart): the server downloads
   the pinned bundle into `[providers.qwen3] model_dir`
   (`./models/qwen3-tts`) in parallel with serving, verifying every file
   against the manifest below. For **Qwen3-TTS-12Hz-0.6B-Base** the bundle
   is `config.json`, `vocab.json`, `merges.txt`, `model.safetensors`,
   `speech_tokenizer/model.safetensors` — all present in the single source
   repo `Qwen/Qwen3-TTS-12Hz-0.6B-Base` at revision
   `5d83992436eae1d760afd27aff78a71d676296fc` (no separate tokenizer-repo
   download: the local `vocab.json`/`merges.txt` pair is used directly).
   CustomVoice/VoiceDesign checkpoints are rejected for cloning, and the
   1.7B Base has no verified manifest yet (explicit error, not silence).
3. Watch `/api/providers/qwen3/status` until `ready` (progress reported
   while downloading/loading; failures name the cause).
4. Record or reuse a recording you own (3–10 s recommended, 1–30 s
   accepted; 24 kHz reference handling per upstream) **plus the exact
   reference transcript** for `icl` mode (`x_vector` needs no transcript).
5. Create a profile:

```bash
curl -X POST 'http://127.0.0.1:17842/api/voices/clones?name=my-voice&provider=qwen3&model=qwen3-tts-0.6b-base&language=ja&mode=icl&transcript=<exact-transcript>&consent_basis=owner' \
  -H 'Authorization: Bearer <token>' -H 'Content-Type: audio/wav' \
  --data-binary @reference.wav
```

6. Synthesize (OpenAI-compatible):

```json
{
  "model": "qwen3-tts-0.6b-base",
  "voice": "<profile-id>",
  "language": "ja",
  "input": "こんにちは。今日は音声合成のテストです。",
  "response_format": "wav"
}
```

CPU smoke-test status: **measured** — `icl` Japanese (3.0 s @ 24 kHz,
synth 962 s, RTF ~320 on a 2-CPU dev box) and `x_vector` English (1.8 s
@ 24 kHz, synth 440 s, RTF ~240) both pass with speech-verified
envelopes; load ~30 s, peak RSS 4.8 GB qwen-only. See
`benchmarks/providers.md`. BC-250 rows stay pending until measured there.

## Pinned download manifests

Qwen3-TTS 0.6B-Base (`Qwen/Qwen3-TTS-12Hz-0.6B-Base`
`5d83992436eae1d760afd27aff78a71d676296fc`), SHA-256 verified by the
server before loading:

| File | SHA-256 | Size |
|---|---|---|
| `config.json` | `2e714c78…ba011` | 4,494 |
| `vocab.json` | `ca10d7e9…1440910` | 2,776,833 |
| `merges.txt` | `599bab54…26f5e3` | 1,671,839 |
| `model.safetensors` | `180b3b10…3b6946f6` | 1,829,344,272 |
| `speech_tokenizer/model.safetensors` | `836b7b35…d30ba571258` | 682,293,092 |

(Full hashes in `src/tts/providers/qwen.rs` `QWEN_06B_MANIFEST`.)
GPT-SoVITS v2Pro (`mikv39/gpt-sovits-onnx-custom`
`69063da489c62f34fffc40acd10e2167623509aa`): required `vits.onnx`,
`ssl.onnx`, `t2s_encoder.onnx`, `t2s_fs_decoder.onnx`,
`t2s_s_decoder.onnx`; quality optionals `bert.onnx`, `g2pW.onnx`,
`g2p_en/{encoder,decoder}_model.onnx`, `sv.onnx` (full hashes in
`src/tts/providers/gptsovits.rs` `GPTSOVITS_V2PRO_MANIFEST`).

## GPT-SoVITS setup (CPU smoke test)

1. Set `[providers.gpt_sovits] enabled = true`, `model_id =
   gpt-sovits-v2pro-onnx`, hot-reload (or restart): the server downloads
   the pinned prebuilt bundle recommended by the reviewed port and
   verifies every file. Offline self-export with the upstream tooling
   remains supported: place a complete bundle in `model_dir` and verified
   local files are reused without re-downloading. Raw `.pth` files are
   rejected with export guidance; never place them in the bundle directory
   expecting them to load. Only `gpt-sovits-v2pro-onnx` has a verified
   manifest today; v2/v2ProPlus fail with explicit guidance.
2. Build: `cargo build --features gpt-sovits`. CPU execution provider
   only; GPU EPs are experimental/unsupported for this provider.
3. Watch `/api/providers/gpt-sovits/status` until `ready`.
4. Create a profile — reference WAV **and exact transcript are required**:

```bash
curl -X POST 'http://127.0.0.1:17842/api/voices/clones?name=en-voice&provider=gpt-sovits&model=gpt-sovits-v2pro-onnx&language=en&mode=icl&transcript=<exact-transcript>&consent_basis=owner' \
  -H 'Authorization: Bearer <token>' -H 'Content-Type: audio/wav' \
  --data-binary @reference.wav
```

```json
{
  "model": "gpt-sovits-v2pro-onnx",
  "voice": "<profile-id>",
  "language": "en",
  "input": "Hello, this is a local voice cloning test.",
  "response_format": "wav"
}
```

CPU smoke-test status: **measured** — English synthesis with model
reuse across successive requests passes on CPU (3.1 s + 3.6 s @ 32 kHz,
RTF ~1.2–1.4, load 8.5 s; see `benchmarks/providers.md`). Concurrency
isolation (distinct references under simultaneous requests,
missing/broken bundle handling) is covered by unit tests; the Chinese
model-backed run is opt-in and pending an authorized Chinese reference.

## Configuration reference

```toml
[providers.qwen3]
enabled = false
model_dir = "./models/qwen3-tts"
model_id = "qwen3-tts-0.6b-base"
revision = "5d83992436eae1d760afd27aff78a71d676296fc"
device = "auto"          # cpu | auto | cuda (cuda needs qwen-cuda)
max_concurrent = 1
cache_entries = 16
max_tokens = 1024        # codec frames per synthesis (128-4096)

[providers.gpt_sovits]
enabled = false
model_dir = "./models/gpt-sovits-v2pro"
model_id = "gpt-sovits-v2pro-onnx"
revision = "69063da489c62f34fffc40acd10e2167623509aa"
device = "auto"          # cpu | auto only
max_concurrent = 1
cache_entries = 16
top_k = 4                # 0 disables
top_p = 0.9              # 0.0 disables
temperature = 1.0
repetition_penalty = 1.0 # 1.0 disables

[voices]
store_dir = "./voices"
max_upload_bytes = 8388608
```

No passwords or tokens live in TOML. Invalid provider updates keep the
last healthy state and surface actionable status via
`/api/providers/{id}/status` and `/api/config/status`.

## Security notes

- All speech endpoints keep Bearer-token auth, rate limits, inference
  admission limits, body limits, and timeout semantics. `/v1/models` and
  `/v1/voices` stay public metadata without private clone data; voice
  profiles require authorization and are scoped to the owning token
  (foreign IDs report `404`, never existence-revealing `403`).
- Reference uploads validate duration, decoded sample count, channels,
  sample rate, MIME **and actual format** (WAV contents required;
  FLAC/MP3 uploads return `415`), transcript presence, and consent basis.
  Oversize uploads return `413`. Paths are UUID-derived; traversal IDs
  are rejected.
- Only user-owned/authorized recordings are supported; store the
  permission/consent basis on the profile. Do not bundle proprietary or
  unlicensed character voice checkpoints, and do not claim official
  Hatsune Miku support.

## Open limitations (honest, not silent)

- Native Candle/ONNX weight execution for both providers is wired
  end-to-end (worker-thread model ownership, prompt caching, atomic
  reference+synthesis, SHA-256-verified downloads). CPU smoke-test results
  are recorded in `benchmarks/providers.md`; rows stay pending until
  measured. No fake/silent fallbacks exist anywhere in the new code.
- Qwen 1.7B Base, GPT-SoVITS v2/v2ProPlus bundles, Japanese for
  GPT-SoVITS, V5 bundles, GPU EPs for GPT-SoVITS, ROCm/CUDA on BC-250,
  and FLAC/MP3 reference uploads are explicitly unsupported and rejected
  with actionable errors.
