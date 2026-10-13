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


## 2026-10-13 BC-250 refactor measurements

This supersedes the BC-250 and GPT-only RSS pending items above. Hardware:
AMD BC-250, 6 physical / 12 logical CPU cores, approximately 14 GiB RAM and
14 GiB swap; Linux, Rust 1.99, release build with `server,qwen,gpt-sovits`,
CPU execution. Qwen upstream code revision `711ceee07cad92673f86de8997bdf54c30caa49f`
with the vendored cancellation patch; model revision
`5d83992436eae1d760afd27aff78a71d676296fc`. GPT model revision
`69063da489c62f34fffc40acd10e2167623509aa`.
Reference audio was generated locally by native Supertonic for these tests;
no private user recordings were used. Raw results are in
[results/2026-10-13-bc250](results/2026-10-13-bc250).

| Qwen case | Wall seconds | Audio seconds | RTF | Conditioning |
|---|---:|---:|---:|---|
| Cold native load, verified files | 7.493 | — | — | — |
| Resident model reuse | 0.000603 | — | — | load count remains 1 |
| X-vector | 18.780 | 1.84 | 10.206 | miss |
| Same voice repeated | 15.541 | 1.84 | 8.446 | hit |
| ICL preparation | 1.593 | — | — | miss |
| ICL synthesis | 32.551 | 1.76 | 18.495 | hit |
| Different voice | 14.523 | 1.52 | 9.554 | miss |
| Long text | 120.896 | 12.64 | 9.565 | hit |
| Healthy synthesis after cancellation | 9.597 | 0.88 | 10.906 | hit |

Active native cancellation was requested after frame 1 and computation
returned 40 ms later. The resident model remained healthy, load count 1.
All completed Qwen benchmark cases terminated with EOS and produced finite,
nonempty audio with measurable energy. Peak process RSS across the sequence
was 6,123,420 KiB; individual snapshots are in the raw results. This includes
reference generation and conditioning, not a claim about weights alone.
`time` reported 230.76 s wall / 336.53 s user / 76.15 s system,
approximately 179% average utilization (100% = one logical CPU).

For the long-text generation stage, measured time was 95.343 s for 158
frames: acoustic prediction 60.138 s, talker 24.413 s, sampling 10.771 s,
mean frame latency 0.603 s. Acoustic prediction is a measured CPU hotspot;
no speculative kernel replacement or quality-reducing bypass was applied.

| GPT-SoVITS CPU case | Wall seconds | Audio seconds | RTF |
|---|---:|---:|---:|
| Cold native load | 9.611 | — | — |
| First English synthesis | 4.005 | 3.30 | 1.214 |
| Same reference repeated | 2.350 | 3.30 | 0.712 |

GPT-only sequence peak RSS was 3,454,240 KiB. Whole sequence: 19.53 s wall,
62.52 s user, 4.45 s system (~343% average CPU utilization).

### Controlled comparison and limits

`examples/provider_compare.rs` ran the same owned reference, text, seed 42
and CPU model against original main `0a350b1` and this refactor. All six
outputs were exactly equal after f32 sample hashing:
`0053b2c9abc2e11e23ce37c04b92a9c0a5eaf4ba9f955cc1c38e72637c7b0f34`
(1.36 seconds of audio). Baseline timings were 16.536, 16.958, 12.175 s;
patched timings 11.963, 11.847, 11.778 s. The first two baseline runs
coincided with compilation, and the final pair is too small a sample to
establish a speedup. Six-thread runs (14.455, 15.227, 11.897 s) did not
establish a threading benefit either; defaults were not tuned on that basis.
The older 2-CPU 440/962-second results are a different environment and
must not be used to calculate a refactor speedup.

For one pinned 1,829,344,272-byte asset, whole-file SHA verification peaked
at 1,793,584 KiB RSS versus 7,944 KiB for buffered verification. Elapsed
4.030 versus 2.652 s is a single, ordered run affected by page cache;
the supported improvement is bounded allocation, not a universal latency
ratio. Warm trusted metadata avoids hashing unchanged files altogether.

Reproduce with the release examples `cache_benchmark`, `tts_benchmark`,
and `provider_compare`; see [profiling instructions](../docs/inference-architecture.md).
CUDA, RTX 3060 latency/VRAM and ROCm remain unverified: this runner has no
NVIDIA device or CUDA toolkit. Chinese GPT references and interactive browser/keyboard
QA also remain pending; Firefox headless mobile/desktop layouts were inspected. No real-time Qwen CPU guarantee is made.


The separate, unseeded GPT regression also produced a 60.3-second waveform
for a short sentence (88.3-second synthesis). Audible/finite validation alone
is not proof of linguistic quality. Seeded benchmark results above must not
be generalized across stochastic settings; subjective quality and EOS behavior
for such GPT outputs warrant further upstream investigation.


Final release HTTP job smoke test with `request_timeout_secs=1` returned
202 in 0.792 ms, stopped generation after frame 1, and successfully completed
a subsequent 6.605-second job despite that HTTP timeout. SSE disconnection
preserved the job. Completed WAV was 0.72 seconds, private and non-cacheable;
model load count remained 1. See the raw HTTP job record for polling-inclusive
cancellation timing.
