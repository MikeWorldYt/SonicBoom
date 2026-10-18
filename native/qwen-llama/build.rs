//! Build script for the SonicBoom-owned llama.cpp wrapper (`sonicboom-qwen-llama`).
//!
//! Backend selection is *explicit* and *validated before CMake runs*, so a
//! requested GPU backend whose SDK is missing or incomplete produces an
//! actionable error naming the required tool, instead of an opaque
//! `ggml-cuda`/`ggml-vulkan` compile failure deep inside the vendored tree.
//!
//! Rules this script enforces:
//!   * CPU GGUF builds stay the dependable default and need no GPU SDK.
//!   * `vulkan`/`cuda` Cargo features opt into exactly one llama.cpp backend
//!     each. ONNX Runtime's `cuda`/`rocm`/`coreml` features never reach here.
//!   * Requesting both backends is allowed only when *both* SDKs validate;
//!     otherwise the build stops early naming the missing one.
//!
//! `SONICBOOM_SKIP_SDK_CHECK=1` bypasses the pre-flight scan for unusual
//! toolchains where CMake can still discover an SDK this script cannot.

use std::path::{Path, PathBuf};

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

fn target_is_windows() -> bool {
    std::env::var("TARGET")
        .map(|t| t.contains("windows"))
        .unwrap_or(cfg!(windows))
}

/// `true` when `program` is an executable file directly inside one of `dirs`.
fn program_in(dirs: &[PathBuf], program: &str) -> Option<PathBuf> {
    dirs.iter()
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

/// Resolve an executable the way CMake's `find_program` would: an explicit
/// directory list first, then every `PATH` entry.
fn find_program(dirs: &[PathBuf], program: &str) -> Option<PathBuf> {
    if let Some(found) = program_in(dirs, program) {
        return Some(found);
    }
    let path = std::env::var_os("PATH")?;
    program_in(&std::env::split_paths(&path).collect::<Vec<_>>(), program)
}

/// `true` when `<root>/<relative>` is a readable file for any root in `roots`.
fn file_in_any(roots: &[PathBuf], relative: &str) -> Option<PathBuf> {
    roots
        .iter()
        .map(|root| root.join(relative))
        .find(|candidate| candidate.is_file())
}

/// Prefixes a CMake `find_package(... CONFIG)` search would consider for
/// installed SDK layouts, in addition to explicit environment variables.
fn prefix_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(prefixes) = std::env::var_os("CMAKE_PREFIX_PATH") {
        roots.extend(std::env::split_paths(&prefixes));
    }
    if let Some(sdk) = env_path("VULKAN_SDK") {
        roots.push(sdk);
    }
    if let Some(home) = env_path("HOME") {
        roots.push(home.join(".local"));
    }
    for fixed in ["/usr", "/usr/local", "/opt"] {
        roots.push(PathBuf::from(fixed));
    }
    roots
}

/// Locate an include directory that satisfies one of ggml-vulkan's SPIR-V
/// header layouts (`spirv/unified1/spirv.hpp`, `spirv-headers/spirv.hpp`, or
/// `spirv.hpp`). The pinned llama.cpp revision discovers the SPIRV-Headers
/// CMake package but never links its imported target, so unless the headers
/// happen to sit in a compiler-default directory the build fails. Returning
/// this directory lets the wrapper's CMakeLists add it explicitly.
fn find_spirv_include_dir() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = env_path("SONICBOOM_SPIRV_INCLUDE_DIR") {
        candidates.push(dir);
    }
    for root in prefix_roots() {
        candidates.push(root.join("include"));
        candidates.push(root.join("spirv-headers").join("include"));
    }
    candidates.into_iter().find(|dir| {
        dir.join("spirv/unified1/spirv.hpp").is_file()
            || dir.join("spirv-headers/spirv.hpp").is_file()
            || dir.join("spirv.hpp").is_file()
    })
}

fn fail(message: &str) -> ! {
    panic!(
        "\n\
         ====================================================================\n\
         SonicBoom native GGUF build configuration error\n\
         ====================================================================\n\
         {message}\n\
         ====================================================================\n"
    );
}

