# Provider Benchmarks

Real measurements only. No fabricated metrics: hardware-gated rows stay
**pending** until they are measured on the named hardware with the pinned
revisions in `docs/providers.md`.

Measurement machine for the rows below (NOT BC-250 — labeled as measured):
CachyOS x86_64, 2 CPUs, 14 GB RAM. Reference audio for all cloning runs:
`ref-en.wav`, a 5.2 s genuine Supertonic F1 utterance at 44.1 kHz with the
exact transcript
"Hello, this is a voice cloning reference recording made for local
testing." (test-owner generated, consent basis `owner`). RTF =
synthesis_time / audio_duration. Load times are cold-start wall clock
(hash verification of present files + native session construction).

## Qwen3-TTS (`qwen3-tts-0.6b-base`, CPU, `device=cpu`)

| Case | Load | Peak RSS | Synth | Audio | RTF | Result |
|---|---|---|---|---|---|---|
| `icl`, `language=ja`, input "こんにちは。今日は音声合成のテストです。" | ~29 s | — (see process row) | 962.1 s | 3.0 s @ 24 kHz | ~317 | **pass** — audible, speech envelope verified |
| `x_vector`, `language=en`, input "Hello, this is a test." | ~29 s | — | 440.4 s | 1.8 s @ 24 kHz | ~240 | **pass** — audible, speech envelope verified |
| load-only probe | 28–31 s | 4.8 GB (test process, qwen-only) | — | — | — | **pass** — status `ready`, effective device `cpu` |

Output verification (out-of-band, independent of the test assertions):
3.0 s Japanese ICL output at 24 kHz, peak 0.25, 96% nonzero samples,
quarter-second RMS trace
`0.089 0.056 0.045 0.001 0.050 0.062 0.056 0.060 0.053 0.044 0.025 0.020 0.000`
— sustained modulated phonation with natural pauses and terminal decay,
not a tone, not noise. The 1.8 s English x-vector output (peak 0.28)
shows onset burst → pause → sustained body → decay. HTTP end-to-end
(`POST /api/voices/clones` → `POST /api/voices/{id}/preview?format=wav`
→ 200, 0.64 s "Hi there." @ 24 kHz, peak 0.34; `POST /v1/audio/speech`
with profile voice → 200 valid Opus) all pass on the same build.

RTF on this 2-CPU box is high (~240–320); BC-250 and stronger CPUs will
differ — their rows stay pending until measured there.

## Qwen3-TTS (NVIDIA CUDA, optional)

| Case | Load | VRAM | Synth | Audio | RTF | Result |
|---|---|---|---|---|---|---|
| RTX 3060, `icl` | pending | pending (record 6/8/12 GB) | pending | pending | pending | **pending** — no NVIDIA hardware on the measurement machine; `qwen-cuda` builds only where the CUDA toolkit exists |

## GPT-SoVITS (`gpt-sovits-v2pro-onnx`, CPU, 9 ONNX sessions)

| Case | Load | Peak RSS | Synth | Audio | RTF | Result |
|---|---|---|---|---|---|---|
| English reference + exact transcript, request 1 ("Hello, this is a local voice cloning test.") | 8.5 s | bounded < 6 GB additional (ran with 6 GB free; no separate VmHWM captured) | 4.2–4.6 s | 3.1 s @ 32 kHz | ~1.4 | **pass** — audible, speech envelope verified |
| Same model, request 2 ("The model is reused across successive requests.") | reused (no reload) | — | 4.1–4.4 s | 3.6 s @ 32 kHz | ~1.2 | **pass** — audible; model reuse confirmed (single `load`, two syntheses) |

Output verification: 3.6 s English output at 32 kHz, peak 0.145, 84%
nonzero, quarter-second RMS trace
`0.043 0.049 0.028 0.033 0.039 0.018 0.023 0.014 0.024 0.018 0.015 0.010 0.003 0.000 0.000`
— sustained modulated phonation decaying to silence.

## Full server process (Supertonic + Qwen resident, E2E run)

| Metric | Value |
|---|---|
| Peak RSS (VmHWM) | 5.9 GB (both models resident during Qwen inference) |
| Supertonic boot | ~20 s (10 voice styles, unchanged behavior) |
| Qwen background prepare | hash-verify 2.5 GB + load ≈ 2 min, `loading (0%→73%→100%)` → `ready` |
| Legacy `POST /v1/audio/speech` (`tts-1`/`alloy`) | 200, valid Opus — no regression |
| Invalid `PATCH` (`providers.qwen3.device=rocm`) | 400, provider stays `ready` — last healthy retained |
| Valid disable/enable cycle | `disabled` → `loading` → `ready` (~2 min, Supertonic unaffected) |
| Voice CRUD | create → get → list → delete → 404 on re-get/re-delete |

## Isolation / regression (covered by unit + API tests, no hardware needed)

- Two distinct voice profiles issued concurrently never share cache keys
  (`concurrent_references_use_distinct_cache_keys`); Qwen prompts are
  cached per `(provider, revision, reference hash, transcript, mode)` in
  the model-owning worker thread; GPT-SoVITS prepares + synthesizes
  atomically under one lock acquisition.
- Qwen `icl` without transcript, GPT-SoVITS without transcript, GPT-SoVITS
  `language=ja`/`es`, raw `.pth` loads, unverified 1.7B/v2/v2ProPlus
  manifests, GPU requests for GPT-SoVITS, CUDA for Qwen without
  `qwen-cuda`, malformed bundles, and traversal profile IDs are all
  rejected with explicit codes (see `cargo test`).
- Legacy `POST /api/tts`, `/v1/audio/speech` aliases, `GET /v1/models`,
  401/429/503 contracts, queue/playback, `/ready`, config reload, and the
  TikTools plugin shape are regression-tested unchanged.

## Still pending (explicitly, not claimed)

- BC-250 (`gfx1013`) CPU rows for both providers.
- NVIDIA RTX 3060 CUDA rows for Qwen.
- GPT-SoVITS Chinese (`zh`) model-backed run (pipeline identical to the
  proven English path; needs an authorized Chinese reference + transcript).
- GPT-SoVITS-only peak RSS (VmHWM capture).
