//! Verified model-asset downloads for optional providers.
//!
//! Downloads pinned files from HuggingFace (or a compatible mirror) with
//! per-file SHA-256 verification, atomic temp-file + rename installs, and
//! progress/cancellation reporting. Files already present with matching
//! hashes are skipped, so restarts and reloads never re-fetch weights.
//! Shares the process-wide `reqwest` client; no Python, no CLI subprocess.

use std::path::{Path, PathBuf};

use sha2::Digest;

use super::error::ProviderError;

/// One pinned asset: relative install path, download URL, expected SHA-256
/// (hex) and expected size in bytes (advisory: used for progress).
#[derive(Debug, Clone)]
pub struct PinnedFile {
    pub path: &'static str,
    pub url: String,
    pub sha256: &'static str,
    pub size: u64,
}

impl PinnedFile {
    /// `https://huggingface.co/<repo>/resolve/<rev>/<path>` URL builder.
    pub fn huggingface(repo: &str, revision: &str, path: &'static str) -> String {
        format!("https://huggingface.co/{repo}/resolve/{revision}/{path}")
    }
}

/// Reject paths that could escape `model_dir` (absolute paths, `..`).
fn safe_join(model_dir: &Path, rel: &str) -> Result<PathBuf, ProviderError> {
    let rel_path = Path::new(rel);
    if rel_path.is_absolute()
        || rel_path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(ProviderError::invalid_voice(format!(
            "refusing to install asset outside the model directory: '{rel}'"
        )));
    }
    Ok(model_dir.join(rel_path))
}

fn hex_sha256_file(path: &Path) -> Result<String, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    Ok(hex::encode(sha2::Sha256::digest(&bytes)))
}

