# SonicBoom Native Voice-Cloning Providers

Local, in-process voice cloning with no Python, PyTorch, `tch`, upstream
CLI, network TTS service, or sidecar process at runtime. Supertonic 3
(`ort`-based ONNX) stays the default provider; Qwen3-TTS 1.7B GGUF/ONNX
(opt-in build) and GPT-SoVITS (ONNX Runtime) are optional and disabled by
default. The Candle Qwen backend was removed; only the native GGUF/ONNX
backend remains.

Upstream implementations under review (revalidate before pinning a
dependency revision):

| Provider | Source | Reviewed revision | Reality |
|---|---|---|---|
| Qwen3-TTS GGUF/ONNX | https://huggingface.co/Emerald7664/Qwen3-TTS-ONNX (ref: https://github.com/HaujetZhao/Qwen3-TTS-GGUF) | `0a36d692562f83a862466b0bf8c12154ddf1965d` | Native Rust + llama.cpp GGUF + ONNX INT8; 1.7B Base ICL cloning only (x-vector explicitly rejected); CPU validated, Vulkan functional but output-equivalence unvalidated |
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
- `QwenGgufProvider` / `GptSovitsProvider`: asset validation, language/mode
  validation, device resolution with effective-device reporting,
  per-model serialization so concurrent voices never swap reference data,
  and a bounded conditioning cache keyed by
  `(provider, model revision, voice identity, reference hash, transcript, mode)`.
- `src/tts/voices.rs`: persistent `VoiceProfileStore` (opaque UUID, consent
  metadata, `<uuid>.wav` references, no arbitrary client filenames, no
  transcript/audio logging).
- Config `[providers.qwen3_gguf]` / `[providers.gpt_sovits]` (`enabled=false`
  by default) and `[voices]`; schema, validation, hot reload (per-request
  reads, last-healthy state retained on failure). A leftover `[providers.qwen3]`
  Candle section is tolerated for boot compatibility but ignored (with a
  startup warning): delete it from old files.
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
| (default) | Supertonic 3 + GPT-SoVITS compiled; optional providers disabled, no weights fetched |
| `qwen-gguf` | Qwen3-TTS 1.7B GGUF/ONNX provider, CPU (`device=cpu\|auto`) |
| `qwen-gguf-vulkan` | Additionally allow GGUF `device=vulkan` on Vulkan-capable GPUs (implies `qwen-gguf`) |
| `qwen-gguf-cuda` | Additionally allow GGUF `device=cuda` (NVIDIA only; implies `qwen-gguf`) |
| `gpt-sovits` | GPT-SoVITS ONNX provider, CPU execution provider only |
| `cuda` / `rocm` / `coreml` | Existing ORT flags for Supertonic; they do **not** accelerate GPT-SoVITS |

AMD BC-250 (`gfx1013`) is CPU-only for the acceptance criteria. Do not
imply CUDA runs on AMD, and do not claim ROCm/MIGraphX support without
proof. `coreml`/`cuda`/`rocm` stay mutually exclusive.

## Language support (explicit, no silent substitution)

- Qwen3-TTS 1.7B Base (GGUF/ONNX): `en`, `de`, `es`, `zh`, `ja`, `fr`,
  `ko`, `ru`, `it`, `pt`. Japanese (`language=ja`) synthesizes through the
  Base model in ICL clone mode.
- GPT-SoVITS: `zh` and `en` only. The reviewed port exposes
  `Lang::{Zh,En}`: `language=ja` and `language=es` return an explicit
  `unsupported_language` error — never Chinese phonemes, never a
  Supertonic fallback. No V5, no Japanese phonemization claim.
- Supertonic: unchanged (English synthesis; legacy voice aliases).

Unknown provider/model/voice/language, incompatible variants (non-1.7B
Qwen bundles, x-vector mode on the GGUF backend, raw GPT-SoVITS `.pth`
checkpoints), missing assets, and unavailable devices all return explicit,
actionable errors. Configured `cpu` fallback from `auto` is allowed where
supported, and the effective device is always reported.

## Qwen3-TTS 1.7B GGUF/ONNX setup (CPU smoke test)

1. Special build required: `cargo build --features qwen-gguf` (add
   `qwen-gguf-vulkan` for AMD Vulkan). Default builds cannot serve Qwen.
2. Press Enable on the qwen3-gguf card (or set `enabled = true` and
   hot-reload): the server downloads the pinned bundle into
   `[providers.qwen3_gguf] model_dir` (`./models/qwen3-tts-1.7b-gguf`) in
   parallel with serving, verifying every file against the manifest in
   `src/tts/providers/qwen_gguf/models.rs` (repo
   `Emerald7664/Qwen3-TTS-ONNX` at revision
   `0a36d692562f83a862466b0bf8c12154ddf1965d`). Only the 1.7B Base bundle
   is supported; anything else fails explicitly.
3. Watch `/api/providers/qwen3-gguf/status` until `ready` (progress
   reported while downloading/loading; failures name the cause).
4. Record or reuse a recording you own (1–30 s accepted) **plus the exact
   reference transcript** (`icl` is the only supported mode).
5. Create a profile:

```bash
curl -X POST 'http://127.0.0.1:17842/api/voices/clones?name=my-voice&provider=qwen3-gguf&model=qwen3-tts-1.7b-base&language=en&mode=icl&transcript=<exact-transcript>&consent_basis=owner' \
  -H 'Authorization: Bearer <token>' -H 'Content-Type: audio/wav' \
  --data-binary @reference.wav
```

