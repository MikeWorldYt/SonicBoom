//! Feature-resolution and SDK-preflight regression guards for the native GGUF
//! build.
//!
//! These read the manifests/build script as data and never invoke Cargo, so
//! they are cheap and cannot deadlock against the outer `cargo test`. They
//! lock down the rules that keep the two acceleration stacks distinct:
//!
//!   * the root `default` set keeps the core providers and never selects more
//!     than one mutually-exclusive ONNX execution provider;
//!   * ONNX Runtime features (`cuda`/`rocm`/`coreml`) never reach the native
//!     llama.cpp crate;
//!   * only `qwen-gguf-cuda`/`qwen-gguf-vulkan` forward to
//!     `sonicboom-qwen-llama/{cuda,vulkan}`;
//!   * the native build script validates requested SDKs before CMake runs.
use std::path::Path;

fn manifest(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// Dependency/feature list of `name = [ ... ]` inside a manifest, tolerant of
/// single-line and multi-line arrays. Entries are unquoted and trimmed.
fn feature_list(manifest: &str, name: &str) -> Vec<String> {
    let marker = format!("\n{name} = [");
    let start = manifest
        .find(&marker)
        .unwrap_or_else(|| panic!("feature `{name}` not declared"))
        + marker.len();
    let end = start + manifest[start..].find(']').expect("unterminated array");
    manifest[start..end]
        .split(',')
        .map(|entry| entry.trim().trim_matches('"').to_string())
        .filter(|entry| !entry.is_empty())
        .collect()
}

#[test]
fn root_defaults_keep_core_providers_and_one_onnx_execution_provider() {
    let default = feature_list(&manifest("Cargo.toml"), "default");
    for required in ["server", "playback", "qwen-gguf", "pocket-tts", "gui"] {
        assert!(
            default.contains(&required.to_string()),
            "`default` unexpectedly dropped `{required}`: {default:?}"
        );
    }
    // `cuda`/`rocm`/`coreml` are mutually-exclusive ONNX execution providers;
    // a default build must never select two at once.
    let onnx_eps: Vec<&str> = ["cuda", "rocm", "coreml"]
        .into_iter()
        .filter(|feature| default.contains(&feature.to_string()))
        .collect();
    assert!(
        onnx_eps.len() <= 1,
        "`default` selects conflicting ONNX execution providers: {onnx_eps:?}"
    );
    // NOTE: a native `qwen-gguf-vulkan`/`qwen-gguf-cuda` backend MAY be enabled
    // in `default` by maintainer choice. It is intentionally not forbidden
    // here, but doing so makes the matching GPU SDK a hard build requirement
    // for everyone (CI/Docker included). The CPU-only configuration is
    // `--no-default-features --features qwen-gguf`; see docs/providers.md.
}

#[test]
fn onnx_gpu_features_never_enable_native_llama_backends() {
    let root = manifest("Cargo.toml");
    for (feature, expected) in [
        ("cuda", "ort/cuda"),
        ("rocm", "ort/rocm"),
        ("coreml", "ort/coreml"),
    ] {
        let deps = feature_list(&root, feature);
        assert!(
            deps.contains(&expected.to_string()),
            "`{feature}` must enable `{expected}`: {deps:?}"
        );
        assert!(
            !deps.iter().any(|dep| dep.contains("sonicboom-qwen-llama")),
            "`{feature}` is an ONNX-only flag and must not activate the native llama.cpp crate: {deps:?}"
        );
    }
}

#[test]
fn native_backends_are_opted_into_only_by_explicit_features() {
    let root = manifest("Cargo.toml");
    assert_eq!(
        feature_list(&root, "qwen-gguf-vulkan"),
        vec!["qwen-gguf", "sonicboom-qwen-llama/vulkan"]
    );
    assert_eq!(
        feature_list(&root, "qwen-gguf-cuda"),
        vec!["qwen-gguf", "sonicboom-qwen-llama/cuda"]
    );
}

#[test]
fn native_crate_backends_are_independent_and_off_by_default() {
    let native = manifest("native/qwen-llama/Cargo.toml");
    assert_eq!(
        feature_list(&native, "default"),
        Vec::<String>::new(),
        "the native crate must not enable a backend by default"
    );
    assert!(!feature_list(&native, "vulkan").contains(&"cuda".to_string()));
    assert!(!feature_list(&native, "cuda").contains(&"vulkan".to_string()));
}

#[test]
fn build_script_validates_sdks_before_cmake_and_is_reconfigurable() {
    let build = manifest("native/qwen-llama/build.rs");
    for needle in [
        "cfg!(feature = \"vulkan\")",
        "cfg!(feature = \"cuda\")",
        "GGML_VULKAN",
        "GGML_CUDA",
        // SDK discovery + the documented bypass must stay wired up.
        "CUDAToolkit_ROOT",
        "CUDACXX",
        "VULKAN_SDK",
        "SONICBOOM_SPIRV_INCLUDE_DIR",
        "SONICBOOM_SKIP_SDK_CHECK",
    ] {
        assert!(
            build.contains(needle),
            "native build.rs must keep `{needle}` so GPU requests fail early and reconfiguration is deterministic"
        );
    }
}
