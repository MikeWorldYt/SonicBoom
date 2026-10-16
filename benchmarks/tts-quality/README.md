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
