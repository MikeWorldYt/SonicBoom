# Remediation verification measurements (model-independent)

Branch work for SB-001–SB-020 at `af87794` baseline. No pinned model weights
(Supertonic 3, Pocket TTS, Qwen3-TTS) are present on this machine and no audio
output device exists, so every real-model and hardware number below is
explicitly NOT RUN. What *was* measured: parity proofs (byte/token/selection
identity between old and new code paths), allocation-bounding tests, lock and
ownership stress tests, and micro-timings. See `evidence.jsonl` for the raw
records.

Machine: AMD BC-250, 12 online CPUs, 14 GiB RAM, Ubuntu, rustc 1.99.0.
Build: debug test profile, default features (`server`, `playback`,
`qwen-gguf`, `pocket-tts`), CPU ORT. Timings are single cold runs in debug
mode — order-of-magnitude evidence only, not release benchmarks.

## Measured (this machine)

- Rate-limiter burst (SB-017): 50k distinct-key `check()` calls in
  77.25 ms total (~1.5 µs/op, debug). Contention immaterial; no change made.
- Opus streaming encoder (SB-010): byte-identical output to the two-phase
  encoder across 110 cases (11 lengths × 5 sample rates × silence/signal),
  plus Ogg page-walk granule checks (last granule 312 + frames×960 on the
  EndStream page; headers-only for empty input).
- Qwen sampler parity (SB-007): 2,250 paired selections across tied/varied
  logits × 5 sampling configs × 3 seeds, plus 200 extreme-logit selections
  and bounded-history duplicate evolution — all identical, histories equal.
- Supertonic duration validation (SB-002): 5,000 in-range durations derive
  lengths identical to the legacy formula; NaN/±inf/negative/zero/huge/
  boundary/overflow cases rejected before allocation.
- Playback ownership races (SB-001): 1,000 deterministic interleavings, zero
  orphans, zero foreign deletions, zero early deletions of live audio.
- Voice-cache identity (SB-015), job retention accounting (SB-014),
  sampler/codec/cache unit tests: all passing (see `cargo test` below).

## NOT RUN (missing assets/hardware)

- Real-model RTF, latency, peak RSS, seed parity, voice quality, output
  hashes: no Supertonic/Pocket/Qwen weights on this machine. Do not download
  multi-GB weights in ordinary CI; run `tests/qwen_gguf_artifacts.rs`-gated
  cases where pinned assets exist.
- Playback gap over 100 clips, idle CPU/wakeups on hardware, USB
  disappearance/recovery: headless, no ALSA device. The idle-wait policy is
  covered by unit tests on the wait computation, not by device measurement.
- Windows path/file-deletion behavior: Linux only here.
- CUDA/ROCm/Vulkan builds: no GPU SDKs; `--all-features` fails in the native
  llama.cpp build script (pre-existing environment limitation).

## Reproduce

```sh
cargo fmt --all -- --check
cargo clippy --all-targets  # zero warnings on supported targets
cargo test                  # lib + bins (default features)
cargo test --lib --no-default-features --features pocket-tts
cargo test --lib --no-default-features --features qwen-gguf
cargo test --no-default-features --lib
cargo test --no-default-features --features tiktools-plugin --bin sonicboom-tiktools-plugin
cd frontend && npm ci && npm test && npm run typecheck && npm run build
```

Pre-existing failures, identical on pristine `af87794`: 3
`engineCard.test.mjs` SSR assertions (activation buttons/loading label).
`tests/qwen_gguf_artifacts.rs` cases ignore without model artifacts.