fn validate_cuda() {
    let exe = if target_is_windows() {
        "nvcc.exe"
    } else {
        "nvcc"
    };
    let mut dirs: Vec<PathBuf> = Vec::new();
    // CMake's FindCUDAToolkit honours CUDACXX (full path to the compiler),
    // CUDAToolkit_ROOT, CUDA_PATH and CUDA_HOME. Mirror that order.
    if let Some(cudacxx) = env_path("CUDACXX")
        && let Some(parent) = cudacxx.parent()
    {
        dirs.push(parent.to_path_buf());
    }
    for var in ["CUDAToolkit_ROOT", "CUDA_PATH", "CUDA_HOME"] {
        if let Some(root) = env_path(var) {
            dirs.push(root.join("bin"));
        }
    }
    dirs.push(PathBuf::from("/usr/local/cuda/bin"));
    dirs.push(PathBuf::from("/opt/cuda/bin"));
    if target_is_windows()
        && let Some(pf) = env_path("ProgramFiles")
    {
        // The default Windows layout is `.../CUDA/v12.4/bin/nvcc.exe`.
        let toolkit = pf.join("NVIDIA GPU Computing Toolkit").join("CUDA");
        if let Ok(versions) = std::fs::read_dir(&toolkit) {
            for version in versions.flatten() {
                dirs.push(version.path().join("bin"));
            }
        }
    }
    if find_program(&dirs, exe).is_none() {
        fail(&format!(
            "`qwen-gguf-cuda` was requested, but the CUDA Toolkit is missing:\n\
             `{exe}` was not found in CUDACXX, CUDAToolkit_ROOT/CUDA_PATH/CUDA_HOME,\n\
             /usr/local/cuda/bin, /opt/cuda/bin, or PATH.\n\n\
             llama.cpp's ggml-cuda backend needs the CUDA Toolkit to compile.\n\
             Choose one:\n\
             \x20 * CPU-only build:      cargo build --no-default-features --features qwen-gguf\n\
             \x20 * install the toolkit: https://developer.nvidia.com/cuda-downloads\n\
             \x20 * point at existing:   export CUDAToolkit_ROOT=/path/to/cuda\n\
             \x20 * bypass this check:   SONICBOOM_SKIP_SDK_CHECK=1 cargo build ...",
        ));
    }
}

fn validate_vulkan() {
    let keys = prefix_roots();
    let glslc = if target_is_windows() {
        "glslc.exe"
    } else {
        "glslc"
    };
    let mut program_dirs: Vec<PathBuf> = keys.iter().map(|root| root.join("bin")).collect();
    program_dirs.push(PathBuf::from("/usr/bin"));
    if find_program(&program_dirs, glslc).is_none() {
        fail(&format!(
            "`qwen-gguf-vulkan` was requested, but the shader compiler is missing:\n\
             `{glslc}` was not found in $VULKAN_SDK/bin, CMAKE_PREFIX_PATH prefixes, or PATH.\n\n\
             ggml-vulkan compiles SPIR-V shaders at build time and needs glslc\n\
             from the Vulkan SDK (`vulkan-sdk`/`shaderc`).\n\
             Choose one:\n\
             \x20 * CPU-only build:      cargo build --no-default-features --features qwen-gguf\n\
             \x20 * install the SDK:     https://vulkan.lunarg.com/sdk/home\n\
             \x20 * point at existing:   export VULKAN_SDK=/path/to/vulkan-sdk\n\
             \x20 * bypass this check:   SONICBOOM_SKIP_SDK_CHECK=1 cargo build ...",
        ));
    }
    let include_roots: Vec<PathBuf> = keys.iter().map(|root| root.join("include")).collect();
    if file_in_any(&include_roots, "vulkan/vulkan.h").is_none() {
        fail(
            "`qwen-gguf-vulkan` was requested, but the Vulkan development headers are\n\
             missing: `vulkan/vulkan.h` was not found under $VULKAN_SDK/include,\n\
             CMAKE_PREFIX_PATH prefixes, /usr/include, /usr/local/include or /opt/include.\n\n\
             Install the Vulkan loader/headers (Arch: `vulkan-headers`; Debian:\n\
             `libvulkan-dev`; Fedora: `vulkan-loader-devel`) or set VULKAN_SDK.\n\
             CPU-only alternative: `cargo build --no-default-features --features qwen-gguf`.\n\
             Bypass this check: SONICBOOM_SKIP_SDK_CHECK=1.",
        );
    }
    // ggml-vulkan includes <spirv/unified1/spirv.hpp> directly. This pinned
    // llama.cpp revision finds the SPIRV-Headers *config* package but never
    // links its imported target, so the headers must either sit in a
    // compiler-default directory or be wired in explicitly. If we can locate
    // them anywhere (`SONICBOOM_SPIRV_INCLUDE_DIR`, a CMAKE_PREFIX_PATH
    // prefix, `$VULKAN_SDK`, `~/.local[/spirv-headers]`, `/usr`, `/usr/local`,
    // `/opt`) the build proceeds; only a genuine absence fails here, with the
    // exact variable to set.
    if find_spirv_include_dir().is_none() {
        fail(
            "`qwen-gguf-vulkan` was requested, but the SPIRV-Headers development files\n\
             are missing: no include directory with `spirv/unified1/spirv.hpp`,\n\
             `spirv-headers/spirv.hpp` or `spirv.hpp` was found under\n\
             $SONICBOOM_SPIRV_INCLUDE_DIR, $VULKAN_SDK/include,\n\
             ~/.local[/spirv-headers]/include, a CMAKE_PREFIX_PATH prefix,\n\
             or /usr/include, /usr/local/include, /opt/include.\n\n\
             ggml-vulkan #includes that header directly and does not add the\n\
             SPIRV-Headers include directory itself.\n\
             Fix one of:\n\
             \x20 * install SPIRV-Headers (Arch: `spirv-headers`; Debian:\n\
             \x20   `spirv-headers`; Fedora: `spirv-headers-devel`);\n\
             \x20 * or point at an existing copy:\n\
             \x20   export SONICBOOM_SPIRV_INCLUDE_DIR=/path/to/include;\n\
             \x20 * or install the full Vulkan SDK and set VULKAN_SDK;\n\
             \x20 * CPU-only build: cargo build --no-default-features --features qwen-gguf\n\
             \x20 * bypass this check: SONICBOOM_SKIP_SDK_CHECK=1",
        );
    }
}

