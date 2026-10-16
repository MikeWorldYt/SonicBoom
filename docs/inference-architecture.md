# Native inference ownership and cache policy

SonicBoom remains Rust-native: Qwen 1.7B uses GGUF (llama.cpp) plus ONNX
Runtime, Pocket TTS and Supertonic use ONNX Runtime. No Python/PyTorch
runtime or model subprocess is introduced. The llama.cpp dependency is a
pinned submodule; the GGUF/ONNX bundle revision is recorded in
`src/tts/providers/qwen_gguf/models.rs`. (The former Candle Qwen backend
and its vendored dependency were removed.)

## Ownership and lock ordering

HTTP handlers reserve admission before cloning/decoding references. Pending
requests wait asynchronously in the shared semaphore queue; its acquire future
is retained to preserve FIFO ordering. Running permits are moved into blocking
native work. Admission/reference preparation permits are shared leases retained
by actual blocking preparation and encoding, so dropping an HTTP future cannot
release capacity while that work continues.

Qwen owns its model and prompts on one dedicated thread, with a two-command
bounded channel and one-message bounded replies. Worker reply waits are off
Tokio executors, emit a watchdog warning after 30 seconds during controlled
API inference, and retain permits
until acknowledged computation ends. Cancellation/deadlines reach the vendored
generation loop, including individual acoustic predictions. Panics in synthesis
retire the damaged model; lost workers can be replaced. Release builds unwind
panics so the job owner can report terminal failure. Normal inference errors and
cancellation preserve healthy shared weights.

Provider lock order is lifecycle -> heavyweight construction resource ->
model/worker -> short state/config locks. Conditioning and synthesis are one
serialized transaction. Job-store locks never cover awaits. Voice-store locks
are acquired only on blocking threads; a bounded two-second advisory file lock
serializes manifest mutations across server instances.

Large optional model construction is serialized across providers. Each provider
loads once, and retired Qwen workers are joined before replacement. A failed
Qwen load does not poison construction for Pocket TTS. Keep sufficient free RAM
for every enabled resident model; this is not an automatic offloading policy.

## Configuration versions and reloads

The server snapshots configuration and revision together. ProviderPool rejects
older versions, preventing an old request from reverting a newer model choice.
Busy reconfiguration is recorded and retried by one coordinator; active model
work finishes before retirement. Background loaders compare model identity
(model directory, model ID/revision, enablement and device), reject obsolete
candidates, and do not discard a compatible load for a sampling/cache-only
change. Maximum-frame/cache/sampling changes preserve loaded weights.

Providers are serialized: `providers.qwen3.max_concurrent` and
`providers.pocket_tts.max_concurrent` must be **1**. Older values that implied
parallel model execution were never effective; set them to 1 before upgrading.
Global admission can still allow distinct providers to run concurrently.

## Disk verification

SHA-256 is computed with a 256 KiB buffered reader and 64 KiB scratch buffer,
on blocking threads, rather than allocating an entire multi-GB file. Verification
progress reports actual processed bytes. Downloaded bytes must match both the
pinned digest and exact size before atomic installation; staging is cleaned on
errors/dropped futures. Linux process-tagged staging files left by dead processes
are reaped without deleting another live server's downloads/reference staging.
Legacy staging without a process tag is not blindly deleted.

Private verification records contain format version, pinned source/revision
URL, expected digest/size and verified file identity. On Unix, an unchanged
regular file's device/inode/size/mtime/ctime (including nanoseconds), together
with owner-private metadata, permits skipping repeat hashing. Changed, corrupt,
untrusted or missing records trigger pinned hashing. Platforms without that
identity policy conservatively stream-hash. Metadata is an optimization of a
prior pinned verification, not trust on first use. No tokens or voice audio are
written into these records. Native offline loads use the same integrity pins.

## Voice caches

