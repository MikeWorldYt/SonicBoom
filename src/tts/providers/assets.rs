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
        || rel_path.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(ProviderError::invalid_voice(format!(
            "refusing to install asset outside the model directory: '{rel}'"
        )));
    }
    Ok(model_dir.join(rel_path))
}

fn hex_sha256_file(path: &Path, limit: u64, on_bytes: impl Fn(u64)) -> Result<String, String> {
    use std::io::Read;
    let input = std::io::BufReader::with_capacity(
        256 * 1024,
        std::fs::File::open(path).map_err(|e| format!("could not open {}: {e}", path.display()))?,
    );
    let mut input = input.take(limit.saturating_add(1));
    let mut hasher = sha2::Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut hashed = 0u64;
    loop {
        let count = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        hashed += count as u64;
        if hashed.is_multiple_of(16 * 1024 * 1024) {
            on_bytes(hashed);
        }
    }
    on_bytes(hashed);
    Ok(hex::encode(hasher.finalize()))
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct Verification {
    version: u32,
    source: String,
    digest: String,
    size: u64,
    identity: Vec<u64>,
}
fn identity(path: &Path) -> Option<Vec<u64>> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    // Unix ctime + inode prevents trusting a replaced file or a changed
    // file whose mtime was restored. Other platforms conservatively rehash.
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(vec![
            metadata.dev(),
            metadata.ino(),
            metadata.len(),
            metadata.mtime() as u64,
            metadata.mtime_nsec() as u64,
            metadata.ctime() as u64,
            metadata.ctime_nsec() as u64,
        ])
    }
    #[cfg(not(unix))]
    {
        None
    }
}
fn metadata_path(path: &Path) -> PathBuf {
    path.with_extension("verified.json")
}
fn verify_cached(path: &Path, file: &PinnedFile) -> Result<bool, String> {
    verify_cached_progress(path, file, |_| {})
}
fn verify_cached_progress(
    path: &Path,
    file: &PinnedFile,
    on_bytes: impl Fn(u64),
) -> Result<bool, String> {
    let before = identity(path);
    let record = Verification {
        version: 1,
        source: file.url.clone(),
        digest: file.sha256.to_ascii_lowercase(),
        size: file.size,
        identity: before.clone().unwrap_or_default(),
    };
    let metadata_path = metadata_path(path);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Some(_), Ok(meta), Ok(parent)) = (
            &before,
            std::fs::symlink_metadata(&metadata_path),
            std::fs::metadata(path.parent().unwrap_or(Path::new("."))),
        ) && meta.is_file()
            && meta.len() < 4096
            && meta.mode() & 0o077 == 0
            && meta.uid() == parent.uid()
            && std::fs::read(&metadata_path)
                .ok()
                .and_then(|b| serde_json::from_slice::<Verification>(&b).ok())
                .as_ref()
                == Some(&record)
        {
            tracing::debug!(
                asset = file.path,
                cache_hit = true,
                "trusted verification identity unchanged"
            );
            return Ok(true);
        }
    }
    if std::fs::metadata(path).map_err(|e| e.to_string())?.len() != file.size {
        return Ok(false);
    }
    let valid = hex_sha256_file(path, file.size, on_bytes)?.eq_ignore_ascii_case(file.sha256);
    if valid && before.is_some() && identity(path) == before {
        let tmp = metadata_path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        if let Ok(mut out) = options.open(&tmp) {
            if out
                .write_all(&serde_json::to_vec(&record).map_err(|e| e.to_string())?)
                .is_ok()
                && out.sync_all().is_ok()
            {
                let _ = std::fs::rename(&tmp, &metadata_path);
            }
            let _ = std::fs::remove_file(tmp);
        }
    }
    Ok(valid
        && std::fs::metadata(path).is_ok_and(|m| m.len() == file.size)
        && (before.is_none() || identity(path) == before))
}

/// Verify an offline/native load against the same trust root as downloads.
/// Call only from blocking inference/loading threads.
pub fn verify_local_files(model_dir: &Path, files: &[PinnedFile]) -> Result<(), ProviderError> {
    for file in files {
        let path = safe_join(model_dir, file.path)?;
        if !verify_cached(&path, file).unwrap_or(false) {
            return Err(ProviderError::model_missing(format!(
                "pinned integrity verification failed for '{}'",
                file.path
            )));
        }
    }
    Ok(())
}

