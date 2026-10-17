# SonicBoom Native Voice-Cloning Providers

Local, in-process voice cloning with no Python, PyTorch, `tch`, upstream
CLI, network TTS service, or sidecar process at runtime. Supertonic 3
(`ort`-based ONNX) stays the default provider; Qwen3-TTS 1.7B GGUF/ONNX
(opt-in build) and Pocket TTS (ONNX Runtime) are optional and disabled by
default. The Candle Qwen backend and GPT-SoVITS were removed; Pocket TTS
is the audio-prompt cloning provider.

Upstream implementations under review (revalidate before pinning a
dependency revision):

| Provider | Source | Reviewed revision | Reality |
|---|---|---|---|
| Qwen3-TTS GGUF/ONNX | https://huggingface.co/Emerald7664/Qwen3-TTS-ONNX (ref: https://github.com/HaujetZhao/Qwen3-TTS-GGUF) | `0a36d692562f83a862466b0bf8c12154ddf1965d` | Native Rust + llama.cpp GGUF + ONNX INT8; 1.7B Base ICL cloning only (x-vector explicitly rejected); CPU validated, Vulkan functional but output-equivalence unvalidated |
| Pocket TTS | https://huggingface.co/KevinAHM/pocket-tts-onnx (export of https://github.com/kyutai-labs/pocket-tts) | `58a6d00cf13d239b6748cb0769f35c580a8f606c` | Pure-Rust driver over 5 ONNX graphs per language; CPU sessions; reference WAV, no transcript (audio-prompt mode) |

ONNX Runtime's packaged native library is the only native-code exception
and is shared by all providers (initialized once). Python was used
**offline only** to validate the Pocket TTS ONNX bundles, never by SonicBoom.

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
- `QwenGgufProvider` / `PocketTtsProvider`: asset validation, language/mode
  validation, device resolution with effective-device reporting,
  per-model serialization so concurrent voices never swap reference data,
  and a bounded conditioning cache keyed by
  `(provider, model revision, voice identity, reference hash, transcript, mode)`.
- `src/tts/voices.rs`: persistent `VoiceProfileStore` (opaque UUID, consent
  metadata, `<uuid>.wav` references, no arbitrary client filenames, no
  transcript/audio logging).
- Config `[providers.qwen3_gguf]` / `[providers.pocket_tts]` (`enabled=false`
  by default) and `[voices]`; schema, validation, hot reload (per-request
  reads, last-healthy state retained on failure). Leftover `[providers.qwen3]`
  Candle and `[providers.gpt_sovits]` sections are tolerated for boot
  compatibility but ignored (with a startup warning): delete them from old
  files. Old GPT-SoVITS voice profiles are kept on disk but unavailable;
  recreate them as Pocket profiles from their stored reference audio.
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
| (default) | Supertonic 3 + Pocket TTS compiled; optional providers disabled, no weights fetched |
| `qwen-gguf` | Qwen3-TTS 1.7B GGUF/ONNX provider, CPU (`device=cpu\|auto`) |
| `qwen-gguf-vulkan` | Additionally allow GGUF `device=vulkan` on Vulkan-capable GPUs (implies `qwen-gguf`) |
| `qwen-gguf-cuda` | Additionally allow GGUF `device=cuda` (NVIDIA only; implies `qwen-gguf`) |
| `pocket-tts` | Pocket TTS ONNX provider, CPU execution provider only |
| `cuda` / `rocm` / `coreml` | Existing ORT flags for Supertonic; they do **not** accelerate Pocket TTS |

AMD BC-250 (`gfx1013`) is CPU-only for the acceptance criteria. Do not
imply CUDA runs on AMD, and do not claim ROCm/MIGraphX support without
proof. `coreml`/`cuda`/`rocm` stay mutually exclusive.

### Build-time SDK validation

The native wrapper (`native/qwen-llama/build.rs`) prints the effective
backend selection and validates every requested SDK **before** CMake runs, so
a missing SDK fails early and actionably instead of deep inside
`ggml-cuda`/`ggml-vulkan`:

- `qwen-gguf-cuda` requires the CUDA Toolkit (`nvcc`). Discovery mirrors
  CMake's `FindCUDAToolkit`: `CUDACXX`, `CUDAToolkit_ROOT`,
  `CUDA_PATH`/`CUDA_HOME`, `/usr/local/cuda/bin`, `/opt/cuda/bin`, the
  Windows default toolkit path, then `PATH`.
