//! Filesystem watcher for `config.toml`.
//!
//! Debounces the burst of low-level events a text
//! editor produces (300–500 ms), then routes the
//! change through the [`ConfigManager`] so validation,
//! diffing, and runtime reconfiguration stay in one
//! place.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use notify_debouncer_full::{DebounceEventResult, new_debouncer};

use super::manager::ConfigManager;

/// Debounce window for filesystem events (spec §43).
pub const DEBOUNCE_MILLIS: u64 = 400;
/// Fallback poll cadence. Native notifications are primary; polling only
/// covers filesystems where they are unreliable, so it checks cheap file
/// metadata (not content) at this interval.
pub const POLL_INTERVAL_SECS: u64 = 5;
/// Every Nth poll tick re-reads the full content even when metadata looks
/// unchanged, catching same-size replacements and metadata-blind network
/// filesystems. Revision deduplication in the manager keeps spurious reloads
/// free.
pub const POLL_VERIFY_EVERY: u32 = 6;

/// Cheap identity snapshot for change detection: length, modification time,
/// and (on Unix) file identity, so atomic rename-over-write is observed even
/// when length and mtime are preserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileMeta {
    len: u64,
    mtime_secs: i64,
    mtime_nanos: u32,
    #[cfg(unix)]
    ino: u64,
    #[cfg(unix)]
    dev: u64,
}

fn file_meta(path: &std::path::Path) -> Option<FileMeta> {
    let meta = std::fs::metadata(path).ok()?;
    let len = meta.len();
    let modified = meta.modified().ok()?;
    let age = modified
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(FileMeta {
            len,
            mtime_secs: age.as_secs() as i64,
            mtime_nanos: age.subsec_nanos(),
            ino: meta.ino(),
            dev: meta.dev(),
        })
    }
    #[cfg(not(unix))]
    {
        Some(FileMeta {
            len,
            mtime_secs: age.as_secs() as i64,
            mtime_nanos: age.subsec_nanos(),
        })
    }
}

/// Handle for the background watcher task.
pub struct ConfigWatcher {
    manager: Arc<ConfigManager>,
    poll_task: tokio::task::JoinHandle<()>,
    _debouncer: Option<
        notify_debouncer_full::Debouncer<
            notify::RecommendedWatcher,
            notify_debouncer_full::RecommendedCache,
        >,
    >,
}

impl Drop for ConfigWatcher {
    fn drop(&mut self) {
        self.poll_task.abort();
        self.manager.set_watching(false);
    }
}

