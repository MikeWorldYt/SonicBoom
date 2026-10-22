# Native Android ARM64 / Termux

Build directly in Termux; no root, proot, distribution, Docker, desktop,
Node.js, Python inference process, PulseAudio or ALSA is needed.
Android is selected by Rust's `target_os = "android"`, not CPU architecture.

## Install and initialize

From the repository checkout:

```sh
./scripts/termux/install.sh
# Enter a strong password without putting it in shell history.
read -r -s -p 'Admin password: ' SONICBOOM_ADMIN_PW; echo
export SONICBOOM_ADMIN_PW
sonicboom init --headless
unset SONICBOOM_ADMIN_PW
./scripts/termux/run.sh
```

To update an existing installation after pulling the desired revision, run
`./scripts/termux/install.sh --upgrade`. Without this flag a different installed
binary is preserved. The installer stages the new executable in `$PREFIX/bin`,
verifies its contents and runs its ONNX self-test before an atomic rename.
A failed validation leaves the installed executable intact.

`SONICBOOM_BINARY=/path/to/SonicBoom ./scripts/termux/run.sh` selects that exact
executable. A missing, empty or non-executable explicit path is an error; only
an unset variable permits fallback from the installed binary to the local build.

`read -s -p` above assumes Termux's interactive Bash. Alternatively export
`SONICBOOM_ADMIN_PW` using your shell's secure input mechanism. There is no
default password. Initialization stores it in the existing permission-restricted
secret store. Avoid passing credentials on command lines or writing them to logs.

The default listener is `127.0.0.1:17842`, with local authentication. Keep this
binding for local applications. LAN access must be deliberately configured with
`server.auth_mode = "token"` and valid bearer tokens using the existing admin
session/token API. Existing remote-bind validation remains authoritative.
API-only admin login and token-management responses are JSON, including CSRF
session data; browser pages require `web-ui`.

```sh
sonicboom config path
sonicboom config list
sonicboom config get server.port
sonicboom config set server.port 17842
sonicboom config validate
sonicboom config effective
sonicboom config status
sonicboom --build-info
env -u LD_LIBRARY_PATH sonicboom --onnx-self-test
env -u LD_LIBRARY_PATH sonicboom doctor
```

Configuration resolves `SONICBOOM_CONFIG`, portable executable-adjacent config,
then `$XDG_CONFIG_HOME/sonicboom/config.toml` or `$HOME/.config/sonicboom/config.toml`.
First-run paths for logs, models and audio are outside the checkout. Keep this
persistent directory across upgrades. Existing model verification and atomic
configuration/download handling are reused.

## Build profiles

Desktop defaults retain GUI, web assets, playback, Pocket and Qwen/Vulkan.
Other profiles use `--no-default-features`:

| Features | Purpose |
| --- | --- |
| `server` | API only; no frontend compilation |
| `server,web-ui` | API plus embedded Vue assets (Node required at build time) |
| `server,android-ort` | Termux CPU Supertonic |
| `server,android-ort,pocket-tts` | Optional Pocket CPU provider |
| `server,android-ort,qwen-gguf` | Optional Qwen CPU provider |
| `server,playback` | Desktop playback queue |

```sh
./scripts/termux/build.sh
# Optional builds, never automatically enabled/downloaded:
SONICBOOM_TERMUX_PROVIDER=pocket ./scripts/termux/build.sh
SONICBOOM_TERMUX_PROVIDER=qwen ./scripts/termux/build.sh
```

The default command is:

```sh
cargo build --release --locked --no-default-features \
  --features server,android-ort --bin SonicBoom
```

The scripts default to two build jobs (`CARGO_BUILD_JOBS=1` is useful on devices with less RAM), check the native Rust host and compiler, and leave global Rust settings
alone, and refuse to overwrite a different installed executable/runtime.
Move an older executable aside deliberately before replacing it. `PREFIX` is
required only by Termux scripts; their interpreter is Android’s `/system/bin/sh`,
so they do not depend on a conventional `/usr/bin` layout; no Termux installation path is hardcoded.

## ONNX Runtime provenance and ABI

`Cargo.lock` pins `ort` and `ort-sys` **2.0.0-rc.13**. Desktop retains the crate's
default API 27 distribution strategy. Android explicitly disables those defaults,
selects **API 24**, and dynamically loads **ONNX Runtime 1.24.3**. Without this
API selection, rc.13's implicit loader rejects the older native library.

