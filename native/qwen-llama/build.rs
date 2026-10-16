fn main() {
    println!("cargo:rerun-if-changed=shim.cpp");
    println!("cargo:rerun-if-changed=CMakeLists.txt");
    println!("cargo:rerun-if-changed=../../vendor/llama.cpp/include/llama.h");
    assert!(
        std::path::Path::new("../../vendor/llama.cpp/include/llama.h").is_file(),
        "initialize the pinned native sources with git submodule update --init --recursive"
    );
    let vulkan = cfg!(feature = "vulkan");
    let cuda = cfg!(feature = "cuda");
    let dst = cmake::Config::new(".")
        .profile("Release")
        .define("GGML_VULKAN", if vulkan { "ON" } else { "OFF" })
        .define("GGML_CUDA", if cuda { "ON" } else { "OFF" })
        .build();
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
        println!("cargo:rustc-link-lib=vulkan");
    }
    if cuda {
        println!("cargo:rustc-link-lib=static=ggml-cuda");
        println!("cargo:rustc-link-lib=cudart");
        println!("cargo:rustc-link-lib=cublas");
        println!("cargo:rustc-link-lib=cublasLt");
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
