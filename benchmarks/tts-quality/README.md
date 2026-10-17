# Request isolation remediation

Baseline: fetched `origin/main` = `35416e8` on 2026-10-16; clean worktree.

Confirmed client defect: `useSynthesis.generate` used any retained job ID without
binding it to the submitted input. A transport failure followed by changing text,
voice or model could resume the previous job and publish its audio. The previous
object URL also stayed visible throughout a new submission. This proves a client
identity failure, not acoustic concatenation or model KV leakage.

Backend inspection: jobs have independent control UUIDs, token-derived owners,
move-owned request bodies, completed-only audio downloads, cancellation checks
before publication and `private, no-store` responses. This inspection alone does
not prove HTTP integration or speech-content correctness.

Native inspection: Qwen calls `runtime.reset()` before generation; it clears both
contexts. Predictor clears each frame. Pocket clones prefilled state for each
chunk and initializes decoder state per decode. No model-state cause is confirmed.

Current completed runs, numerical runtime-build failures and remaining gates are
reported in [REPORT.md](REPORT.md) and [verification.json](verification.json).
Real CPU weights were available and executed; model directories alone were not
counted as successful validation. Human listening and GPU acceptance remain open.

## Rust diagnostic runner

```
cargo test --no-default-features --test tts_request_isolation
cargo run --no-default-features --bin tts-quality-audit -- --help
# Create the parent, but not NEW_OUTPUT_DIR; all destinations are create-new.
cargo run --release --no-default-features --features pocket-tts --bin tts-quality-audit -- pocket pocket-tts-es es models/pocket-tts NEW_OUTPUT_DIR AUTHORIZED_REFERENCE.wav
QWEN_REFERENCE_TRANSCRIPT='exact transcript' cargo run --release --no-default-features --features qwen-gguf --bin tts-quality-audit -- qwen qwen3-tts-1.7b-base es models/qwen3-tts-1.7b-gguf NEW_OUTPUT_DIR AUTHORIZED_REFERENCE.wav
```

Runner sequence: hola, test, mundo, cold test (engine dropped/reloaded), warm
test, punctuation, short words, combined text and multiple sentences. Each call
uses a fresh UUID and WAV destination. JSONL records hashes, effective CPU,
revision, seed, timing/RTF, sample count, health statistics and exposed metrics.
`job_id=null` explicitly means this is the direct-provider boundary, not HTTP.
Unexposed Pocket metrics are null/unknown, not invented. `listen.html` provides
playable clips. Same WAV hashes establish determinism only, never word accuracy.
The mock test verifies independent files, identities, WAV lengths and no overwrite;
it does not exercise real HTTP jobs or infer speech correctness from mock signals.

## Offline parameter and content evaluation

`pocket_grid.py RUNNER MODEL_DIR AUTHORIZED_REFERENCE NEW_OUTPUT` runs the Rust
binary for every supported language and all steps {1,2,4,8}/temperatures
{0.5,0.7,0.9}, using short, medium and multi-sentence targets local to each model.
It writes one run status per combination and never downloads models. `AUDIT_TEXTS_JSON`
overrides the direct runner's fixture array; `AUDIT_POCKET_STEPS` and
`AUDIT_POCKET_TEMPERATURE` select validated benchmark settings, without changing
server defaults. The grid is serialized; model startup is outside per-request
RTF. Resource/memory attribution and perceptual review require separate measurement.

`score_asr.py MANIFEST TARGETS_JSON ASR_JSONL NEW_OUTPUT` accepts explicitly
provided offline ASR results (schema in its docstring), requires an ASR model and
revision, checks exact target hashes, joins by request UUID and reports NFC,
case-folded, punctuation-separated WER/CER with insertions/deletions/substitutions.
Accents remain significant. It explicitly flags `hola test` for a `test` request
and requires manual review of suspect clips; it never turns ASR guesses into a
passing content assertion. No production Python or ASR dependency is added.

Local baseline references use the already documented eSpeak NG synthetic clip
in `benchmarks/pocket/results/2026-10-15-bc250/`; no human recording is included.
It does not contain `hola`. Its exact transcript and provenance are recorded in
that directory's `summary.json`. The synthetic Spanish pair in `fixtures/` supplies both the containing-`hola`
and non-containing-`hola` cases without a human recording.

For alternating references, explicitly set `AUDIT_ALT_REFERENCE_WAV` and (Qwen)
`AUDIT_ALT_REFERENCE_TRANSCRIPT`. Four extra requests run A/B/A/B at fixed seed
with reference hashes and independent voice IDs. A and B never come from generated
audio. `fixtures/README.md` identifies the synthetic `hola`-containing fixture.
The manifest includes the executable SHA-256 and ORT build, so runtime git HEAD
alone is not mistaken for a precise inference-build identity.

Active native cancellation regression (no pre-cancelled substitute):