The GGUF/ONNX backend (`qwen3-gguf`, opt-in build) mirrors the same ownership
shape: one native worker thread owns the GGUF contexts, ONNX sessions,
embeddings and tokenizer; requests cross as plain data; `Drop` joins the
owner thread so cancellation never releases the model mid-computation. Its
reference cache keys on backend id, pinned model revision, voice identity,
reference hash, transcript and cloning mode, and is cleared at safe
boundaries on voice invalidation. GPU initialization failures fall back to
CPU before any synthesis, and the fallback reason travels in provider status.

Each native worker promotes conditioning on preparation/synthesis access and
evicts least recently used entries. Entries are bounded by configured count (1..=1024)
and 64 MiB. Identity includes provider, reviewed implementation/model revision,
model ID, decoded reference hash, sample-rate handling, transcript and cloning
mode. Profile-scoped keys isolate voices; ownership is checked before cache
retrieval. An eagerly prepared identical prompt can be rebound without encoding
again. Deletion logically invalidates the profile and retires artifacts at a
safe worker boundary.

The server caches one manifest plus a decoded-reference LRU (16 entries/64 MiB).
Manifest and WAV identities detect changes, and create/delete operations keep
invalidation coherent. Profile directories are owner-private; manifests and
reference writes remain atomic. Pocket TTS retains only its last conditioned
profile/reference under its serialized model transaction and invalidates that
reuse on changes/deletion. Its native data is replaced at the next safe reference
preparation rather than unloading healthy weights.

## Instrumentation and profiling

Authenticated `/api/providers` includes actual selected device, model residency,
load count and Qwen conditioning counters. Jobs expose measured stage durations,
frames, elapsed time, timestamps and actual provider/model/voice/device context.
Logs identify request paths and job IDs, not request bodies, query transcripts,
credentials or recordings. No provider logs raw reference audio or transcripts
independently of configured verbosity.

Qwen logs prefill, acoustic prediction, talker, sampling and tokenizer decoding
costs, generated frames, mean frame latency, EOS/max-frame reason, audio duration
and RTF. Audio encoding executes off the async executor. Reproducible Rust
benchmark tools and measurements are in `benchmarks/providers.md`; ordinary CI
never needs multi-GB model downloads. The generation loop has EOS and frame
limits; slowness is not an infinite loop. A max-frame result is explicitly
withheld, with an actionable error, instead of silently truncating speech.


### Reproducible profiling commands

```sh
cargo run --release --no-default-features --features qwen --example cache_benchmark -- streaming models/qwen3-tts/model.safetensors
# The cache example checks the pinned 0.6B model digest.
QWEN_MODEL_DIR=./models/qwen3-tts \
  /usr/bin/time -v cargo run --release --no-default-features \
  --features qwen-gguf,pocket-tts --example tts_benchmark -- all
# Linux perf needs host permission and symbols; profile the built binary:
perf record -g target/release/examples/tts_benchmark all
perf report
```

Use the exact pinned model, build features, owned reference, text, seed and
thread settings for comparisons. Run without concurrent compilation or other
inference, repeat runs, and capture `/usr/bin/time -v` plus JSON results.
Do not compare the historical two-CPU audit numbers with this machine as if
only software changed.


## Baseline revalidation

Latest main at inspection was `0a350b162249f9e8d5973e8471b8ac3fcbf54124`,
the same revision as the audit. The default 120-second response timeout,
absence of native Qwen cancellation/progress, blocking worker replies,
whole-file optional-model hashing, insertion-order worker prompt eviction,
repeated reference decoding, optional-provider reload races, CPU-only Auto,
ignored hardware tests, and misleading provider concurrency knobs were
present. The shared HTTP gate already retained its permit in blocking work;
this refactor preserves that ownership and adds true cooperative stopping.
The generic `ConditioningCache` already implemented LRU, but Qwen's actual
worker prompt cache did not use its access promotion. Supertonic already had
a versioned manager; optional providers needed corresponding safeguards.
Qwen already stopped on EOS or a maximum frame count; it was not an infinite
loop. Real-model tests remain opt-in to keep ordinary CI independent of weights.
