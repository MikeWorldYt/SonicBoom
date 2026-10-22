//! Model-free native link/backend and ONNX ABI probe, including normal exit.
fn main() -> anyhow::Result<()> {
    sonicboom::onnx_runtime::self_test()?;
    let backends = sonicboom_qwen_llama::runtime_backends();
    println!("Native GGML backends: {backends}");
    anyhow::ensure!(
        backends.lines().any(|s| s == "cpu"),
        "native CPU backend missing"
    );
    let path =
        std::env::temp_dir().join(format!("sonicboom-invalid-{}.gguf", uuid::Uuid::new_v4()));
    let name = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("invalid temp path"))?;
    anyhow::ensure!(
        sonicboom_qwen_llama::Model::load(name, 0, 0, 1, 64, false).is_err(),
        "missing model accepted"
    );
    std::fs::write(&path, b"corrupted GGUF model")?;
    let rejected = sonicboom_qwen_llama::Model::load(name, 0, 0, 1, 64, false).is_err();
    std::fs::remove_file(path)?;
    anyhow::ensure!(rejected, "corrupted model accepted");
    println!("Native CPU discovery and missing/corrupted model checks passed");
    Ok(())
}