- `qwen-gguf-vulkan` requires `glslc`, the Vulkan loader/headers, and
  SPIRV-Headers. The pinned llama.cpp revision finds the SPIRV-Headers
  *config* package but never links its include directory, so the build script
  locates a directory holding `spirv/unified1/spirv.hpp`,
  `spirv-headers/spirv.hpp` or `spirv.hpp` (`$SONICBOOM_SPIRV_INCLUDE_DIR`,
  `$VULKAN_SDK/include`, `~/.local[/spirv-headers]/include`, a
  `CMAKE_PREFIX_PATH` prefix, `/usr/include`, `/usr/local/include`,
  `/opt/include`) and wires it into the `ggml-vulkan` target. Set
  `SONICBOOM_SPIRV_INCLUDE_DIR` when the headers live somewhere non-standard.
- `qwen-gguf-cuda,qwen-gguf-vulkan` together are allowed only when **both**
  SDKs validate; otherwise the build stops naming the missing one.

Each failure names the required tool, the discovery variables, and a recovery
command (install path, `export CUDAToolkit_ROOT=…`/`VULKAN_SDK=…`/
`SONICBOOM_SPIRV_INCLUDE_DIR=…`, or the CPU-only build). `CMAKE_PREFIX_PATH`
and every discovery variable are registered with `cargo:rerun-if-env-changed`,
so changing them reconfigures deterministically. `SONICBOOM_SKIP_SDK_CHECK=1`
bypasses the pre-flight scan for unusual toolchains where CMake can still
discover the SDK. A `cargo:warning` line such as
`native GGML backend selection: CPU + Vulkan (ggml-vulkan)` states the
effective selection without reading CMake output.

## Language support (explicit, no silent substitution)

- Qwen3-TTS 1.7B Base (GGUF/ONNX): `en`, `de`, `es`, `zh`, `ja`, `fr`,
  `ko`, `ru`, `it`, `pt`. Japanese (`language=ja`) synthesizes through the
  Base model in ICL clone mode.
- Pocket TTS: `en` (`pocket-tts-en`), `es` (`pocket-tts-es`),
  `pt` (`pocket-tts-pt`), `de` (`pocket-tts-de`), `it` (`pocket-tts-it`),
  and `fr` (`pocket-tts-fr-24l`, larger 24-layer preview). Each has its
  own pinned bundle and tokenizer. Use `pt` for Portuguese, not `por`.
  Chinese (`zh`), Japanese (`ja`), Dutch (`nl`), and other unverified bundles
  return explicit unsupported-language/model errors. No language falls back
  to English or another provider. Existing Qwen language support is unchanged.
- GPT-SoVITS is retired: its model IDs return an explicit retired-model
  error and its old voice profiles are unavailable (recreate as Pocket).
- Supertonic: unchanged (English synthesis; legacy voice aliases).

Unknown provider/model/voice/language, incompatible variants (non-1.7B
Qwen bundles, x-vector mode on the GGUF backend, non-`audio_prompt` mode
on Pocket TTS), missing assets, and unavailable devices all return explicit,
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

### Qwen3-TTS sampling and quality settings

Additive configuration (existing files keep loading unchanged):

```toml
[providers.qwen3_gguf.sampling]
preset = "balanced"        # balanced | stable | expressive | custom

[providers.qwen3_gguf.quality]
enabled = false            # detection is off by default
policy = "report"          # off | report | retry | experimental_trim
max_retries = 1            # hard ceiling of 1; bounded regeneration only
```

`balanced` reproduces the pinned upstream defaults exactly, so under identical
artifacts, seed and device the output is unchanged. `stable` and `expressive`
are experimental; `custom` takes explicit `[providers.qwen3_gguf.sampling.talker]`
/ `.predictor` parameters (`temperature`, `top_k`, `top_p`, `repetition_penalty`).

Per-request overrides are optional query parameters on `/api/tts/jobs`,
`/api/voices/{id}/preview` and the OpenAI path: `sampling_preset`, `seed`
(a decimal string so all 64 bits survive JavaScript), `quality`,
`talker_temperature`, `talker_top_k`, `talker_top_p`, `talker_repetition_penalty`,
and the `predictor_*` equivalents. Invalid values are a `400`; specifying
sampling/quality for a non-Qwen provider is rejected explicitly rather than
silently ignored. Detection semantics, the offline-ASR requirement for echo
confirmation, and remaining limitations are documented in
`docs/qwen-gguf-architecture.md`.

## Pinned download manifests

