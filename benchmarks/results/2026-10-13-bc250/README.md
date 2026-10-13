# BC-250 raw measurements

These JSON records contain measurements from native Rust release examples,
not private reference recordings. See ../../providers.md for hardware, pinned
revisions, build flags, CPU utilization and interpretation limits.

Ordinary validation on Linux:

- `cargo fmt --all -- --check`: passed.
- `cargo check --workspace --all-targets`: passed.
- `cargo test --workspace`: 118 library + 225 server + 1 integration passed;
  1 hardware/engine unit test and 5 real-model tests are ignored ordinarily.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `node --check static/index.js`: passed.
- `cargo check --workspace --all-targets` and `cargo test --workspace` with
  `--no-default-features`, then individual `server`, `qwen`, `gpt-sovits`,
  and Linux `gui` feature combinations: passed.

No CUDA toolkit/NVIDIA device or interactive browser surface was available.
CUDA builds/runtime and RTX 3060/VRAM remain pending. Firefox headless layout
renders were inspected at 390×844 and 1440×1000; interactive flow and keyboard
accessibility testing remain pending. Other operating systems were not tested. Progress is not incremental
cloned-audio streaming. Native kernel cancellation waits for a checkpoint;
GPT-SoVITS cancellation is supported between sentences.


All five gated real-model cases passed across separate completed runs:
GPT English synthesis/model reuse; Qwen active cancellation then healthy reuse;
Qwen load; Qwen Japanese ICL; Qwen English x-vector. The first combined process
was interrupted before finishing ICL, so the last three were rerun separately.
Native cancellation acknowledgement was 44.66 ms. Unseeded GPT produced long
60.3-second outputs for short sentences; see the quality risk in providers.md.
Regression runs overlapped compilation and are not performance comparisons.


The final release HTTP smoke test used a one-second response timeout:
initial acceptance 0.792 ms; cancellation after frame 1 observed within
0.254 s (including polling); a new job completed in 6.605 s and returned
0.72 s / 34,604 bytes of WAV with `private, no-store`. Disconnecting the
SSE connection did not cancel that job. Qwen remained ready/resident with
model load count 1, one conditioning miss and five hits across the smoke runs.
The Python standard-library HTTP harness was test tooling only; inference
ran entirely in the Rust server. Headless renders used isolated test profiles.
