# SonicBoom audio isolation and fidelity remediation — 2026-10-16

The confirmed client ownership defect is fixed. The tested CPU providers preserve
independent request state, including real cancellation recovery. **Speech-quality
acceptance is not complete:** listening review, broader reference coverage and
cross-build/device fidelity remain open. No sampling defaults were changed.

## Verified causes and fixes

- **Client job/audio ownership:** baseline `35416e8`,
  `frontend/src/composables/useSynthesis.ts::generate`, resumed any retained job ID
  without tying it to current input. After a transport failure, edited text,
  model or voice could therefore fetch the earlier job's audio. Old audio also
  remained visible during new work. Inputs are now frozen once, resume is explicit,
  request epochs guard callbacks, and new submissions clear prior state/audio.
  The player pauses/detaches synchronously before its element is removed; teardown
  aborts monitoring and revokes its URL. Reconnection retains the original ID.
- **Pocket text fidelity:** `UnigramTokenizer::decode` rendered UTF-8 byte-fallback
  pieces literally (`<0xC3><0x89>` instead of `É`). The independent exporter
  fixture reproduced this in sentence splitting. Bytes now reconstruct UTF-8
  before decoding. Accent, emoji and CJK round trips run without model files.
- **Pocket chunk RNG:** `PocketNative::run_chunk` restarted the same fixed seed
  at each chunk. The pinned exporter continues its random stream. RNG ownership
  now belongs to the request and continues across chunks; fresh requests restart
  independently. The real ONNX test proves both behaviors and decoder recovery.
- **Reference resampling:** linear interpolation has measurable downsampling
  aliasing. Only Qwen/Pocket reference conditioning now uses centered, DC-normalized
  Blackman windowed-sinc interpolation with a 6% transition guard and integer
  ceil sample counts. Equal-rate input is unchanged. No denoising, trimming,
  gain normalization or limiting was added. Keys include preprocessing identity;
  Pocket resolves cached embeddings before resampling. Resident weights remain.
  Voice creation additionally returns duration, source channels/rate, peak, RMS,
  DC, clipping and low-amplitude fraction, and the wizard displays conservative
  near-silence/heavy-clipping/DC warnings. These advise without rejecting or
  modifying a valid clip; no raw speech is logged.
- **Backend hardening, not an observed contamination cause:** completed audio
  could be overwritten by an accidental duplicate internal completion and retained
  bytes counted twice. Publication now rejects duplicate/stopped completions;
  late cancellation/deadline status is preserved. HTTP tests use real admission,
  storage, status and download logic with a deterministic synthesis executor.

No tested direct-provider result showed unexplained dependence on the previous
request. This does not establish absence of reference echo on arbitrary inputs.
UI replay, reference echo and PCM concatenation remain distinct diagnoses.

## Pinned sources and notices