Qwen3-TTS 1.7B GGUF/ONNX (`Emerald7664/Qwen3-TTS-ONNX`
`0a36d692562f83a862466b0bf8c12154ddf1965d`), SHA-256 verified by the
server before loading (full hashes in
`src/tts/providers/qwen_gguf/models.rs` `MANIFEST`): talker
`qwen3_tts_talker.q5_k.gguf`, predictor `qwen3_tts_predictor.q8_0.gguf`,
speaker/codec/decode INT8 ONNX bundles, embeddings, tokenizer.
Pocket TTS per-language bundles (`KevinAHM/pocket-tts-onnx`
`58a6d00cf13d239b6748cb0769f35c580a8f606c`, CC-BY-4.0): `bundle.json`,
`tokenizer.model`, `bos_before_voice.npy`, `text_conditioner.onnx`,
`mimi_encoder.onnx`, `flow_lm_main_int8.onnx`, `flow_lm_flow_int8.onnx`,
`mimi_decoder_int8.onnx` (full hashes in
`src/tts/providers/pocket_tts.rs` `POCKET_TTS_EN_MANIFEST` /
`POCKET_TTS_ES_MANIFEST`).

## Pocket TTS setup (CPU smoke test)

1. In **Engines**, click **Activate** beside the Pocket language model you want.
   This selects that model and enables Pocket in one settings update.
   The model shows **Loading…** during download/load and **Active** when ready.
   Previously downloaded bundles and saved voices are retained when switching.
   Alternatively, set `[providers.pocket_tts] enabled = true` and choose one
   `model_id` from the table below. Hot-reload (or restart) downloads and verifies that
   language's eight pinned files. Complete verified local bundles are reused.
   One bundle is resident at a time; change `model_id` and wait for `ready`
   before generating with another language. The French preview has 24 layers
   and costs more RAM/CPU than the six-layer distilled models.

   | Language | model_id | Bundle under `model_dir/onnx` |
   | --- | --- | --- |
   | English | `pocket-tts-en` | `english_2026-04` |
   | Spanish | `pocket-tts-es` | `spanish` |
   | Portuguese | `pocket-tts-pt` | `portuguese` |
   | German | `pocket-tts-de` | `german` |
   | Italian | `pocket-tts-it` | `italian` |
   | French (preview) | `pocket-tts-fr-24l` | `french_24l` |

2. No special build: the Pocket TTS backend ships in default builds (lean
   builds add `--features pocket-tts`). CPU execution provider
   only; GPU EPs are unsupported for this provider.
3. Watch `/api/providers/pocket-tts/status` until `ready`.
4. Create a profile — reference WAV only, **no transcript needed**
   (`audio_prompt` is the only supported mode):

```bash
curl -X POST 'http://127.0.0.1:17842/api/voices/clones?name=en-voice&provider=pocket-tts&model=pocket-tts-en&language=en&mode=audio_prompt&consent_basis=owner' \
  -H 'Authorization: Bearer <token>' -H 'Content-Type: audio/wav' \
  --data-binary @reference.wav
```

```json
{
  "model": "pocket-tts-en",
  "voice": "<profile-id>",
  "language": "en",
  "input": "Hello, this is a local voice cloning test.",
  "response_format": "wav"
}
```

CPU smoke-test status: **measured** — English (3.44 s @ 24 kHz) and
Spanish (2.80 s @ 24 kHz) cloned synthesis with model reuse passes on
CPU at ~1.5x real time (see `benchmarks/providers.md`). Concurrency
isolation (distinct references under simultaneous requests,
missing/broken bundle handling) is covered by unit tests.

## GPT-SoVITS retirement

GPT-SoVITS was removed and replaced by Pocket TTS. Its model IDs
(`gpt-sovits-v2-onnx`, `gpt-sovits-v2pro-onnx`, `gpt-sovits-v2proplus-onnx`)
return an explicit retired-model error (never a silent reroute), a
leftover `[providers.gpt_sovits]` config section boots with a warning
and is otherwise ignored, and old GPT-SoVITS voice profiles are kept on
disk but unavailable: recreate them as `pocket-tts` profiles
(`mode=audio_prompt`) from their stored reference audio. The old
`models/gpt-sovits-v2pro` cache is left inert for manual removal.

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
allow_cpu_fallback = false  # explicit vulkan/cuda failure falls back to CPU when true

