# Native Android validation

Validated on an M2102J20SG running Android 16, ARM64/Bionic, with native Termux
Rust 1.99.1 and Clang 21.1.8. The server build used
`--no-default-features --features server,android-ort`; no Node.js, GUI, desktop
audio, proot, Docker or Python inference runtime was involved.

| Check | Native result |
| --- | --- |
| Official Android ORT 1.24.3 / API 24 | Load, initialize, run/destroy/recreate real ONNX sessions, normal exit: passed |
| Supertonic 3 | Both HTTP synthesis APIs, audible 44.1 kHz mono PCM16 WAV: passed |
| Pocket English | Real reference-conditioned synthesis, audible 24 kHz mono WAV: passed |
| Qwen CPU | Compile, link `libc++_shared.so`, discover CPU backend, reject missing/corrupt GGUF: passed |
| Core Rust tests | 359 passed, four tests explicitly ignored |
| Android Clippy | All targets with `-D warnings`: passed (upstream ORT API-24 dependency warnings remain) |
| Optional-provider and TikTools builds | `cargo check --all-targets`: passed |
| CI/release helper tests | 26 CI helper tests and 10 release helper tests: passed |
| Desktop and Android NDK CI | Jobs preserved/added; not executed on this Android device |

The full server integration also passed password persistence, JSON admin login,
CSRF/token minting, incorrect bearer authentication, authenticated configuration,
port-conflict rejection, invalid voice, oversized input, concurrent requests,
background-job cancellation/deadlines, synchronous client disconnect, SIGTERM,
restart with persistent tokens, and Ctrl+C. API-only mode retains health,
readiness and JSON setup APIs while omitting HTML/static routes.

An idempotent rebuild also passed with `CARGO_NET_OFFLINE=true`, reusing the
verified installed runtime and cached Rust dependencies.

The [cached-model offline test](results/android16-supertonic-offline.json) also
passed synthesis and restart while download traffic was routed to a closed
loopback proxy. This validates reuse of installed assets; it is not kernel-level
network isolation.

Runtime failure tests cover missing libraries, invalid ELF, wrong machine type,
and Android libraries without the ORT C API. An isolated test-only preload shim
injects ENOSPC into the real initialization process: it returns a clear error,
without panic or a committed partial configuration. No filesystem is filled.

## Runtime shutdown fix

The first release build exposed an upstream `ort` exit-order crash **after**
successful inference. The vendored Android `__cxa_atexit` fix was validated first
with an isolated native Rust session probe, then with the actual SonicBoom
release binary and running-server tests. Normal process exit is part of the
check. See [the patch description](../../vendor/ort/SONICBOOM-PATCH.md).

The auxiliary mock quality-audit command now records that ORT is unused instead
of loading a native runtime solely for manifest metadata. Its regression test
passes with `ORT_DYLIB_PATH` deliberately pointing to a nonexistent library.
The installed LLVM linker crashed once during overlapping debug/release builds;
the complete native test suite passed when retried serially.

## Measured Supertonic run

[Full 20-request report](results/android16-supertonic-warm.json), with two
intra-op CPU threads and no compiler processes running during synthesis:

| Measurement | Result |
| --- | --- |
| Cached model readiness | 6.99 s, including cache verification and startup checks |
| Observed loading phase | Approximately 2 s; measured by readiness/status polling |
| First synthesis | 6.31 s |
| Median synthesis | 6.38 s |
| Synthesis range | 6.13–7.50 s |
| Audio duration per request | 2.21 s |
| Median real-time factor | 2.88 |
| Sampled peak RSS | 471 MiB |
| Peak process thread count | 14 |
| Overall CPU use | 160.7% of one core |

RSS decreased 38.6 MiB between the first and last synthesis samples. The last
five samples varied by about 1.4 MiB; no steady growth was observed in this run.
Long-duration thermal and memory stress testing remains outstanding.

Device power/thermal state was not controlled after prolonged native builds.
The earlier [cold-cache functional run](results/android16-supertonic-cold.json)
measured 1.94–2.25 s synthesis while compilation ran concurrently. Preserve both
reports; they describe different device conditions and do not establish an
optimization comparison or a performance guarantee.

Additional evidence:

- [Pocket English synthesis](results/android16-pocket-en.json).
- [Qwen CPU linking/backend/error checks](results/android16-qwen-backend.json).

Qwen real-model speech synthesis, physical disk-full testing and Android Vulkan
execution remain unvalidated. No Android GPU acceleration is claimed.

## PR review follow-up (2026-10-22)

Eight isolated wrapper regression tests passed. These exercise explicit binary
selection (including missing, empty and non-executable paths), mandatory default
runtime integrity, custom-runtime opt-in, idempotent installation, explicit
upgrades, staged self-test failure, runtime validation failure and invalid
arguments. Package/build/ELF tools are stubbed for these tests; they never replace
the real installed executable. Failed upgrades preserve the previous contents
and remove staging files.

Separately, native `run.sh --build-info` verified the official runtime digest and
completed the actual ONNX inference self-test. The [review follow-up HTTP run](results/android16-review-regressions.json)
passed cached Supertonic synthesis through both APIs, the explicit invalid-voice
HTTP 400 response and message, concurrency/cancellation/deadline checks, and
shutdown/restart. Downloads were routed through a closed proxy, with the same
network-isolation limitations described above. Three repeated synthesis requests
were used for functional verification, not a new stability benchmark.
