//! Optional browser setup wizard and shared storage diagnostic.
mod wizard;
pub use wizard::*;

pub(crate) fn verify_model_storage(path: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    let probe = path.join(format!(".sonicboom-write-test-{}", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&probe)?;
    let result = file.sync_all();
    drop(file);
    let cleanup = std::fs::remove_file(probe);
    result.and(cleanup)
}