fn main() {
    for var in [
        "CUDACXX",
        "CUDAToolkit_ROOT",
        "CUDA_PATH",
        "CUDA_HOME",
        "VULKAN_SDK",
        "CMAKE_PREFIX_PATH",
        "SONICBOOM_SPIRV_INCLUDE_DIR",
        "SONICBOOM_SKIP_SDK_CHECK",
        "SONICBOOM_CUDA_NCCL",
        "SONICBOOM_CUDA_ARCHITECTURES",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    println!("cargo:rerun-if-changed=shim.cpp");
    println!("cargo:rerun-if-changed=CMakeLists.txt");
    println!("cargo:rerun-if-changed=../../vendor/llama.cpp/include/llama.h");
    let sources = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/llama.cpp");
    if !sources.join("include/llama.h").is_file() || !sources.join("CMakeLists.txt").is_file() {
        fail(&format!(
            "Pinned llama.cpp sources are missing or incomplete at {}.\n\
             Run `git submodule update --init --recursive` from the repository root.\n\
             CI checkout must set `submodules: recursive`; Docker contexts must include vendor/llama.cpp.\n\
             This is a source checkout problem, not a missing Vulkan/CUDA SDK.\n\
             Compilation never downloads or changes the pinned native revision.",
            sources.display()
        ));
    }

    let vulkan = cfg!(feature = "vulkan");
    let cuda = cfg!(feature = "cuda");
    let selection = match (cuda, vulkan) {
        (false, false) => "CPU only",
        (true, false) => "CPU + CUDA (ggml-cuda)",
        (false, true) => "CPU + Vulkan (ggml-vulkan)",
        (true, true) => "CPU + CUDA + Vulkan (ggml-cuda + ggml-vulkan)",
    };
    println!("cargo:warning=sonicboom-qwen-llama: native GGML backend selection: {selection}");

    let skip = std::env::var_os("SONICBOOM_SKIP_SDK_CHECK").is_some_and(|v| v != "0");
    if skip && (cuda || vulkan) {
        println!(
            "cargo:warning=sonicboom-qwen-llama: SONICBOOM_SKIP_SDK_CHECK set; \
             skipping SDK pre-flight validation"
        );
    } else {
        // Validate every requested backend before CMake runs. When both are
        // requested, both must validate or the build stops early.
        if cuda {
            validate_cuda();
        }
        if vulkan {
            validate_vulkan();
        }
    }

    let mut cmake = cmake::Config::new(".");
    cmake
        .profile("Release")
        .define("GGML_VULKAN", if vulkan { "ON" } else { "OFF" })
        .define("GGML_CUDA", if cuda { "ON" } else { "OFF" });
    // Multi-GPU NCCL collectives are an explicit opt-in and OFF by default:
    // the CUDA devel images ship libnccl, so leaving llama.cpp's ON default
    // makes static ggml-cuda.a reference NCCL symbols that Cargo never links.
    // See CMakeLists.txt for the full rationale. Single-GPU inference is
    // unaffected.
    let nccl =
        cuda && std::env::var_os("SONICBOOM_CUDA_NCCL").is_some_and(|v| !v.is_empty() && v != "0");
    cmake.define("SONICBOOM_CUDA_NCCL", if nccl { "ON" } else { "OFF" });
    if nccl {
        println!(
            "cargo:warning=sonicboom-qwen-llama: NCCL multi-GPU collectives enabled; \
             linking the discovered NCCL library"
        );
    }
    // CI smoke builds may narrow the CUDA architecture set to cut NVCC time.
    // Unset keeps the full release default (all supported architectures).
    if let Some(architectures) = std::env::var_os("SONICBOOM_CUDA_ARCHITECTURES")
        && !architectures.is_empty()
    {
        println!(
            "cargo:warning=sonicboom-qwen-llama: CUDA architectures overridden to {}",
            architectures.to_string_lossy()
        );
        cmake.define("SONICBOOM_CUDA_ARCHITECTURES", architectures);
    }
    if vulkan && let Some(dir) = find_spirv_include_dir() {
        // Handed to CMakeLists.txt, which adds it to the ggml-vulkan target
        // (the pinned llama.cpp revision never links the SPIRV-Headers package).
        println!(
            "cargo:warning=sonicboom-qwen-llama: SPIR-V headers: {}",
            dir.display()
        );
        cmake.define("SONICBOOM_SPIRV_INCLUDE_DIR", dir);
    }
    let dst = cmake.build();
    println!("cargo:rustc-link-search=native={}/lib", dst.display());
    for lib in [
        "sonicboom-qwen-llama",
        "llama",
        "ggml",
        "ggml-cpu",
        "ggml-base",
    ] {
        println!("cargo:rustc-link-lib=static={lib}");
    }
    if vulkan {
        println!("cargo:rustc-link-lib=static=ggml-vulkan");
        println!(
            "cargo:rustc-link-lib={}",
            if target_is_windows() {
                "vulkan-1"
            } else {
                "vulkan"
            }
        );
    }
    if cuda {
        let paths = std::fs::read_to_string(dst.join("build/cuda-link-paths.txt"))
            .expect("CMake must report resolved CUDA libraries for Cargo linking");
        for library in paths.lines() {
            let parent = Path::new(library)
                .parent()
                .expect("absolute CUDA library path");
            println!("cargo:rustc-link-search=native={}", parent.display());
        }
        println!("cargo:rustc-link-lib=static=ggml-cuda");
        println!("cargo:rustc-link-lib=cudart");
        println!("cargo:rustc-link-lib=cublas");
        println!("cargo:rustc-link-lib=cublasLt");
        // The NCCL library path is only present in `cuda-link-paths.txt` when
        // the collectives opt-in is enabled; link it then, never otherwise.
        if nccl {
            println!("cargo:rustc-link-lib=nccl");
        }
        println!(
            "cargo:rustc-link-lib={}",
            if target_is_windows() {
                "nvcuda"
            } else {
                "cuda"
            }
        );
    }
    let target = std::env::var("TARGET").unwrap();
    if target.contains("apple") {
        println!("cargo:rustc-link-lib=c++");
    } else if !target.contains("msvc") {
        println!("cargo:rustc-link-lib=stdc++");
    }
    if target.contains("linux") {
        println!("cargo:rustc-link-lib=dl");
        println!("cargo:rustc-link-lib=pthread");
        println!("cargo:rustc-link-lib=m");
    }
}