/// Staging files are removed on every error, cancellation and dropped future.
struct StagedFile(PathBuf);
impl Drop for StagedFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Reap only our process-tagged staging files whose Linux owner process
/// no longer exists. Active downloads in another server are left intact.
fn cleanup_staging(dest: &Path) {
    #[cfg(target_os = "linux")]
    if let (Some(parent), Some(stem)) = (dest.parent(), dest.file_stem().and_then(|s| s.to_str()))
        && let Ok(entries) = std::fs::read_dir(parent)
    {
        for entry in entries.take(1024).flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let Some(tail) = name
                .strip_prefix(&format!("{stem}."))
                .and_then(|n| n.strip_suffix(".part"))
            else {
                continue;
            };
            let Some((pid, id)) = tail.split_once('.') else {
                continue;
            };
            if let Ok(pid) = pid.parse::<u32>()
                && uuid::Uuid::parse_str(id).is_ok()
                && !Path::new("/proc").join(pid.to_string()).exists()
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = dest;
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
    let total: u64 = files
        .iter()
        .fold(0u64, |sum, f| sum.saturating_add(f.size))
        .max(1);
    let mut done: u64 = 0;
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(1800))
        .build()
        .map_err(|e| ProviderError::inference_failed(format!("http client failed: {e}")))?;
    for file in files {
        if cancelled() {
            return Err(ProviderError::loading(format!(
                "{provider} download cancelled"
            )));
        }
        let dest = safe_join(model_dir, file.path)?;
        let sweep_path = dest.clone();
        let _ = tokio::task::spawn_blocking(move || cleanup_staging(&sweep_path)).await;
        let mut valid = false;
        if dest.is_file() {
            let check_path = dest.clone();
            let check_file = file.clone();
            let (progress_tx, mut progress_rx) = tokio::sync::watch::channel(0u64);
            let mut check = tokio::task::spawn_blocking(move || {
                verify_cached_progress(&check_path, &check_file, |n| {
                    progress_tx.send_replace(n);
                })
            });
            let result = loop {
                tokio::select! {
                    result = &mut check => break result,
                    changed = progress_rx.changed() => {
                        if changed.is_ok() { on_progress((done + (*progress_rx.borrow()).min(file.size)) as f32 / total as f32); }
                    }
                }
            };
            match result {
                Ok(Ok(true)) => valid = true,
                _ => {
                    tracing::warn!(
                        "{} has unexpected hash, re-downloading '{}'",
                        provider,
                        file.path
                    );
                    let _ = tokio::fs::remove_file(&dest).await;
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
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
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
        let tmp = dest.with_extension(format!(
            "{}.{}.part",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let _staging = StagedFile(tmp.clone());
        let mut out = tokio::fs::File::create(&tmp).await.map_err(|e| {
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
        use tokio::io::AsyncWriteExt;
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
            if file_done.saturating_add(chunk.len() as u64) > file.size {
                return Err(ProviderError::model_missing("asset exceeds pinned size"));
            }
            hasher.update(&chunk);
            out.write_all(&chunk).await.map_err(|e| {
                ProviderError::model_missing(format!("could not stage '{}': {e}", file.path))
            })?;
            file_done += chunk.len() as u64;
            if file_done > file.size {
                return Err(ProviderError::model_missing("asset exceeds pinned size"));
            }
            // Progress counts verified-completed files fully plus the
            // in-flight file proportionally (capped at its manifest size).
            on_progress((done + file_done.min(file.size)) as f32 / total as f32);
        }
        out.flush()
            .await
            .map_err(|e| ProviderError::model_missing(e.to_string()))?;
        out.sync_all()
            .await
            .map_err(|e| ProviderError::model_missing(e.to_string()))?;
        drop(out);
        let hash = hex::encode(hasher.finalize());
        if file_done != file.size || !hash.eq_ignore_ascii_case(file.sha256) {
            let _ = std::fs::remove_file(&tmp);
            return Err(ProviderError::model_missing(format!(
                "integrity check failed for '{}' (hash mismatch); \
                 the staged file was discarded, retry or import offline",
                file.path
            )));
        }
        tokio::fs::rename(&tmp, &dest).await.map_err(|e| {
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

    #[cfg(target_os = "linux")]
    #[test]
    fn crash_cleanup_reaps_only_owned_dead_process_staging() {
        let dir = std::env::temp_dir().join(format!("sb-orphan-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let dead = dir.join(format!(
            "model.4294967295.{}.part",
            uuid::Uuid::new_v4().simple()
        ));
        let live = dir.join(format!(
            "model.{}.{}.part",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let unrelated = dir.join("other.part");
        for path in [&dead, &live, &unrelated] {
            std::fs::write(path, b"partial").unwrap();
        }
        cleanup_staging(&dir.join("model.safetensors"));
        assert!(!dead.exists());
        assert!(live.exists());
        assert!(unrelated.exists());
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn verification_metadata_recovers_and_detects_changed_assets() {
        let dir = std::env::temp_dir().join(format!("sb-integrity-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("weight.bin");
        std::fs::write(&path, b"abc").unwrap();
        let file = PinnedFile {
            path: "weight.bin",
            url: "pinned/revision".into(),
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            size: 3,
        };
        assert!(verify_cached(&path, &file).unwrap());
        assert!(verify_cached(&path, &file).unwrap());
        std::fs::write(metadata_path(&path), b"corrupt metadata").unwrap();
        assert!(
            verify_cached(&path, &file).unwrap(),
            "corrupt metadata triggers pinned rehash"
        );
        std::fs::write(&path, b"abd").unwrap();
        assert!(
            !verify_cached(&path, &file).unwrap(),
            "same-size modifications invalidate trust"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn interrupted_staging_is_removed_on_drop() {
        let path = std::env::temp_dir().join(format!("sb-stage-{}.part", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"partial").unwrap();
        {
            let _staging = StagedFile(path.clone());
        }
        assert!(!path.exists());
    }
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
        // Windows rooted paths are not necessarily absolute (`\asset` has
        // no drive); joining them can still escape the model directory.
        #[cfg(windows)]
        for attack in [
            r"\evil.onnx",
            r"C:evil.onnx",
            r"C:\evil.onnx",
            r"\\host\share\evil.onnx",
        ] {
            assert!(safe_join(&dir, attack).is_err(), "accepted {attack}");
        }
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
