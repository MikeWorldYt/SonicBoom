# Pocket TTS quality investigation

Status: partial quality investigation, 2026-10-15. Language support was expanded in a follow-up (see below). Comparative WAVs and timing results are available, but no listening evaluation has
been collected; no quality improvement, preset ranking, quantization loss, or
numerical parity is established by this change.

## Reproducible CPU runs

Hardware inspected: AMD BC-250, AuthenticAMD family 23/model 71, 6 cores/12 threads.
Build without playback, server, or Qwen features:

```sh
cargo build --release --example pocket_benchmark --no-default-features --features pocket-tts
POCKET_BENCH_STEPS=1 target/release/examples/pocket_benchmark models/pocket-tts /path/to/consented-mono-pcm16.wav /path/to/results/steps1 'Hello world, this is a test of voice cloning on a CPU.' > /path/to/results/steps1.jsonl
```

Repeat in fresh processes with `POCKET_BENCH_STEPS=2`, `4`, and `8`; omit the
variable to run all four in one process. Fresh processes are necessary for
meaningful cross-step RSS comparison. Each process records model load time,
one cold-conditioning run and three warm runs, fixed seed 49420, unchanged
0.7 temperature and 4 CPU threads. Every trial writes float32 mono WAV.
`process_peak_rss_kib` is Linux process VmHWM, includes loading and all previous
trials, and is not incremental model memory. `api_ttfa_seconds` measures complete
synchronous synthesis; it is not streaming or first decoder frame latency.
RTF = synthesis wall seconds / output duration. Do not compare only RTF when
output lengths differ.

Record commit, compiler/ORT versions, CPU governor, CPU affinity, background
load, reference SHA256/sample rate/consent basis, text bytes, model revision and
all model hashes beside results. Use multiple texts, references, and seeds for
quality conclusions; this runner's single seed is only a reproducible probe.
Do not commit private references or cloned audio without permission.

## Source verification

