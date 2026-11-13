//! Atomic apply tests (§9.1 protocol): temp → fsync → rename, mode
//! preservation, verify-before-write, CRLF/BOM preservation on disk.

use halyard_edit::{EditError, Patch, apply_to_file, apply_to_file_verified, atomic_write};

fn patch(start: u32, end: u32, text: &str) -> Patch {
    Patch::new(start, end, text.to_owned())
        .unwrap_or_else(|_| unreachable!("static patch specs are valid"))
}

fn temp_path(tag: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir();
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    dir.join(format!(
        "halyard-edit-atomic-{}-{n}-{tag}",
        std::process::id()
    ))
}

#[test]
fn apply_to_file_writes_patched_content() {
    let path = temp_path("write");
    std::fs::write(&path, b"version = \"0.4.1\"\n").unwrap_or_else(|e| panic!("seed: {e}"));
    let out =
        apply_to_file(&path, &[patch(11, 16, "0.5.0")]).unwrap_or_else(|e| panic!("apply: {e}"));
    assert_eq!(
        String::from_utf8(out).unwrap_or_default(),
        "version = \"0.5.0\"\n"
    );
    let on_disk = std::fs::read(&path).unwrap_or_else(|e| panic!("read: {e}"));
    assert_eq!(on_disk, b"version = \"0.5.0\"\n");
    std::fs::remove_file(&path).unwrap_or_else(|e| panic!("cleanup: {e}"));
}

#[test]
fn verify_failure_leaves_file_untouched() {
    let path = temp_path("verify");
    std::fs::write(&path, b"version = \"0.4.1\"\n").unwrap_or_else(|e| panic!("seed: {e}"));
    let err = apply_to_file_verified(&path, &[patch(11, 16, "0.5.0")], |_| {
        Err(EditError::VerifyFailed {
            reason: "simulated re-parse mismatch".to_owned(),
        })
    })
    .err()
    .unwrap_or_else(|| panic!("verify must fail"));
    assert_eq!(err.code(), "E-EDIT-VERIFY");
    let on_disk = std::fs::read(&path).unwrap_or_else(|e| panic!("read: {e}"));
    assert_eq!(on_disk, b"version = \"0.4.1\"\n", "file must be untouched");
    std::fs::remove_file(&path).unwrap_or_else(|e| panic!("cleanup: {e}"));
}

#[test]
fn crlf_and_bom_survive_the_disk_roundtrip() {
    let path = temp_path("crlf");
    let content: &[u8] = b"\xEF\xBB\xBF[package]\r\nversion = \"0.4.1\"\r\n";
    std::fs::write(&path, content).unwrap_or_else(|e| panic!("seed: {e}"));
    apply_to_file(&path, &[patch(25, 30, "0.5.0")]).unwrap_or_else(|e| panic!("apply: {e}"));
    let on_disk = std::fs::read(&path).unwrap_or_else(|e| panic!("read: {e}"));
    assert_eq!(
        String::from_utf8_lossy(&on_disk),
        "\u{FEFF}[package]\r\nversion = \"0.5.0\"\r\n"
    );
    std::fs::remove_file(&path).unwrap_or_else(|e| panic!("cleanup: {e}"));
}

#[cfg(unix)]
#[test]
fn file_mode_is_preserved() {
    use std::os::unix::fs::PermissionsExt as _;
    let path = temp_path("mode");
    std::fs::write(&path, b"#!/bin/sh\necho hi\n").unwrap_or_else(|e| panic!("seed: {e}"));
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| panic!("chmod: {e}"));
    apply_to_file(&path, &[patch(10, 12, "ho")]).unwrap_or_else(|e| panic!("apply: {e}"));
    let mode = std::fs::metadata(&path)
        .unwrap_or_else(|e| panic!("meta: {e}"))
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o755, "mode must survive the rename");
    std::fs::remove_file(&path).unwrap_or_else(|e| panic!("cleanup: {e}"));
}

#[test]
fn atomic_write_creates_new_files_and_no_temp_litter() {
    // A private directory: the system temp dir is shared with any
    // concurrent halyard process, whose in-flight `.halyard-edit-` temp
    // file would trip the litter scan.
    let dir =
        std::env::temp_dir().join(format!("halyard-edit-atomic-{}-litter", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("mkdir: {e}"));
    let path = dir.join("new");
    atomic_write(&path, b"created\n").unwrap_or_else(|e| panic!("write: {e}"));
    let on_disk = std::fs::read(&path).unwrap_or_else(|e| panic!("read: {e}"));
    assert_eq!(on_disk, b"created\n");
    // No temp files left in the directory.
    let litter: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("readdir: {e}"))
        .filter_map(std::result::Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".halyard-edit-")
        })
        .collect();
    assert!(litter.is_empty(), "temp litter: {litter:?}");
    std::fs::remove_file(&path).unwrap_or_else(|e| panic!("cleanup: {e}"));
    std::fs::remove_dir(&dir).unwrap_or_else(|e| panic!("rmdir: {e}"));
}

#[test]
fn out_of_bounds_patch_writes_nothing() {
    let path = temp_path("bounds");
    std::fs::write(&path, b"tiny").unwrap_or_else(|e| panic!("seed: {e}"));
    let err = apply_to_file(&path, &[patch(0, 100, "x")])
        .err()
        .unwrap_or_else(|| panic!("must fail"));
    assert_eq!(err.code(), "E-EDIT-PATCHES");
    let on_disk = std::fs::read(&path).unwrap_or_else(|e| panic!("read: {e}"));
    assert_eq!(on_disk, b"tiny");
    std::fs::remove_file(&path).unwrap_or_else(|e| panic!("cleanup: {e}"));
}
