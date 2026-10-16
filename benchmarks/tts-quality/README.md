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

Pending gates (NOT RUN): real-model ordered warm/cold sequence, actual native
cancellation/error recovery, concurrent HTTP callers, authorized alternating
references, ASR WER/CER, listening review, parameter grid and GPU comparisons.
Local model directories exist; presence alone does not prove usable assets or
reference consent. No speech-quality parity or changed sampling defaults claimed.

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