The installer obtains Microsoft's full Android AAR from Maven Central:

<https://repo.maven.apache.org/maven2/com/microsoft/onnxruntime/onnxruntime-android/1.24.3/onnxruntime-android-1.24.3.aar>

SHA-256:
`67397e4a970e75617f765d2015ceaf911917e1d822276cfb5792744e8085cbce`

Only `jni/arm64-v8a/libonnxruntime.so` is extracted. The Java JNI wrapper is
unnecessary. The archive is cached under `$XDG_CACHE_HOME/sonicboom/runtime`
(or `$HOME/.cache/sonicboom/runtime`). The library is installed under
`$PREFIX/lib/sonicboom/libonnxruntime.so`; `ORT_DYLIB_PATH` overrides it.
The extracted library SHA-256 is
`4d2318b3849abb8862133d3068fc7e807ed8b2671cc6d83657fff2fcb9e1caad`.
An existing verified library is reused even when the archive cache is absent.

Every wrapper validation, including startup, verifies the pinned library's
SHA-256. For an intentionally custom runtime, set both `ORT_DYLIB_PATH` to a
separate path and `SONICBOOM_ALLOW_CUSTOM_ORT=1`. This skips only the digest pin;
ELF/ABI validation and the startup inference self-test still apply. The default
`$PREFIX/lib/sonicboom/libonnxruntime.so` always requires the pinned digest,
even with the custom-runtime option enabled.

The shell validator checks ELF64/AArch64, Android dependencies and `OrtGetApiBase`,
rejecting typical glibc dependencies. The executable's fallible explicit loader
then checks runtime/API compatibility; a real session test proves more than
an ELF header check. An arbitrary malicious native library is not made safe by
these checks: use the verified artifact.

The repository vendors a narrow `ort` rc.13 Android shutdown fix. Native testing
found that upstream's `.fini_array` environment cleanup ran after the Android
runtime's C++ destructors, causing a Bionic destroyed-mutex abort after successful
inference. Android now registers the existing Apple-style `__cxa_atexit` callback
after environment creation. Desktop cleanup behavior is unchanged. See
[`vendor/ort/SONICBOOM-PATCH.md`](../../vendor/ort/SONICBOOM-PATCH.md).

No runtime download happens at server startup. After runtime/model installation,
normal operation works offline. First use of Supertonic downloads the existing
SHA-256-pinned model set; optional providers download only when explicitly enabled.

