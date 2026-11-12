//! Atomic file writes (§9.1): temp file in the same directory → fsync →
//! rename, preserving the original file mode; Windows retries
//! `ERROR_ACCESS_DENIED` (antivirus/indexer locks) before failing.
//!
//! Nothing is written when patches fail to validate or the verifier
//! rejects the patched content — the caller gets the would-be output back
//! for inspection but the file on disk is untouched.

use std::io::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::Patch;
use crate::apply::apply;
use crate::error::EditError;

/// Temp-file name counter (process-unique; combined with the pid).
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Applies `patches` to the file at `path` and writes the result with the
/// atomic protocol. Returns the patched content.
pub fn apply_to_file(path: &Path, patches: &[Patch]) -> Result<Vec<u8>, EditError> {
    apply_to_file_verified(path, patches, |_| Ok(()))
}

/// Like [`apply_to_file`], but runs `verifier` over the patched content
/// (re-parse + semantic check, §9.1) before anything touches the disk.
pub fn apply_to_file_verified(
    path: &Path,
    patches: &[Patch],
    verifier: impl FnOnce(&[u8]) -> Result<(), EditError>,
) -> Result<Vec<u8>, EditError> {
    let original = std::fs::read(path).map_err(|source| EditError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let edited = apply(&original, patches)?;
    verifier(&edited)?;
    atomic_write(path, &edited)?;
    Ok(edited)
}

/// Writes `bytes` to `path` atomically: temp sibling → fsync → rename,
/// preserving the original mode when the file exists.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), EditError> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let temp = create_temp_sibling(dir, path)?;
    // Preserve the file mode (POSIX); no-op where unsupported.
    #[cfg(unix)]
    if let Ok(meta) = std::fs::metadata(path) {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::Permissions::from_mode(meta.permissions().mode());
        let preserve = std::fs::set_permissions(&temp, mode);
        if let Err(source) = preserve {
            let _ = std::fs::remove_file(&temp);
            return Err(EditError::Io {
                path: temp.display().to_string(),
                source,
            });
        }
    }
    if let Err(err) = write_and_sync(&temp, bytes) {
        let _ = std::fs::remove_file(&temp);
        return Err(err);
    }
    rename_with_retry(&temp, path)
}

/// Creates a unique temp sibling for `path` (same dir ⇒ same filesystem,
/// so the final rename is atomic).
fn create_temp_sibling(dir: &Path, path: &Path) -> Result<std::path::PathBuf, EditError> {
    let file_name = path
        .file_name()
        .map_or_else(|| "file".into(), |name| name.to_string_lossy());
    for _ in 0..64 {
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = dir.join(format!(
            ".halyard-edit-{}-{counter}-{file_name}.tmp",
            std::process::id()
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(_) => return Ok(candidate),
            // Name collision (never expected with pid+counter): retry.
            Err(source) if source.kind() != std::io::ErrorKind::AlreadyExists => {
                return Err(EditError::Io {
                    path: candidate.display().to_string(),
                    source,
                });
            }
            Err(_) => {}
        }
    }
    Err(EditError::Io {
        path: dir.display().to_string(),
        source: std::io::Error::other("could not create a unique temp file after 64 attempts"),
    })
}

/// Writes and fsyncs the temp file.
fn write_and_sync(temp: &Path, bytes: &[u8]) -> Result<(), EditError> {
    let mut file = std::fs::File::create(temp).map_err(|source| EditError::Io {
        path: temp.display().to_string(),
        source,
    })?;
    file.write_all(bytes).map_err(|source| EditError::Io {
        path: temp.display().to_string(),
        source,
    })?;
    file.sync_all().map_err(|source| EditError::Io {
        path: temp.display().to_string(),
        source,
    })?;
    Ok(())
}

/// Renames temp → destination. On Windows a failed rename with
/// `ERROR_ACCESS_DENIED` (mapped to `PermissionDenied`) is retried with
/// backoff — antivirus/indexer handles make fresh files briefly locked.
#[cfg(windows)]
fn rename_with_retry(temp: &Path, dest: &Path) -> Result<(), EditError> {
    let mut last: Option<std::io::Error> = None;
    for attempt in 0..5u32 {
        match std::fs::rename(temp, dest) {
            Ok(()) => return Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied && attempt < 4 => {
                last = Some(err);
                std::thread::sleep(std::time::Duration::from_millis(
                    10u64.saturating_mul(attempt + 1),
                ));
            }
            Err(err) => {
                return Err(EditError::Io {
                    path: dest.display().to_string(),
                    source: err,
                });
            }
        }
    }
    Err(EditError::Io {
        path: dest.display().to_string(),
        source: last.unwrap_or_else(|| std::io::Error::other("rename failed after retries")),
    })
}

/// Renames temp → destination (POSIX replaces atomically).
#[cfg(not(windows))]
fn rename_with_retry(temp: &Path, dest: &Path) -> Result<(), EditError> {
    std::fs::rename(temp, dest).map_err(|source| EditError::Io {
        path: dest.display().to_string(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_names_are_unique() {
        let dir = std::env::temp_dir();
        let first = create_temp_sibling(&dir, Path::new("demo.toml"))
            .unwrap_or_else(|e| panic!("temp create: {e}"));
        std::fs::remove_file(&first).unwrap_or_else(|e| panic!("cleanup: {e}"));
        let second = create_temp_sibling(&dir, Path::new("demo.toml"))
            .unwrap_or_else(|e| panic!("temp create: {e}"));
        std::fs::remove_file(&second).unwrap_or_else(|e| panic!("cleanup: {e}"));
        assert_ne!(first, second);
    }
}