/// Download every missing or corrupt file, verifying hashes.
///
/// `on_progress` receives overall 0.0–1.0 progress and is safe to call
/// often; cancellation is cooperative via `cancelled()`.
/// `on_download_start` fires once per actually-fetched file so callers can
/// tell real downloads apart from cache verification (both advance progress).
pub async fn ensure_pinned_files(
    provider: &str,
    model_dir: &Path,
    files: &[PinnedFile],
    hf_token: Option<&str>,
    on_progress: impl Fn(f32) + Send + Sync,
    on_download_start: impl Fn() + Send + Sync,
    cancelled: impl Fn() -> bool + Send + Sync,
) -> Result<(), ProviderError> {
    let total: u64 = files.iter().map(|f| f.size).sum::<u64>().max(1);
    let mut done: u64 = 0;
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ProviderError::inference_failed(format!("http client failed: {e}")))?;
    for file in files {
        if cancelled() {
            return Err(ProviderError::loading(format!(
                "{provider} download cancelled"
            )));
        }
        let dest = safe_join(model_dir, file.path)?;
        let mut valid = false;
        if dest.is_file() {
            match hex_sha256_file(&dest) {
                Ok(hash) if hash.eq_ignore_ascii_case(file.sha256) => valid = true,
                _ => {
                    tracing::warn!(
                        "{} has unexpected hash, re-downloading '{}'",
                        provider,
                        file.path
                    );
                    let _ = std::fs::remove_file(&dest);
                }
            }
        }
        if valid {
            tracing::info!("{provider} already cached (verified): '{}'", file.path);
            done += file.size;
            on_progress(done as f32 / total as f32);
            continue;
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ProviderError::model_missing(format!("could not create {}: {e}", parent.display()))
            })?;
        }
        tracing::info!(
            "{} downloading '{}' ({} bytes)",
            provider,
            file.path,
            file.size
        );
        on_download_start();
        let mut request = client.get(&file.url);
        if let Some(token) = hf_token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await.map_err(|e| {
            ProviderError::model_missing(format!(
                "could not download '{}': {e} (see docs/providers.md for offline import)",
                file.path
            ))
        })?;
        if !response.status().is_success() {
            return Err(ProviderError::model_missing(format!(
                "download of '{}' failed with HTTP {}",
                file.path,
                response.status()
            )));
        }
        let tmp = dest.with_extension(format!("{}.part", uuid::Uuid::new_v4().simple()));
        let mut out = std::fs::File::create(&tmp).map_err(|e| {
            ProviderError::model_missing(format!("could not stage '{}': {e}", file.path))
        })?;
        // Restrict staged weights while downloading (defense in depth;
        // model bytes are integrity-checked before use regardless).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        let mut hasher = sha2::Sha256::new();
        let mut stream = response.bytes_stream();
        use futures_util::StreamExt;
        use std::io::Write;
        let mut file_done: u64 = 0;
        while let Some(chunk) = stream.next().await {
            if cancelled() {
                let _ = std::fs::remove_file(&tmp);
                return Err(ProviderError::loading(format!(
                    "{provider} download cancelled"
                )));
            }
            let chunk = chunk.map_err(|e| {
                ProviderError::model_missing(format!(
                    "download of '{}' interrupted: {e}",
                    file.path
                ))
            })?;
            hasher.update(&chunk);
            out.write_all(&chunk).map_err(|e| {
                ProviderError::model_missing(format!("could not stage '{}': {e}", file.path))
            })?;
            file_done += chunk.len() as u64;
            // Progress counts verified-completed files fully plus the
            // in-flight file proportionally (capped at its manifest size).
            on_progress((done + file_done.min(file.size)) as f32 / total as f32);
        }
        drop(out);
        let hash = hex::encode(hasher.finalize());
        if !hash.eq_ignore_ascii_case(file.sha256) {
            let _ = std::fs::remove_file(&tmp);
            return Err(ProviderError::model_missing(format!(
                "integrity check failed for '{}' (hash mismatch); \
                 the staged file was discarded, retry or import offline",
                file.path
            )));
        }
        std::fs::rename(&tmp, &dest).map_err(|e| {
            ProviderError::model_missing(format!("could not install '{}': {e}", file.path))
        })?;
        done += file.size;
        on_progress(done as f32 / total as f32);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opt-in: exercises the real download path (HTTP + SHA-256 + atomic
    /// install + skip-if-verified) against one tiny manifest file.
    /// Ignored by default (needs network).
    #[tokio::test]
    #[ignore = "needs network: downloads one 4 KB manifest file from HuggingFace"]
    async fn downloads_and_verifies_a_pinned_file() {
        let dir = std::env::temp_dir().join(format!("sb-dl-test-{}", uuid::Uuid::new_v4()));
        let files = [PinnedFile {
            path: "config.json",
            url: PinnedFile::huggingface(
                "Qwen/Qwen3-TTS-12Hz-0.6B-Base",
                "5d83992436eae1d760afd27aff78a71d676296fc",
                "config.json",
            ),
            sha256: "2e714c787c8edb98b05432685cddb634add2de4d4e645f653d68251ef72ba011",
            size: 4494,
        }];
        let progress = std::sync::Mutex::new(Vec::new());
        ensure_pinned_files(
            "test",
            &dir,
            &files,
            None,
            |p| {
                progress.lock().map(|mut v| v.push(p)).ok();
            },
            || {},
            || false,
        )
        .await
        .expect("pinned download");
        assert!(dir.join("config.json").is_file());
        assert!(
            progress
                .lock()
                .map(|v| v.iter().any(|p| *p >= 1.0))
                .unwrap_or(false)
        );
        // Second run skips the verified file (no re-fetch).
        ensure_pinned_files("test", &dir, &files, None, |_| {}, || {}, || false)
            .await
            .expect("skip-if-verified");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn warm_cache_never_reports_a_download() {
        // A fully cached bundle must verify silently: progress advances but
        // `on_download_start` never fires, so status lines can honestly say
        // "verifying" instead of "downloading".
        let dir = std::env::temp_dir().join(format!("sb-warm-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let content = b"pinned-bytes";
        std::fs::write(dir.join("weight.bin"), content).unwrap();
        let digest = hex::encode(sha2::Sha256::digest(content));
        let files = [PinnedFile {
            path: "weight.bin",
            url: "https://example.invalid/weight.bin".to_string(),
            sha256: Box::leak(digest.into_boxed_str()),
            size: content.len() as u64,
        }];
        let started = std::sync::Mutex::new(false);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime
            .block_on(ensure_pinned_files(
                "test",
                &dir,
                &files,
                None,
                |_| {},
                || {
                    *started.lock().unwrap() = true;
                },
                || false,
            ))
            .expect("warm cache verifies without fetching");
        assert!(!*started.lock().unwrap(), "no download must start");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn absolute_and_parent_paths_are_rejected() {
        let dir = std::path::PathBuf::from("/tmp/models");
        assert!(safe_join(&dir, "/etc/passwd").is_err());
        assert!(safe_join(&dir, "../evil.onnx").is_err());
        assert!(safe_join(&dir, "a/../../evil.onnx").is_err());
        assert!(safe_join(&dir, "speech_tokenizer/model.safetensors").is_ok());
    }

    #[test]
    fn huggingface_url_pins_revision() {
        let url = PinnedFile::huggingface("Qwen/X", "abc123", "model.safetensors");
        assert_eq!(
            url,
            "https://huggingface.co/Qwen/X/resolve/abc123/model.safetensors"
        );
    }
}