6. Synthesize (OpenAI-compatible):

```json
{
  "model": "qwen3-tts-1.7b-base",
  "voice": "<profile-id>",
  "language": "en",
  "input": "Hello, this is a local voice cloning test.",
  "response_format": "wav"
}
```

CPU smoke-test status: **measured** — 41 frames / 3.28 s audio, EOS
reached, finite PCM, conditioning cache hit on repeat, bit-identical
output for the same seed/reference. See
`benchmarks/results/2026-10-15-qwen-gguf/`.

## Pinned download manifests

Qwen3-TTS 1.7B GGUF/ONNX (`Emerald7664/Qwen3-TTS-ONNX`
`0a36d692562f83a862466b0bf8c12154ddf1965d`), SHA-256 verified by the
server before loading (full hashes in
`src/tts/providers/qwen_gguf/models.rs` `MANIFEST`): talker
`qwen3_tts_talker.q5_k.gguf`, predictor `qwen3_tts_predictor.q8_0.gguf`,
speaker/codec/decode INT8 ONNX bundles, embeddings, tokenizer.
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
2. No special build: the GPT-SoVITS backend ships in default builds (lean
   builds add `--features gpt-sovits`). CPU execution provider
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
[providers.qwen3_gguf]
enabled = false
model_dir = "./models/qwen3-tts-1.7b-gguf"
device = "auto"          # cpu | auto | vulkan (needs qwen-gguf-vulkan) | cuda (needs qwen-gguf-cuda)
gpu_layers = -1
threads = 4
max_concurrent = 1
cache_entries = 16
max_tokens = 512         # acoustic frames per synthesis (1-1024)

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

### Native GGUF/ONNX backend (`qwen3-gguf`, opt-in build)

Compiled only with `--features qwen-gguf` (not in default builds). It owns
`qwen3-tts-1.7b-base` end to end — ICL cloning, EOS-terminated generation,
reference-conditioning cache with reuse, cancellation recovery — and it is
the only Qwen backend: the Candle provider was removed.

```toml
[providers.qwen3_gguf]
enabled = false
model_dir = "./models/qwen3-tts-1.7b-gguf"
device = "auto"          # cpu | auto | vulkan (needs qwen-gguf-vulkan) | cuda (needs qwen-gguf-cuda)
gpu_layers = -1          # -1 offloads all layers, 0 is CPU
threads = 4              # 1-256
max_concurrent = 1
cache_entries = 16
max_tokens = 512         # acoustic frames per synthesis (1-1024)
```

CPU status: **measured** — 41 frames / 3.28 s audio, EOS reached, finite
PCM, conditioning cache hit on repeat (1.64 s → 0.004 s), bit-identical
output for the same seed/reference; GPU failures fall back to CPU with the
reason reported in `/api/providers/qwen3-gguf/status` (never silent).
X-vector cloning is rejected explicitly until validated against the
reference implementation.

Vulkan status (BC-250/RADV, release, seed 42): **initializes and runs** —
34 frames / 2.72 s audio, EOS, finite PCM, bit-identical across repeated
runs, ~28× faster generation than CPU (2.8 s vs 77.7 s for 256 tokens).
**Content equivalence with the validated CPU path is not established**:
outputs differ substantially from the first samples, so Vulkan is proven
functional but not proven correct — CPU remains the reference path and
Vulkan stays opt-in. Full numbers and limits in
`benchmarks/results/2026-10-15-qwen-gguf/bc250-vulkan-vs-cpu.json`.

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

- Native weight execution for both providers is wired end-to-end
  (worker-thread model ownership, prompt caching, atomic
  reference+synthesis, SHA-256-verified downloads). CPU smoke-test results
  are recorded in `benchmarks/providers.md`; rows stay pending until
  measured. No fake/silent fallbacks exist anywhere in the new code.
- Non-1.7B Qwen bundles (including the retired 0.6B Candle model),
  GPT-SoVITS v2/v2ProPlus bundles, Japanese for GPT-SoVITS, V5 bundles,
  GPU EPs for GPT-SoVITS, ROCm/CUDA on BC-250, and FLAC/MP3 reference
  uploads are explicitly unsupported and rejected with actionable errors.
  Qwen 1.7B via the GGUF/ONNX backend is CPU-validated (see above);
  Vulkan runs but its output equivalence is unvalidated, CUDA is
  unmeasured, and x-vector mode remains unsupported until measured.

## Reliable native synthesis jobs

Use the authenticated [background job API](inference-jobs.md) for long CPU
inference, with progress and cooperative cancellation. The synchronous routes
remain supported. See [ownership/cache architecture](inference-architecture.md)
for model residency, integrity verification, reloads and cache bounds.

Qwen GGUF `device=auto` selects Vulkan/CUDA only when the matching
`qwen-gguf-vulkan`/`qwen-gguf-cuda` build feature is compiled and the
backend initializes; otherwise it reports CPU with the fallback reason.
Explicit `vulkan`/`cuda` fails with an actionable error if build/runtime
support is unavailable. `cpu` remains supported on AMD BC-250. ROCm
acceleration is not implemented or advertised.

Provider concurrency settings must be 1. Sampling, frame-limit and cache-only
changes retain weights; genuine model/device changes coordinate with active work.
GGUF CPU can remain substantially slower than real-time (RTF ~26 measured);
use jobs rather than raising an HTTP timeout alone. Measured resident/peak RAM
can be several GB per model; see the current benchmark rows before enabling
multiple backends.
