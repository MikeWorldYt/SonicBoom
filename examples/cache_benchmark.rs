//! Explicit hash-verification comparison for the GGUF talker weights; no
//! model or audio inference. Requires `--features qwen-gguf`.
use sha2::Digest;
use std::{io::Read, time::Instant};
fn main() -> anyhow::Result<()> {
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "streaming".into());
    let path = std::path::PathBuf::from(
        std::env::args()
            .nth(2)
            .unwrap_or_else(|| "models/qwen3-tts-1.7b-gguf/qwen3_tts_talker.q5_k.gguf".into()),
    );
    let expected = sonicboom::tts::providers::qwen_gguf::models::manifest()
        .into_iter()
        .find(|file| file.path == "qwen3_tts_talker.q5_k.gguf")
        .map(|file| file.sha256.to_string())
        .expect("talker entry in pinned manifest");
    let started = Instant::now();
    let digest = match mode.as_str() {
        "baseline" => hex::encode(sha2::Sha256::digest(std::fs::read(&path)?)),
        "streaming" => {
            let mut file =
                std::io::BufReader::with_capacity(256 * 1024, std::fs::File::open(&path)?);
            let mut buffer = [0u8; 64 * 1024];
            let mut hasher = sha2::Sha256::new();
            loop {
                let n = file.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buffer[..n]);
            }
            hex::encode(hasher.finalize())
        }
        _ => anyhow::bail!("use baseline or streaming"),
    };
    anyhow::ensure!(digest == expected, "benchmark file failed pinned SHA-256");
    println!(
        "{}",
        serde_json::json!({ "mode": mode, "elapsed_secs": started.elapsed().as_secs_f64(), "bytes": std::fs::metadata(&path)?.len(), "digest": digest,
        "memory": std::fs::read_to_string("/proc/self/status").unwrap_or_default().lines().filter(|l| l.starts_with("VmHWM:") || l.starts_with("VmRSS:")).collect::<Vec<_>>() })
    );
    Ok(())
}