Export revision: `58a6d00cf13d239b6748cb0769f35c580a8f606c` in
[KevinAHM/pocket-tts-onnx](https://huggingface.co/KevinAHM/pocket-tts-onnx/tree/58a6d00cf13d239b6748cb0769f35c580a8f606c).
Kyutai source inspected at `ec569b2dd3fec4a601b391aace185791e92b3785`:
[flow_lm.py](https://github.com/kyutai-labs/pocket-tts/blob/ec569b2dd3fec4a601b391aace185791e92b3785/pocket_tts/models/flow_lm.py).
LSD evaluates `(s=i/n, t=(i+1)/n)` then adds `flow/n`.
Noise standard deviation is sqrt(temperature). Upstream also supports OT and
drifting samplers; these cannot be selected interchangeably for an LSD export.
Keep `lsd_steps=1` until checkpoint-specific comparative evidence supports a
change. Fast/balanced/quality names are deliberately not assigned yet.

[September English config](https://github.com/kyutai-labs/pocket-tts/blob/ec569b2dd3fec4a601b391aace185791e92b3785/pocket_tts/config/english_2026-09.yaml)
specifies `tokenizer.json` using the tokenizers backend, recommended temperature
0.3, BOS before voice and 24 kHz output. Current April loading uses
`tokenizer.model` with a custom unigram implementation. A filename/checkpoint
substitution cannot establish compatibility. The pinned export tree contains
April English, French 24l, German, Italian, Portuguese, Spanish and several 24l
variants; no September English or Dutch bundle is present there. Listing files
is not validation of tokenizer IDs, graph signatures, sampler or acoustic quality.
At the initial audit no new language was advertised; the follow-up below records subsequent verification.

April English has sibling FP32 flow-main, flow-head and Mimi decoder graphs at
the pinned revision. These are candidates for an identical-checkpoint ablation;
the production loader still selects pinned INT8 graphs. FP16 equivalence is
unverified. Do not substitute root-level legacy graphs for language-local graphs.

## Required parity and quality evidence still outstanding

1. Pin the original checkpoint and export provenance, verify every downloaded
   member by SHA256, and retain all existing license/access checks.
2. Export stage fixtures from both Kyutai and Rust: prepared UTF-8 text, token
   IDs, reference PCM, voice embeddings, BOS/prefill tensors, every flow state,
   conditioning vector, initial noise, each integration state, latent frames,
   EOS logits and stopping index, and decoded PCM with shapes/dtypes.
   Equal seeds are insufficient: PyTorch, NumPy and Rust StdRng use different
   generators. Feed identical explicit noise tensors for stage parity; also
   report native seeded end-to-end runs separately. Compare exact token and
   integer states; report max absolute/RMS/relative errors, NaN masks and first
   divergent stage for floats. Set justified tolerances before accepting results.
3. Compare LSD 1/2/4/8 using identical inputs/noise; compare INT8/FP32 from the
   same checkpoint and export pipeline. Vary only one factor at a time.
4. Compare existing linear reference resampling with a documented band-limited
   implementation at 16/22.05/44.1/48 kHz. Measure alias rejection with synthetic
   tones, then embedding differences, speaker similarity and listening results.
   Keep 24 kHz identity samples identical. Do not change shared audio resampling.
5. Blind/randomize reference-plus-A/B listening; preserve raw WAVs without
   normalization that hides clipping. Report preference and uncertainty, ASR
   WER/CER against exact texts and pinned ASR version, speaker cosine similarity
   against the same authorized references and pinned embedding model. Neither
   audibility nor a single automated score establishes quality improvement.
6. Validate language-local tokenizer fixtures and ONNX input/output/state
   contracts before adding per-language hashed manifests and UI options. Keep
   existing IDs and voice profiles; never reinterpret an existing language's
   profile as another checkpoint. Dutch remains experimental and unavailable
   until an exact compatible bundle is verified.

## Regression checks

```sh
cargo test --lib --no-default-features --features pocket-tts pocket_tts -- --skip real_bundle
POCKET_TTS_BUNDLE_DIR=./models/pocket-tts cargo test --lib --no-default-features --features pocket-tts real_bundle_matches_phase_a_reference -- --ignored
```

The real synthesis test requires both `POCKET_TTS_BUNDLE_DIR` and
`POCKET_TTS_REF_WAV`; missing variables now fail explicitly instead of reporting
success without inference. Its audibility assertion is a smoke test, not a
quality or upstream parity test. The phase-A fixture checks token IDs and bundle
metadata only. Full stage parity remains outstanding.

## Recorded AMD baseline

[Results](results/2026-10-15-bc250/summary.json) and
[listening page](results/2026-10-15-bc250/listen.html) retain a locally generated
eSpeak NG 1.52.0 default English synthetic reference and one WAV per step count.
The complete four-trial WAV sets remain at the absolute paths in JSONL.

| LSD steps | Warm median RTF | Warm API TTFA (s) | Process peak RSS (MiB) |
| --- | ---: | ---: | ---: |
| 1 | 1.0035 | 3.1309 | 936.2 |
| 2 | 0.9493 | 3.1136 | 932.9 |
| 4 | 1.0664 | 3.4978 | 954.2 |
| 8 | 1.1337 | 3.7186 | 945.3 |

This is one synthetic speaker, one text, one seed, three warm repetitions,
sequential runs without affinity/governor control. Output duration differs:
3.12 s at one step and 3.28 s otherwise. Do not infer a quality ranking or a
speed advantage for two steps from these small, uncontrolled samples.

`resampling_probe.py` compares a NumPy mirror of Rust interpolation with SciPy
polyphase Kaiser beta=5. The 15 kHz tone after 48→24 kHz conversion measured
approximately 0 dB with linear interpolation versus -57.53 dB with polyphase;
44.1→24 kHz measured -3.12 versus -57.23 dB. This establishes synthetic
anti-alias behavior only, not voice similarity or speech quality. Timings are
Python implementation timings and must not be presented as Rust throughput.

Validation on this AMD CPU: 90 library tests passed, 3 ignored; the explicitly
run real-bundle tokenizer fixture and English/Spanish reference synthesis smoke
test passed separately. Release benchmark built and all 16 trials completed.

Inspected call sites: provider inference/cache/load/manifest paths; configuration
model, loader allowlist, validation and schema; provider API mapping and pool;
voice profile reference/consent handling; capability registry; frontend settings
translations. OpenAI alias and TikTools call sites were inspected read-only.
No configuration migration is needed for the isolation fix: IDs, profile files,
defaults, pinned hashes, and consent/license checks remain unchanged. Requests
whose model and language disagree now return an explicit error; requesting a
different resident bundle requires configuring/loading it first.

## Verified language expansion

Portuguese (`pocket-tts-pt`, `pt`), German (`pocket-tts-de`, `de`), Italian
(`pocket-tts-it`, `it`) and French (`pocket-tts-fr-24l`, `fr`, 24-layer preview)
now use language-local pinned manifests at the same immutable export revision.
All eight files per language were downloaded and checked by size and SHA256.
All four bundles passed native CPU synthesis with the retained synthetic
reference and exact token-ID comparison against Python SentencePiece 0.2.2:
nine cases per bundle including accents, combining marks, whitespace, newlines,
punctuation, emoji and out-of-language byte fallback. See
[verification and hashes](results/2026-10-15-languages/verification.json) and
[smoke sample audio](results/2026-10-15-languages/listen.html).
These checks establish compatibility and finite audible output, not quality.
French's 24-layer preview is more expensive and is named explicitly.
Chinese/Japanese/Dutch remain unsupported because the pinned repository has
no exact bundles for them. No unverified model is routed to another language.

Regenerate independent token fixtures (requires SentencePiece):

```sh
python benchmarks/pocket/tokenizer_fixtures.py models/pocket-tts > benchmarks/pocket/fixtures/languages.json
POCKET_TTS_BUNDLE_DIR=./models/pocket-tts POCKET_TTS_REF_WAV=benchmarks/pocket/results/2026-10-15-bc250/reference-espeak.wav POCKET_TTS_COMPARE_DIR=/tmp/pocket-language-audit/audio cargo test --lib --no-default-features --features pocket-tts real_bundle_new_languages_match_tokenizers_and_synthesize -- --ignored --nocapture
```

The token fixture generator reads local pinned artifacts; it never downloads or
accepts replacement models. The opt-in test runs the production integrity checks
before tokenizer and native inference validation. Model weights are not checked
into the repository. Existing provider/profile IDs remain valid. Conditioning
cache keys now include model identity so references cannot cross language bundles.
The clone API rejects model/language mismatches before storing a profile. Pocket
model and clone language dropdowns expose only the selected model's language;
other providers retain their existing language behavior.

Server regression note: the lean `server,pocket-tts` binary suite has an existing
OpenAI test that unconditionally expects Qwen support and fails when Qwen is
compiled out (190 other tests passed). The Pocket-specific profile rejection
regression passed in that lean build. The normal server suite passed all 192 tests with
`server,pocket-tts,qwen-gguf`; no OpenAI/Qwen source changes were made. The lean
library suite passed 92 tests (4 opt-in tests ignored), and the separate new-language
native verification test passed all four bundles. Frontend production build passed.