```
POCKET_TTS_BUNDLE_DIR=models/pocket-tts POCKET_TTS_REF_WAV=benchmarks/pocket/results/2026-10-15-bc250/reference-espeak.wav cargo test --no-default-features --features pocket-tts --lib real_bundle_request_rng_and_active_cancellation_are_isolated -- --ignored --nocapture
QWEN_GGUF_MODEL_DIR=models/qwen3-tts-1.7b-gguf QWEN_REFERENCE_WAV=AUTHORIZED.wav QWEN_REFERENCE_TRANSCRIPT='exact transcript' cargo test --no-default-features --features qwen-gguf --test qwen_gguf_artifacts native_worker_reuses_conditioning_and_recovers_after_cancellation -- --ignored --nocapture
```

Each watcher waits for at least one real inference frame before cancellation and
fails if that never happens. Pocket also checks continued random streams across
identical chunks, deterministic request restart, and fresh decoder histories.

The optional offline ASR helper is `transcribe_whisper.py LOCAL_MODEL AUDIT_ROOT
NEW_JSONL`. Development environment used: faster-whisper 1.2.1, CTranslate2 4.8.3,
PyAV **16.0.1** (PyAV 19 fails this faster-whisper decoder API), CPU INT8,
2 threads, beam 5, forced fixture language, no VAD filtering and no previous-text
conditioning. Model: `Systran/faster-whisper-tiny` revision
`d90ca5fe260221311c53c58e660288d3deb8d356`; actual binary SHA-256 is recorded in
every ASR row. Download this development-only model explicitly before running;
the helper only opens local files. No ASR or Python package enters SonicBoom's
Cargo dependency graph or default CI.

`summarize_grid.py GRID_ROOT TARGETS_JSON ASR_JSONL NEW_CSV` aggregates pinned ASR
scores and audio health/RTF for each language/step/temperature. These are noisy
ASR diagnostics on synthetic reference voices. Timings include concurrent audit
load and are not controlled speed benchmarks. Manual naturalness, speaker
similarity and suspect-clip listening remain separate gates.

### Pocket independent exporter comparison

`pocket_export_fixture.py PINNED_RUNTIME MODEL_ROOT NEW_JSON` calls the pinned
exporter's actual code on deterministic synthetic 24 kHz PCM, temperature zero
and LSD=1; it does not reimplement its generator. It records tokenizer/chunks,
latents and decoder samples for all six bundles. `fixtures/pocket-export.json`
uses Python ORT 1.31; `pocket-export-ort128.json` records ORT 1.28. Both differ
from the shipped static ORT build numerically. Do not describe those failed
cross-build comparisons as passing runtime parity.

For controlled parity, install `requirements-offline.txt`, use its Python ORT
shared library, and explicitly run:

```
POCKET_TTS_BUNDLE_DIR=models/pocket-tts ORT_DYLIB_PATH=/absolute/path/to/venv/lib/python3.14/site-packages/onnxruntime/capi/libonnxruntime.so.1.28.0 cargo test --no-default-features --features pocket-tts,ort/load-dynamic --lib real_bundles_match_pinned_exporter_prompts_states_and_decoder -- --ignored --nocapture
```

The test remains a hard failure with a mismatched runtime build. This opt-in
comparison does not alter the production feature defaults or add a production
Python dependency. The Unicode fix affects byte-fallback text. The parameter
grid corpus contained no byte-fallback pieces in any language (see
`pocket-grid-final/byte-fallback-audit.json`), so its recorded grid remains useful
for those targets; it does not represent post-fix Unicode speech measurements.

`AUDIT_SEED=<decimal-u64>` overrides the runner's default 42 and is recorded in
each row. This also reproduces the exact decimal seed shown by Qwen/Pocket job
diagnostics. To explore another seed, use a new output directory; never overwrite
an earlier run. Fixed-seed comparisons must keep model/runtime build and device
identical.

Earlier Pocket manifest rows with `mode: null` recorded an omitted request mode;
the provider's validated effective default was `audio_prompt`. New runner requests
set it explicitly. Null Pocket frame/cache/EOS/chunk fields mean those metrics
were not exposed by the provider return type, not fabricated values. Full detailed
per-chunk trace plumbing remains an open audit enhancement; the independent
exporter comparison records actual latent frame counts and complete decoder data.

### Runner knobs for quality and sampling

The `tts-quality-audit` binary accepts additional environment overrides. Each
row now carries a `quality` object (audio-health measurements, plus echo analysis
when ASR is supplied) and the effective `sampling_preset`:

- `AUDIT_SAMPLING_PRESET=balanced|stable|expressive|custom` — select a Qwen
  sampling preset for the whole run (default `balanced`, the pinned baseline).
- `AUDIT_QUALITY=off|report|retry|experimental_trim` — quality pass mode
  (default `report`; the runner records evidence, it never performs trimming).
- `AUDIT_DEVICE=cpu|vulkan|cuda` — explicit native device for the Qwen worker.
  CPU/Vulkan comparisons require separate runs into separate fresh output
  directories; never overwrite an earlier run.
- `AUDIT_ASR_JSON='["transcript 1", ...]'` — one offline ASR transcript per
  text, aligned by index, enabling reference-echo confirmation. This is
  benchmark-only; the production request path never runs ASR.

`AUDIT_ALT_REFERENCE_WAV` / `AUDIT_ALT_REFERENCE_TRANSCRIPT` still drive the
A/B/A/B reference alternation for boundary experiments. The runner writes hashed
inputs only and never logs raw reference audio or transcript text.
