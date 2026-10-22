//! Native runtime initialization and an offline, real ONNX inference probe.
use anyhow::{Context, Result};

/// Load explicitly before any session API (whose implicit loader may panic).
pub fn initialize() -> Result<()> {
    #[cfg(any(target_os = "android", feature = "android-ort"))]
    {
        let path = std::env::var_os("ORT_DYLIB_PATH")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("PREFIX").map(|p| std::path::PathBuf::from(p).join("lib/sonicboom/libonnxruntime.so")))
            .context("Set ORT_DYLIB_PATH to a compatible Android libonnxruntime.so; run scripts/termux/install.sh")?;
        anyhow::ensure!(
            path.is_file(),
            "ONNX Runtime library missing: {}",
            path.display()
        );
        ort::init_from(&path)
            .with_context(|| {
                format!(
                    "Cannot load ONNX Runtime from {} (check Android ARM64 ABI and API version)",
                    path.display()
                )
            })?
            .commit();
    }
    ort::environment::Environment::current().context("ONNX Runtime initialization failed")?;
    Ok(())
}

/// Session creation, execution, output validation and destruction, twice.
/// This tests runtime compatibility, not provider model quality/readiness.
pub fn self_test() -> Result<()> {
    initialize()?;
    for _ in 0..2 {
        let mut session = ort::session::Session::builder()?
            .with_intra_threads(1)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .commit_from_memory(include_bytes!("../assets/runtime-identity.onnx"))?;
        let input = ort::value::Tensor::from_array(([3usize], vec![1.0f32, -2.0, 3.5]))?;
        let output = session.run(ort::inputs!["x" => input])?;
        let (_, values) = output["y"].try_extract_tensor::<f32>()?;
        anyhow::ensure!(
            values == [1.0, -2.0, 3.5],
            "ONNX inference returned incorrect output"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn offline_identity_inference_and_session_recreation() {
        super::self_test().expect("native ONNX runtime must create, run and destroy real sessions");
    }
}