Official references:
- [Rust ort rc.13 feature definitions](https://github.com/pykeio/ort/blob/v2.0.0-rc.13/Cargo.toml)
- [ort dynamic loading and version checks](https://github.com/pykeio/ort/blob/v2.0.0-rc.13/src/lib.rs)
- [Microsoft Android builds](https://onnxruntime.ai/docs/build/android.html)

## Native validation

```sh
# Runtime: load, initialize, create session, execute Identity graph, check float
# values, release session; repeat in the same process. No network/model download.
sonicboom --onnx-self-test
# Python is only an integration-test driver, never an inference dependency:
pkg install python
./scripts/termux/test.sh --smoke-only
./scripts/termux/test.sh --report "$HOME/sonicboom-termux-results.json"
# After weights are cached, verify downloads are unnecessary:
./scripts/termux/test.sh --offline --report "$HOME/sonicboom-termux-offline.json"
# Longer fixed-input memory sampling:
./scripts/termux/test.sh --repetitions 20 --report "$HOME/sonicboom-termux-memory.json"
```

The full integration test initializes an isolated configuration and secret store,
finds an available loopback port, verifies APIs, waits for actual readiness,
executes Supertonic through both TTS APIs, validates nonempty/non-silent PCM WAV,
checks invalid runtime/voice failures, concurrent synthesis, cancellation and deadlines, then verifies SIGTERM, restart and Ctrl+C. It also injects ENOSPC into a child process writing an isolated temporary directory; no filesystem is filled.
Its Supertonic cache is reusable outside the checkout. A `/ready` 503 is never
counted as inference success. The JSON report includes Android version, model
readiness time (including downloads on a cold cache), three synthesis latencies,
audio duration, real-time factors, RSS/high-water memory and thread count where
Android permits `/proc` inspection. Model-ready time is not pure model-load time.

For controlled measurements, warm the download cache, record device model,
thermal/battery state and CPU utilization with Android/Termux tools. Compare
RSS across extended repetitions; the short integration probe cannot prove the
absence of long-term native leaks. `SONICBOOM_CPU_THREADS=1..32` controls Supertonic
intra-op threads on Android (default 2); no synthesis quality setting is reduced.
The existing inference gate defaults to one running and eight pending requests.

CI's Android NDK job checks compilation, feature dependencies and runtime ELF ABI.
It does **not** execute Android inference. Native execution and performance results
must be recorded separately. Pocket cloning and Qwen inference require their own
explicit models/reference-audio tests; compilation does not establish synthesis
compatibility. Android Vulkan is not validated or enabled by these scripts.

## Lifecycle and troubleshooting

Ctrl+C and SIGTERM use the existing graceful Axum shutdown. Android can kill
background processes without delivering a signal; battery exemption and
`termux-wake-lock` may help but do not guarantee uninterrupted operation. No root
is required. Preserve model/config directories when restarting.

- Missing runtime: run the installer or set `ORT_DYLIB_PATH` to the verified library.
- Wrong architecture/glibc library/API: replace it with the pinned Android AAR library.
- CMake missing C++ symbols: avoid parent-app private C++ libraries. Build and runtime wrappers clear
  `LD_LIBRARY_PATH` and use Termux tool RUNPATHs: otherwise Termux's liblzma can shadow the Android system
  copy required by libandroid/libunwindstack. No global settings change.
- Occupied port: choose another port using `config set`; headless mode fails binding
  instead of silently moving to another port.
- Incomplete API-only setup: use `init --headless` with a valid bootstrap password.
- Model failure: inspect `/api/status`, `/api/providers` and logs; readiness stays 503.

The existing gate/control tests cover queue limits, cancellation and deadlines;
Supertonic model corruption under offline startup, physical disk-full conditions,
long-running memory/thermal behavior and Qwen real-model synthesis still require
explicit native fault-injection/hardware tests. Do not treat the short probe as
coverage of every failure mode.

## Optional provider diagnostics

After generating a mono PCM16 reference WAV with Supertonic, explicitly test one
Pocket bundle (English by default) without fetching other languages:

```sh
unset LD_LIBRARY_PATH
export POCKET_TTS_BUNDLE_DIR="$HOME/.cache/sonicboom/models/pocket-tts"
export POCKET_TTS_REF_WAV="$HOME/termux-reference.wav"
cargo run --release --locked --no-default-features \
  --features android-ort,pocket-tts --example pocket_smoke -- --download
```

The reference must be authorized audio; synthetic Supertonic output is suitable
for a compatibility probe. Omit `--download` for an offline run. The example
checks finite, audible output and releases the provider. Set `POCKET_TTS_MODEL`
only to a supported model ID and provide text appropriate for that language when
adapting the probe. Only English is used by the default probe.

For Qwen, the existing `qwen_gguf_smoke` example uses `QWEN_GGUF_MODEL_DIR`,
`QWEN_REFERENCE_WAV`, `QWEN_REFERENCE_TRANSCRIPT` and `QWEN_GGUF_DEVICE=cpu`.
The 1.7B provider needs substantially more memory than Supertonic/Pocket; measure
available RAM and context/load peaks before attempting it. A successful native
build is not a claim of successful Qwen inference on a memory-constrained phone.

Run native Rust tests with a clean loader path: `env -u LD_LIBRARY_PATH cargo test --locked --no-default-features --features server,android-ort`. A parent application can inject private C++ libraries that break compiler tools; putting all Termux libraries on the runtime loader path can also shadow Android system dependencies.

A model-free Qwen CPU link/backend probe is available with `env -u LD_LIBRARY_PATH cargo run --locked --no-default-features --features android-ort,qwen-gguf --example qwen_backend_probe`. It also rejects missing/corrupted GGUF files without loading real weights.

Native device results and remaining validation limits are recorded in [VALIDATION.md](VALIDATION.md).

## API compatibility note

An explicitly unknown Supertonic voice on `POST /api/tts` now returns HTTP 400
with `unknown Supertonic voice`, instead of synthesizing with the default voice.
Omitting `voice` still selects the configured default. Existing valid voice IDs
are unchanged. The native HTTP integration suite checks this error response.

Wrapper regression checks run without package installation or model downloads:
`python3 scripts/termux/test_wrappers.py`. They use isolated fake build tools;
real native inference and HTTP validation remain in `scripts/termux/test.sh`.
