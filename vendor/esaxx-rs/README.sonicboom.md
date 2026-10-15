# SonicBoom MSVC runtime patch

Vendored from the crates.io esaxx-rs 0.1.10 distribution, Apache-2.0.
Original LICENSE and source notices are preserved. upstream-source.sha256.json
records original file hashes. Only build.rs is modified: both static_crt(true)
calls become static_crt(false), matching Rust MSVC defaults, knf-rs-sys 0.3.2
and ONNX Runtime. Do not enable +crt-static for production Windows builds.

Upstream fix: https://github.com/Narsil/esaxx-rs/pull/19 (open on 2026-10-14).
Both tokenizers 0.20.4 (GPT-SoVITS) and 0.22.2 (Qwen) use this patch.
Remove the override only after a reviewed upstream release fixes CRT selection;
update the lockfile deliberately and run native Windows GUI tests and builds.
Never hide mismatches with /NODEFAULTLIB or /FORCE.