[providers.pocket_tts]
enabled = false
model_dir = "./models/pocket-tts"
model_id = "pocket-tts-en"   # en/es/pt/de/it models; French: pocket-tts-fr-24l
revision = "58a6d00cf13d239b6748cb0769f35c580a8f606c"
device = "auto"          # cpu | auto only
max_concurrent = 1
cache_entries = 16
temperature = 0.7        # 0.0 disables noise
lsd_steps = 1            # 1-8
cpu_threads = 4          # 1-32

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
allow_cpu_fallback = false  # explicit device failure -> reported CPU fallback when true
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
  unvalidated Pocket TTS bundles, non-`audio_prompt` Pocket modes,
  GPU EPs for Pocket TTS, ROCm/CUDA on BC-250, and FLAC/MP3 reference
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
support is unavailable, **unless** `allow_cpu_fallback = true` is set, in
which case the failure is reported and synthesis runs on CPU. The
requested device is the config value; the effective device (with any
fallback reason) is exposed as `effective_device`/detail in
`/api/providers/qwen3-gguf/status` and is never the requested value unless
it actually initialized. `cpu` remains supported on AMD BC-250. ROCm
acceleration is not implemented or advertised.

Provider concurrency settings must be 1. Sampling, frame-limit and cache-only
changes retain weights; genuine model/device changes coordinate with active work.
GGUF CPU can remain substantially slower than real-time (RTF ~26 measured);
use jobs rather than raising an HTTP timeout alone. Measured resident/peak RAM
can be several GB per model; see the current benchmark rows before enabling
multiple backends.

### Audio isolation and quality follow-up (2026-10-16)

Use `tts-quality-audit --help` (Rust binary) and the commands in
`benchmarks/tts-quality/README.md` for independent direct-provider WAV manifests.
The UI now separates new submission from explicitly resuming the stored previous
input; a changed text/model/voice cannot silently resume an older job. A new
submission clears and releases old audio, and disposal invalidates callbacks.
Cancellation acknowledgement does not imply native completion.

Qwen remains **1.7B Base ICL only**. Provide an authorized clean reference
(3–10 seconds is a useful starting range, subject to provider bounds) and its
exact transcript, punctuation and language. Transcript mismatch and reference
utterance echo are different from previous-request contamination. No transcript,
reference codes or leading audio have been removed as a workaround. Experimental
X-vector remains disabled: the converted speaker-only prompt and decoder path
have no validated intelligibility/speaker/leakage comparison with ICL.

Pocket uses transcript-free audio prompting and separate en/es/pt/de/it/fr
bundles (French is `pocket-tts-fr-24l`); unsupported pairings still fail explicitly.
Reviewed exporter `KevinAHM/pocket-tts-onnx` revision
`58a6d00cf13d239b6748cb0769f35c580a8f606c`, `pocket_tts_onnx.py` SHA-256
`4381a4396ba08b2626a25a87001e3c51dbacd136e1022d2d40a8cefb14b44be0`.
Prompt preparation, chunk-state cloning and EOS `> -4.0` follow that runtime.
Its normal sampler continues across chunks. Rust now owns one seeded RNG per
request rather than restarting the same seed for every chunk. This changes
seeded multi-chunk outputs intentionally; single-chunk sampling semantics and
production temperature 0.7 / LSD steps 1 are preserved. No claim of NumPy/Rust
bit-identical RNG or perceptual improvement follows from that correction.

Only reference conditioning uses the new centered Blackman windowed-sinc
resampler (`sinc-blackman-96-zero-phase-v1`), with integer ceil duration and
versioned conditioning identity. Same-rate audio is copied unchanged. The
filter does not denoise, trim, stretch or normalize waveforms. Interpolation
can ring at sharp discontinuities; there is no hidden clipping/limiting step.
Playback and output-format resampling retain their existing implementation.

Pocket's independent exporter fixture exposed and fixed a tokenizer decode bug:
byte-fallback IDs were rendered as literal `<0xXX>` strings during sentence
splitting. UTF-8 bytes are now reassembled before Unicode decoding. Regression
cases cover accents, emoji and CJK without model files. The real exporter check
covers preparation, token IDs, long-text chunk order and all six language bundles.

Numerical parity must also pin the **runtime build**, not merely the ORT version.
The production static ORT reports `rel-1.28.0/da9b5e3`; the Python 1.28 wheel
reports `HEAD/45de2a8b06`. A temperature-zero diagnostic produces different English
latent values and EOS counts between those builds (10 vs 9 frames). Loading the
Python wheel's same CPU library into the Rust opt-in comparison yields zero
latent error for all six bundles. This is controlled same-runtime pipeline
validation, not a change to the shipped static runtime. Cross-build speech
fidelity is still unvalidated; see the audit report and explicit invocation.