/// Start watching `config_path` for external edits.
///
/// Native notifications are supplemented by content polling, including
/// platforms or filesystems where native notification setup fails.
pub fn start_watching(manager: Arc<ConfigManager>) -> Option<ConfigWatcher> {
    let config_path = manager.path().to_path_buf();
    let manager_clone = Arc::clone(&manager);
    let closure_path = config_path.clone();
    // Captured from the runtime thread `start_watching`
    // is called on; notify delivers events on its own
    // watcher thread, where `Handle::current()` would
    // panic.
    let handle = tokio::runtime::Handle::current();

    let mut debouncer = new_debouncer(
        Duration::from_millis(DEBOUNCE_MILLIS),
        None,
        move |result: DebounceEventResult| {
            if let Err(e) = &result {
                tracing::warn!("Config watcher error: {e:?}");
                return;
            }
            let events = result.as_ref().unwrap();

            // Only react to file-content changes on the
            // config file itself (create/write/remove/
            // rename), not metadata-only notices.
            let relevant = events.iter().any(|debounced| {
                let event = &debounced.event;
                event.paths.iter().any(|p| p == &closure_path)
                    && matches!(
                        event.kind,
                        notify::EventKind::Modify(_)
                            | notify::EventKind::Create(_)
                            | notify::EventKind::Remove(_)
                    )
            });
            if !relevant {
                return;
            }
            // Notify events are delivered on a dedicated
            // watcher thread; hop onto the Tokio runtime
            // (via the handle captured above) to perform
            // the async reload without blocking the
            // watcher thread.
            let manager = Arc::clone(&manager_clone);
            let path = closure_path.clone();
            handle.spawn(async move {
                match manager.reload_from_filesystem().await {
                    super::manager::ReloadOutcome::Updated { revision, changes } => {
                        tracing::info!(
                            revision,
                            changes = changes.len(),
                            "Applied external configuration change from {path:?}"
                        );
                    }
                    super::manager::ReloadOutcome::Rejected { error } => {
                        tracing::warn!(
                            "Configuration reload rejected: {error} \
                             (previous configuration remains active)"
                        );
                    }
                    super::manager::ReloadOutcome::Unchanged => {}
                }
            });
        },
    )
    .map_err(|error| {
        tracing::warn!("Native config notifications unavailable; using polling: {error}")
    })
    .ok();

    // Watch the config file's parent directory so
    // editor rename-over-write patterns are caught,
    // and the file itself.
    let watch_path = config_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    if let Some(watcher) = debouncer.as_mut()
        && let Err(error) = watcher.watch(&watch_path, notify::RecursiveMode::NonRecursive)
    {
        tracing::warn!("Native config watch unavailable; using polling: {error}");
        debouncer = None;
    }

    manager.set_watching(true);

    tracing::info!(
        path = ?config_path,
        debounce_ms = DEBOUNCE_MILLIS,
        "Watching configuration file for changes"
    );

    // Some network/virtual filesystems silently omit native notifications.
    // Metadata-first polling supplements notify at a slow cadence: cheap
    // stat checks each tick, full content reads only when metadata changed
    // or on the periodic verification tick (same-size replacements and
    // metadata-blind filesystems). The manager deduplicates both paths, so a
    // reload here is a no-op unless content actually changed.
    let poll_manager = Arc::clone(&manager);
    let poll_path = config_path;
    let mut previous_meta = file_meta(&poll_path);
    let mut previous_content = std::fs::read(&poll_path).ok();
    let poll_task = tokio::spawn(async move {
        let _ = poll_manager.reload_from_filesystem().await;
        let mut interval = tokio::time::interval(Duration::from_secs(POLL_INTERVAL_SECS));
        let mut ticks: u32 = 0;
        loop {
            interval.tick().await;
            ticks = ticks.wrapping_add(1);
            let verify = ticks.is_multiple_of(POLL_VERIFY_EVERY);
            let meta = file_meta(&poll_path);
            if !verify && meta == previous_meta {
                continue;
            }
            previous_meta = meta;
            let content = tokio::fs::read(&poll_path).await.ok();
            if content != previous_content {
                previous_content = content;
                let _ = poll_manager.reload_from_filesystem().await;
            }
        }
    });
    Some(ConfigWatcher {
        manager,
        poll_task,
        _debouncer: debouncer,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounce_window_is_in_spec_range() {
        assert!(
            (300..=500).contains(&DEBOUNCE_MILLIS),
            "debounce must be 300-500ms"
        );
    }

    #[test]
    fn fallback_poll_stays_slow_with_periodic_verification() {
        // The fallback exists for filesystems without native notifications:
        // slow metadata ticks with an even slower full-content verification.
        const {
            assert!(POLL_INTERVAL_SECS >= 1);
            assert!(POLL_VERIFY_EVERY >= 1);
            assert!(POLL_VERIFY_EVERY as u64 * POLL_INTERVAL_SECS >= 10);
        }
    }

    fn watch_file(dir: &std::path::Path, name: &str, content: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn metadata_detects_size_deletion_and_atomic_rename() {
        let dir = std::env::temp_dir().join(format!("sonicboom-meta-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = watch_file(&dir, "config.toml", b"[server]\nport = 1\n");
        let baseline = file_meta(&path).expect("fresh file has metadata");
        // Repeated stats are stable: an unchanged file never triggers a read.
        assert_eq!(file_meta(&path), Some(baseline));
        // Size changes are always observed.
        std::fs::write(&path, b"[server]\nport = 1\n# padding to change length\n").unwrap();
        assert_ne!(file_meta(&path), Some(baseline));
        // Deletion is observed as missing metadata.
        std::fs::remove_file(&path).unwrap();
        assert_eq!(file_meta(&path), None);
        // Atomic rename-over-write changes file identity even when the
        // replacement has identical length and lands within one mtime tick.
        let replacement = watch_file(&dir, "swap.toml", b"[server]\nport = 2\n");
        std::fs::write(&path, b"[server]\nport = 1\n").unwrap();
        let before_swap = file_meta(&path).expect("restored file has metadata");
        std::fs::rename(&replacement, &path).unwrap();
        assert_ne!(
            file_meta(&path),
            Some(before_swap),
            "rename-over-write must change file identity"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn same_size_replacement_changes_content_for_the_verify_tick() {
        // Same-size edits may share length (and, on coarse filesystems,
        // metadata): the periodic verification tick compares content, so it
        // must observe the replacement.
        let dir = std::env::temp_dir().join(format!("sonicboom-verify-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = watch_file(&dir, "config.toml", b"[server]\nport = 1111\n");
        let previous = std::fs::read(&path).ok();
        std::fs::write(&path, b"[server]\nport = 2222\n").unwrap();
        assert_eq!(
            previous.as_ref().unwrap().len(),
            std::fs::metadata(&path).unwrap().len() as usize
        );
        // The verify tick's content comparison observes the replacement.
        assert_ne!(std::fs::read(&path).ok(), previous);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn real_watcher_applies_edits_and_rejects_invalid_file() {
        let dir = std::env::temp_dir().join(format!("sonicboom-watch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let mut config = super::super::model::AppConfig::default();
        config.paths.audio = Some(dir.join("audio").display().to_string());
        super::super::writer::atomic_write_config_blocking(&path, &config).unwrap();
        let manager = ConfigManager::load(path.clone()).await.unwrap();
        let watcher = start_watching(manager.clone()).unwrap();
        let mut events = manager.subscribe();
        config.audio.volume = 0.5;
        super::super::writer::atomic_write_config_blocking(&path, &config).unwrap();
        let event = tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            event,
            super::super::diff::ConfigEvent::Updated { .. }
        ));
        assert_eq!(manager.get().await.audio.volume, 0.5);
        let revision = manager.revision();
        std::fs::write(&path, "[server]\nport = 'hello'\n").unwrap();
        let event = tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            event,
            super::super::diff::ConfigEvent::ReloadRejected { .. }
        ));
        assert_eq!(manager.revision(), revision);
        assert_eq!(manager.get().await.audio.volume, 0.5);
        drop(watcher);
        assert_eq!(manager.status().await.watcher, "inactive");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn watching_a_temp_config_file_works() {
        let dir = std::env::temp_dir().join(format!(
            "sonicboom-watcher-test-{}-{}",
            std::process::id(),
            "watch"
        ));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.toml");
        let _ = std::fs::remove_file(&path);
        let mut config = super::super::model::AppConfig::default();
        {
            config.paths.audio = Some(dir.join("audio").to_string_lossy().to_string());
        }
        super::super::writer::atomic_write_config_blocking(&path, &config).unwrap();
        let manager = ConfigManager::load(path).await.unwrap();
        // Watching may be unavailable on some platforms;
        // the contract is "Some on supported platforms".
        let watcher = start_watching(manager);
        assert!(watcher.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
