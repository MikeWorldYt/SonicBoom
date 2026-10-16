# Release pipeline repair: validation report

Validation performed 2026-10-14 (America/Lima), on CachyOS Linux x86_64,
Rust 1.99.0. This is an implementation/validation report, not a claim that a
Windows release or a new GitHub Release has been validated or published.

PR: https://github.com/hernan-lc/SonicBoom/pull/1

## Confirmed root causes

The v0.0.4 Windows job log explicitly reports `esaxx.o` with
`MT_StaticRelease` conflicting with `knfc.obj` with `MD_DynamicRelease`,
followed by LNK2038, LNK4098 and LNK1319. Tests fail before release compilation.
The matrix had default fail-fast behavior, cancelling Linux and skipping the
publish job. Cargo declares `SonicBoom`, but the release workflow requested
`sonicboom`/`sonicboom.exe`. Raw artifact uploads, recursive checksum paths and
no explicit missing-file failure also made packaging/publication unreliable.
The GitHub API confirmed v0.0.1 through v0.0.4 have no binary assets.

`cargo tree --locked --features gui -i esaxx-rs` confirms the shared patched
esaxx-rs 0.1.10 serves tokenizers 0.20.4 (retired GPT-SoVITS) and 0.22.2 (Qwen).
knf-rs-sys 0.3.2 defaults KNF_STATIC_CRT to false. The vendor provenance hashes
confirm build.rs is the only modified original esaxx file. Both static_crt
calls use false; no linker suppression or provider/test removal was used.
Native Windows linking still requires execution on a Windows runner.
Source review also found Windows-rooted/drive-relative asset paths could pass
an is_absolute-only guard and fail the existing absolute-path regression test.
The guard now rejects RootDir/Prefix components, with additional Windows cases.
The initial Ubuntu Docker attempt exposed two match guards unsupported by its
pinned Rust 1.89; equivalent ordinary matches retain provider behavior without
upgrading the toolchain or dependencies.

ORT's locked Linux CPU distribution links static libonnxruntime.a. Windows
selects a DirectML-capable static distribution and imports DirectML/D3D12/
DXGI/DXCore even with CPU inference; runtime documentation and test DLL lookup
account for this. Native ORT 1.28.0 licenses/notices are pinned and packaged. Static inspection
of the SHA-256-verified Windows ORT artifact confirmed DirectML.dll 1.15.4,
byte-identical to Microsoft's NuGet DLL; its distinct license/notices are
preserved, and packaging verifies that DLL's hash. Its import table references
D3D12 and Windows API-set libraries; this inspection does not execute Windows
code or establish DLL resolution on a Windows host.

