# Releases and runtime requirements

Release binaries are CPU builds with default features plus `gui`: Supertonic,
Qwen3-TTS, Pocket TTS, playback, HTTP server and tray support are retained.
Models are downloaded separately on normal startup; no credentials, user
configuration, downloaded voices/models or caches belong in release archives.

## Supported release targets

* Linux x86_64: built/tested on Ubuntu 24.04 (glibc 2.39). Install
  `libasound2t64 libssl3t64 libstdc++6 libgcc-s1 ca-certificates` using apt.
  Other distributions need equivalent libraries and a compatible glibc; they
  are not separately certified. The tray uses a D-Bus StatusNotifier host;
  a missing host falls back to server operation. Launch `./sonicboom` to
  include any packaged ONNX shared libraries in the loader search path.
* Windows x86_64: Windows 10 (2004 or newer)/11 with the current Microsoft Visual C++
  2015–2022 x64 Redistributable (https://aka.ms/vs/17/release/vc_redist.x64.exe).
  The pinned Windows ORT static distribution also links DirectML/D3D12/DXGI/
  DXCore imports even though SonicBoom selects CPU inference. The archive must carry the pinned DirectML 1.15.4 DLL and its
  separate Microsoft redistribution terms. Keep the DLL with the executable; do not infer GPU support from those imports.
  Launch `SonicBoom.exe`. CI uses Windows Server 2022; consumer desktop
  tray interaction and physical audio devices still need manual checks.
* macOS is not a release target. The source has Apple feature gates, but no
  macOS archive is advertised until native CI, signing/notarization, runtime
  dependencies and hardware testing are established.

These archives are portable application bundles, **not fully standalone**.
System libraries, the Windows redistributable, certificates, audio devices,
and models remain external requirements. Packaging carries Cargo-copied native
DLL/SO files and dependency license files. `ldd` on Linux, PE import inspection
on Windows, and isolated startup with a restricted Windows DLL search PATH
check native loading. The model-free probe explicitly initializes ONNX Runtime.
The locked CPU ORT distribution may link statically; do not invent a DLL
requirement if the produced executable has no ONNX dynamic import. If future
ORT builds use shared libraries they must appear beside the built executable
(via `ort/copy-dylibs`), be packaged with their notices, and pass the probe.

GPU acceleration is not enabled in these packages. CUDA/ROCm/CoreML builds
require separate SDKs, provider libraries, driver compatibility and hardware
validation. Qwen CUDA is an independent opt-in feature; Pocket TTS uses CPU
ORT even if other provider acceleration is selected. CI does not certify
model inference quality, GPU execution, tray interaction or audio playback.

## Create a release

1. Merge the release-fix PR into the repository that will own the release
   (`hernan-lc/SonicBoom` for this repair). Require green CI on both platforms.
2. Run the Release workflow manually on the merged commit. Manual runs build,
   test, package, inspect and smoke-test both platforms and upload workflow
   artifacts, but do not publish a GitHub Release.
3. Review those artifacts. Use a new version, recommended `v0.0.5`; never
   reuse or force-move `v0.0.4`. Create and push the new tag at that merged
   commit to **hernan-lc/SonicBoom**, not another remote by accident.
4. The tag workflow tests GUI before release compilation on each native runner.
   Fail-fast is disabled. Both jobs must succeed before publishing. The publish
   job checks the tag is on `main`, refuses a release that already carries
   assets or notes (an empty placeholder release left by publishing a new tag
   through the web UI is removed so the draft can be staged), downloads only
   the two packages, generates and verifies SHA256SUMS, creates a draft, verifies
   the downloaded draft assets against checksums through the API, then publishes
   it with generated release notes. It verifies published assets again.
5. Confirm exactly these assets are present: `sonicboom-linux-x86_64.tar.gz`,
   `sonicboom-windows-x86_64.zip`, `SHA256SUMS`. Download and verify them.

Publishing uses the workflow repository's `GITHUB_TOKEN` and `contents: write`
only in the publish job. No PAT or cross-repository upload is needed. Concurrency
serializes the same tag without cancelling a release already running. Existing
releases/assets are never overwritten automatically: only a release with no
assets and no notes is discarded and rebuilt, so creating the release (and with
it the tag) through the GitHub web UI no longer blocks the workflow. A release
that already carries assets or notes always fails the publish job. If a draft
is left after failure, inspect its assets and logs; delete that unpublished
draft deliberately before retrying. Do not blindly rerun over an existing
production release.

## Local packaging and checksum verification

Use the same target as the native host (MSVC Windows cannot be validated by
Linux cross-compilation):

```sh
cargo test --locked --features gui --target x86_64-unknown-linux-gnu
cargo build --release --locked --features gui --target x86_64-unknown-linux-gnu
python scripts/release/package.py --target x86_64-unknown-linux-gnu
python scripts/release/smoke.py dist/sonicboom-linux-x86_64.tar.gz
```

Substitute `x86_64-pc-windows-msvc` and the `.zip` name on Windows. Python 3.12+
is required. With both packages present:

```sh
python scripts/release/package.py --checksums
python scripts/release/package.py --checksums --verify
cd dist
sha256sum -c SHA256SUMS
```

Windows PowerShell: compare `(Get-FileHash .\sonicboom-windows-x86_64.zip
-Algorithm SHA256).Hash` with the corresponding entry in `SHA256SUMS`.
Checksums cover the **final archive bytes**, not temporary extracted files.
Archive entry order, ownership, modes, gzip timestamps and ZIP timestamps are
normalized. Identical inputs yield identical archive bytes on the same packaging
toolchain; this does not claim bit-for-bit Rust builds across toolchain versions
or hosts. BUILD-INFO.json records the commit, target, features and rustc version.

`SonicBoom --smoke-test` is a diagnostic mode: use an isolated
`SONICBOOM_CONFIG` directory, loopback bind, and temporary working directory.
It starts the real server without the tray/browser or model downloads,
initializes native ORT, serves HTTP for 15 seconds and exits gracefully. It
rejects non-loopback configuration. This is not model inference validation.
The packaging probe enforces startup/health and shutdown deadlines and kills
any remaining child on failure. It never uses your personal config or models.

## Troubleshooting

* LNK2038 / LNK4098 / LNK1319 mentioning `MT_StaticRelease` versus
  `MD_DynamicRelease`: verify `cargo tree --locked --features gui -i esaxx-rs`
  points at `vendor/esaxx-rs`. Both tokenizers versions use the same patch.
  Delete stale target outputs or use a fresh target directory. Do not set
  `KNF_STATIC_CRT=1`, `+crt-static`, `/NODEFAULTLIB` or suppress the mismatch.
  See `vendor/esaxx-rs/README.sonicboom.md` for upstream provenance and removal
  criteria. Native Windows test and release links are the regression checks.
* Missing DLL / 0xc0000135: install the x64 MSVC redistributable, extract the
  whole archive, inspect the printed PE imports, and keep supplied DLLs beside
  SonicBoom.exe. Do not copy arbitrary DLLs from another machine.
* Missing `.so`: run `ldd ./SonicBoom` and on each bundled SO, install the
  documented OS packages, and use the `./sonicboom` launcher. Packaging/smoke
  failures must be fixed before publishing; never mask `not found` output.
* Model-dependent failure: inspect model trust/revision, disk/network access,
  provider enablement and logs separately. A healthy model-free server does
  not prove successful speech synthesis. Exercise all three providers with
  real models and reference audio before claiming full inference validation.

## Rollback

Mark a faulty published release as prerelease or remove its public release
entry after recording the incident. Leave its Git tag immutable. Recommend
installation of the previous verified version and restore user data from a
backup if a migration occurred. Revert the change through a PR and publish a
new tag after validation. Never replace existing assets silently: checksums and
consumer downloads must continue to identify the same bytes.

### Native and container preflight

Initialize sources with `git submodule update --init --recursive`, then run
`python3 scripts/ci/preflight.py`. It checks the checked-out Gitlink revisions,
explicit Cargo examples, local path manifests and each Docker builder's manifest
inputs and feature names. CI runs it before compilation on Linux and Windows.
Default release binaries include all providers with CPU backends. Vulkan and
native CUDA remain explicit `qwen-gguf-vulkan` / `qwen-gguf-cuda` opt-ins and
require their SDKs; ONNX Runtime's `cuda` feature selects a separate stack.

Docker's CPU and playback targets select their features explicitly. The CUDA
target selects both ONNX Runtime CUDA and native GGML CUDA. The locked
ort-sys rc.13 distribution ships CUDA 13 only, so the CUDA images use matching
CUDA 13/cuDNN runtime and development bases; a CUDA 13 compatible host driver
is required. CPU images retain their independent SDK-free build. Its pinned toolkit
builds the portable llama.cpp architecture set without requiring a GPU during
compilation. At runtime use NVIDIA Container Toolkit (`docker run --gpus all`):
`libcuda.so.1` must come from the host driver. Image checks permit only that
specific driver library to be absent and never ship linker stubs. Hardware
inference still requires a supported GPU and installed models.

`TOKEN_STORE_PATH=/app/data/tokens.json` in Docker/Compose is a storage path,
not an access token. Keep it writable by the non-root `sonicboom` user and mount
persistent data as needed. Docker's name-based secret warning does not indicate
embedded credentials here; do not place access tokens in build arguments or
image environment defaults. Model weights, credential files, nested Git data
and local build caches are excluded from the allowlisted build context.

Native GGML CUDA builds disable multi-GPU NCCL collectives by default
(`GGML_CUDA_NCCL=OFF`). SonicBoom runs a single service and does not need NCCL
for single-GPU inference; leaving it enabled made the static `ggml-cuda`
archive reference NCCL symbols the Rust link never resolved. Set
`SONICBOOM_CUDA_NCCL=1` to opt in (build.rs then links the discovered NCCL
library). `SONICBOOM_CUDA_ARCHITECTURES` overrides the compiled architecture
set; unset keeps the full release set, and CI smoke builds may narrow it to cut
NVCC time.

### CI structure and CUDA validation

Ordinary push/pull-request CI (`.github/workflows/ci.yml`) never runs the full
CUDA Docker build. It runs formatting, Clippy, tests, the security-regression
manifest guard, the feature matrix, native CPU/Vulkan GGUF builds, Linux and
Windows GUI packaging, Docker CPU/playback images and a fast *static* CUDA
configuration check (`scripts/ci/cuda_structure.py`) that invokes neither NVCC
nor Docker. That static check proves the CUDA feature names, Docker stages,
digest-pinned bases, native CMake/NCCL policy and runtime packaging are wired
correctly; it does not prove GPU inference works.

The full native CUDA compile and CUDA Docker image build live in the dedicated
`.github/workflows/cuda-full.yml` workflow, triggered manually, weekly, and on
release tags. It compiles the native backend, verifies NCCL/static linking and
shared-library resolution, builds the CUDA image with a layer cache, and runs
GPU runtime tests only when a self-hosted CUDA runner is attached. Release CUDA
artifacts are not treated as validated unless those full checks pass.

Use `python3 scripts/ci/container_smoke.py sonicboom:cpu` (or
`sonicboom:playback`) to check real HTTP health and graceful shutdown without
model downloads. The production images set `BIND=0.0.0.0`, so the probe starts
the container with an explicit `BIND=127.0.0.1` override; the application
deliberately refuses `--smoke-test` on a non-loopback listener.