The Qwen [MIT source and copyright notice](https://github.com/HaujetZhao/Qwen3-TTS-GGUF/tree/74feb581bc8cefb835fc608107e857036d7580a1)
were rechecked in the clean pinned checkout. Pocket's [MIT upstream flow code](https://github.com/kyutai-labs/pocket-tts/blob/ec569b2dd3fec4a601b391aace185791e92b3785/pocket_tts/models/flow_lm.py)
was fetched at that exact revision; SHA-256 is
`c4595225b0f81dc30df0b3ef77c1bcee954357452f4ec1c38abf57ccb3f71119`.
Its LSD update and square-root temperature scaling agree with the exporter path.
The pinned [ONNX export card](https://huggingface.co/KevinAHM/pocket-tts-onnx/blob/58a6d00cf13d239b6748cb0769f35c580a8f606c/README.md)
declares CC-BY-4.0. No model weights or upstream application code are redistributed
in this remediation; the fixtures and helpers identify their sources. Existing
Qwen conversion/license limitations remain in the architecture audit.

## Regression evidence

- Frontend deterministic blobs: `hola → test → mundo`, exact API mapping,
  model/voice edits, preview versus resumable job, failure/resume, confirmed
  cancellation, repeated clicks and teardown. New input never implicitly resumes
  the old job. An unresolved job must be finished or cancelled before new work.
- Mock HTTP: concurrent authenticated callers, mixed profile/built-in query
  payloads, private downloads, independent IDs, exact synthesized bytes, failed
  generation, active cancellation and deadline expiry. Stopped jobs publish no
  partial audio; snapshots contain no raw text.
- Qwen CPU: nine independent final clips. `02`, cold `04`, warm `05` and voice-A
  `06/08` are byte-identical at seed 42. Voice-B `07/09` are identical to each
  other and differ from A. A's authorized synthetic Spanish reference contains
  `hola`; B's does not. Cache hits/misses and EOS are recorded per request.
- Qwen real cancellation: cancellation waits for a generated talker frame;
  the next seeded request exactly reproduces first PCM with conditioning reuse.
  Native codec contract and exact pinned ICL prompt/trailing tensor tests passed.
- Pocket CPU: final Spanish ordered/alternating sequence and all six language
  bundles executed. The real English ONNX test cancels after a frame, continues
  RNG across identical chunks, then reproduces fresh seeded latents and PCM.
- Native errors injected after partial model execution: **NOT RUN**. HTTP failure
  publication and native cancellation are tested; these are not substitutes for
  every possible native fault.

A separate five-request Qwen CPU extension also completed `¿Hola?`, `sí`, `no`,
`¿Qué tal?` and a multi-sentence target. Clips span 0.24–2.32 seconds. Four short
hypotheses match normalized targets; the longer hypothesis contains word errors.
See `qwen-extended/` for all WAVs, hypotheses and scores. These are not manually
reviewed listening results.

## Upstream fidelity and a runtime-build limitation

Qwen source is pinned at `74feb581bc8cefb835fc608107e857036d7580a1`; converted
assets at `0a36d692562f83a862466b0bf8c12154ddf1965d`. Exact combined tokenizer IDs,
ICL prefix/fusion rows, acoustic-code rows, padding and trailing embeddings match
upstream PromptBuilder byte-for-byte on two synthetic conditioning fixtures.
Talker `[positions, positions, positions, zeros]`, predictor offsets/reset and
final decoder flush were checked against pinned source. Full intermediate hidden,
logit/sampling, precision and GPU comparisons remain **NOT RUN**.

Pocket exporter is pinned at `58a6d00cf13d239b6748cb0769f35c580a8f606c` (CC-BY-4.0;
underlying weights retain their upstream consent/license conditions). The exact
exporter generates fixtures; Python is offline validation only. On the **same
CPU ORT shared library**, all six bundles pass preparation, token IDs, long-text
chunk ordering, complete latent hashes and complete decoded PCM hashes. Latent
maximum absolute error is zero. EOS threshold stays `> -4.0`.

**Cross-build numerical comparison FAILS and is retained as a limitation.**
Production static ORT: `rel-1.28.0/da9b5e3`. Python wheel ORT 1.28:
`HEAD/45de2a8b06`; Python 1.31: `HEAD/402979a`. These differ numerically and can
change deterministic EOS (English diagnostic: 10 static frames vs 9 with the
Python 1.28 library). The matching-library test is explicitly opt-in via
`ort/load-dynamic`; production defaults/runtime were not silently switched.
The strict fixture test rejects a mismatched build, rather than increasing
error tolerances or claiming numeric parity. Speech effects of those runtime
build differences need separate evaluation.

## Audio-quality measurements

See [listening index](listen.html), [all parameter results](pocket-grid-final/quality.csv),
and each run's manifest, WAVs and pinned ASR hypotheses. References are generated
locally with eSpeak NG; no human recordings are included.

All 72 Pocket configurations completed (6 languages × 4 step counts × 3
 temperatures), with 3 texts each: **216 clips**. The fixed-seed CPU ASR readout
at production defaults (steps 1, temperature 0.7) is:

| Language | WER | CER | Mean observed RTF |
| --- | ---: | ---: | ---: |
| en | 0.00% | 0.00% | 3.321 |
| es | 23.81% | 12.17% | 2.841 |
| pt | 42.86% | 17.31% | 2.691 |
| de | 55.56% | 25.00% | 2.870 |
| it | 36.84% | 12.84% | 2.799 |
| fr | 35.00% | 10.34% | 6.392 |

These are **ASR diagnostics, not manually verified TTS error rates**. Only three
texts per configuration and synthetic references were used. More steps did not
consistently reduce WER; no new default is justified. Timings include concurrent
audit workload and are not controlled speed benchmarks. Startup/memory attribution
was **NOT RUN**. Grid targets contain no byte-fallback pieces; Unicode speech
quality after the decoder fix is **NOT RUN**, although exact text/tensor regression
checks pass.

31/216 grid PCM clips exceeded full scale; worst peak was 1.5223 and worst
out-of-range sample fraction 0.1404%. Existing WAV conversion clips those samples.
No default normalization/limiter was introduced to hide this. The nine Qwen final
clips peak at 0.6733 with no full-scale exceedance, lasting 0.40–0.48 seconds.

Whisper Tiny (pinned model/runtime and hashes in the artifacts) reports Qwen
aggregate WER 111.11%, CER 37.84%; short `test` is heard as `Tes`/`de este` by ASR.
No unexpected `hola` was detected in that nine-clip ASR set. This **does not**
prove intelligibility or absence of reference echo; suspect clips need listening.
For the same-reference Spanish Pocket before/after exploratory sequence, WER
changed 66.67% → 77.78% while CER changed 56.76% → 54.05%. This mixed result is
not evidence of perceptual improvement. Sampling defaults remain unchanged.

Numeric filter tests measure 18 kHz alias suppression relative to linear of
129.29 dB (44.1→24 kHz) and 134.16 dB (48→24 kHz). A 13–20 kHz sweep improves by
100.41 and 103.95 dB respectively. Passband amplitude, impulse timing, silence,
DC, 16→24 kHz, equal-rate identity and exact duration checks pass. Interpolation
can ring at hard edges; it does not apply hidden amplitude clamping.

A final 42-word English Pocket request requires two chunks in the pinned upstream
splitter. The current optimized Rust runner generated a fresh 14.16-second WAV in
8.33 seconds (observed RTF 0.588). See `pocket-long-final/` and the independent
expected chunk list in `fixtures/pocket-long-input.json`. This single result is
not a controlled speed benchmark or a general intelligibility guarantee. Pinned
ASR reports zero normalized WER/CER and no inserted/omitted words on these 42
words; listening is still pending.

## Compatibility, remaining gates and failures

CPU and existing Rust runtime/API/profile/playback contracts are preserved.
No Python production dependency, per-request weight reload, language fallback,
reference transcript removal, acoustic-code removal or blind audio trimming was
introduced. Pocket language/model pairings still fail explicitly when invalid.
Qwen stays 1.7B Base ICL; X-vector remains disabled because converted speaker-only
contracts and comparative intelligibility/similarity/leakage are not validated.

**NOT RUN:** human listening/naturalness/speaker similarity; live-browser and
real-model HTTP boundary comparisons; broader noisy/silent/authorized-human
reference matrix; native fault injection; GPU/CUDA/Vulkan; macOS/Windows;
higher-precision speech comparisons; controlled memory/startup measurements;
learned speech/noise coverage and externally verified language-mismatch detection. Existing conversion
limitations (missing FP32 external data/invalid FP16 graph) remain documented.
Qwen/Pocket job diagnostics expose the effective seed as an optional decimal string, preserving u64 precision in browser JSON. Providers without seed metadata retain the explicit unavailable label.

Offline ASR setup initially failed with an incomplete vocabulary download and
PyAV 19's incompatible decoder API. The pinned bundle plus PyAV 16.0.1 resolved
those setup failures. Interrupted exploratory runs are labeled in
[BASELINE-PARTIAL.md](BASELINE-PARTIAL.md), not counted as completed gates.

Test commands and exact observed summaries are in [verification.json](verification.json).
Commits are local; no PR or deployment was created.

## Quality detection and configurable sampling (2026-10-17)

**Implemented and unit/integration tested** (no real-model run in this
environment; see NOT RUN below):

- Deterministic audio-health measurement (`peak`, `rms`, `dc_offset`,
  `clipping_fraction`, `near_silent_fraction`, `max_discontinuity`,
  `impulsive_samples`, `chunk_boundary_peaks`, `non_finite_samples`).
- Layered reference-echo detector: text-only candidate isolation plus
  ASR-confirmed detection when an offline transcript is supplied. No claim
  without ASR; `confidence` remains `null` (uncalibrated).
- Configurable sampling with `balanced`/`stable`/`expressive`/`custom`
  presets, validated parameters, and `u64` seeds carried as decimal strings.
- Reference-health `defects`/`warnings`/`guidance` split (truncation,
  background-noise, quiet, silence and duration heuristics).
- Bounded (≤1) reproducible regeneration on confident failure.

**Measured:** `cargo test` — 458 passed / 0 failed / 10 ignored (real-model
artifact tests); `cargo clippy --all-targets -- -D warnings` clean; `cargo fmt
--all -- --check` clean; frontend `npm run typecheck`, `npm test` (21) and
`npm run build` pass. The audit runner gained `AUDIT_SAMPLING_PRESET`,
`AUDIT_QUALITY`, `AUDIT_DEVICE` and `AUDIT_ASR_JSON`, and each manifest row now
carries `quality` (audio-health + optional echo) and `sampling_preset`.

**NOT RUN (blocked):**

- **CPU vs Vulkan real-model comparison.** The Vulkan feature compiles and the
  runner can select `AUDIT_DEVICE=vulkan`, but no Vulkan-capable run was
  executed here; there is no CPU/Vulkan PCM-equivalence evidence in this report.
- **Reference-echo reproduction on real audio.** Confirmation requires a
  verified reference WAV/transcript plus an offline ASR transcript
  (`AUDIT_ASR_JSON`); none was available, so only synthetic fixtures prove the
  classifier.
- **X-vector-only cloning.** Still disabled: converted speaker-only contracts
  and comparative intelligibility/similarity/leakage are unvalidated.
- **Higher-precision components, Qwen3-TTS 0.6B Base and CustomVoice.** Not
  introduced; no integrity-verified, audibly validated bundles were added.

No numerical percentage improvements are claimed. Reference echo and noise are
not reported as "fixed": the deterministic path detects and reports them, and
`retry` regenerates once, but upstream model behavior is unchanged and no audio
is trimmed, filtered, gated or normalized to hide a defect.