## Checks observed to pass

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --locked --features gui --all-targets -- -D warnings` | Pass |
| `cargo clippy --locked --all-targets -- -D warnings` | Pass |
| `cargo test --locked --features gui` | 347 passed, 6 existing tests ignored, 0 failed |
| `cargo test --locked` | 344 passed, 6 existing tests ignored, 0 failed |
| `cargo test --locked --no-default-features --features server` | 296 passed, 5 existing tests ignored, 0 failed |
| `cargo build --release --locked --features gui --target x86_64-unknown-linux-gnu` | Pass |
| Python packaging/publishing safety tests | 6 passed |
| actionlint 1.7.12 on both workflows | Pass |
| Existing Dockerfile/Compose structure and supply-chain shell guards | Pass |
| Vendor original-file SHA-256 comparison | Only build.rs differs |
| Ubuntu 24.04 CPU Docker build, pinned Rust 1.89 | Pass after replacing unsupported match guards and supplying libclang |
| Ubuntu CPU container runtime probe | Nonroot user, resolved shared libraries, ORT initialization, health 200, no models, graceful exit 0 |
| Actual Linux tar.gz archive inspection | Nonempty executable, mode 0755, documentation and licenses; no user models/configuration/caches |
| Repeat packaging from identical inputs | Byte-identical archives (`cmp`) |
| Actual Linux archive SHA-256 generation and verification | Pass (`sha256sum --check`) |
| Extracted Linux package native runtime probe | All `ldd` dependencies resolve, explicit ORT initialization succeeds |
| Extracted Linux package HTTP probe | `/health` returns 200 and `OK`; no model files downloaded |
| Extracted Linux package shutdown | Exits 0 after the bounded diagnostic lifetime via normal listener shutdown |

The local Docker client uses the legacy builder, so Ubuntu validation used a
temporary Dockerfile consisting of the exact `builder` and final `runtime`
stages, omitting unrelated playback/CUDA stages. Both stage instructions came
unchanged from the repository Dockerfile. This validates the CPU container,
not the playback/CUDA images or the hosted GUI release job.

The actual archive was tested on the local CachyOS host. The workflow specifies
Ubuntu 24.04 for production Linux packages; hosted Ubuntu validation remains
pending, so a local binary is not evidence of Ubuntu ABI compatibility.
Synthetic ZIP fixtures test Windows archive structure/determinism/casing and
checksum guards; they are **not** a compiled Windows application.
The final two-platform SHA256SUMS generation and publication require both real
platform packages. The implementation deliberately refuses a partial set.

## Failed or externally blocked checks

* `cargo audit --deny unmaintained --deny unsound` fails with four rustls
  0.23.45 advisories (RUSTSEC-2026-0346 through RUSTSEC-2026-0349), unmaintained
  fxhash 0.2.1 (RUSTSEC-2025-0057), and unsound rand 0.7.3
  (RUSTSEC-2026-0097). Existing yanked-crate warnings are also reported.
  These are unchanged locked dependencies. rustls is not selected in the local
  default+GUI graph; fxhash comes via jieba, and rand 0.7.3 via
  arpabet's PHF build dependencies. No new audit exceptions were added, and
  unrelated dependencies were not silently upgraded. The audit gate remains
  red and must be resolved deliberately before release approval.
* Fork PR Actions requires maintainer approval. The source-account manual
  validation run is also unavailable due account/billing constraints reported
  by the user, who requested leaving those settings alone. No account settings
  or tokens were changed. Windows GUI tests, CRT link validation, Windows
  release compilation, native DLL resolution and ZIP startup/shutdown remain
  unverified. No successful hosted Ubuntu result is claimed.
* Model-dependent tests remain explicitly ignored by the existing suite.
  Real Supertonic/Qwen/Pocket TTS inference, GPU execution, physical playback
  and interactive tray behavior were not certified by a model-free probe.
* No merge, new tag, release upload or published-asset API validation has
  occurred. Existing tags/releases remain unchanged. Published-asset checks
  are implemented in the tag workflow and regression-tested with fixtures.

## Files and release procedure

The changes are grouped into logical commits: Cargo.toml/Cargo.lock and
vendor/esaxx-rs fix native linkage; Dockerfile/.dockerignore preserve native
build inputs and exclude private context; src/main.rs adds a bounded diagnostic
startup; provider asset guards reject Windows-rooted paths and provider error
matches remain compatible with Rust 1.89; scripts/release owns deterministic packaging, native probes, checksum
and publication verification; both GitHub workflows add independent native
builds and safe publication; LICENSE, native notices, README and release docs
explain distribution and runtime requirements.

After merging into **hernan-lc/SonicBoom**, resolve the audit and hosted-run
blockers, require both native platforms to pass, and run the manual Release
workflow to inspect packages without publishing. Then create a fresh
`v0.0.5` tag at the verified merged commit in that repository. The repository's
GITHUB_TOKEN stages a draft, verifies all three assets by downloading them,
then publishes and checks them again. Do not reuse v0.0.4 or overwrite assets.
See [RELEASING.md](RELEASING.md) for exact requirements and rollback instructions.
