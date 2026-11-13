//! M3.2 differential (§9.2 acceptance): our Cargo.lock patch must be
//! byte-identical to what cargo itself writes for the same version move.
//!
//! Ground truth: a real cargo workspace (path dependencies only, so every
//! step is offline) in a tempdir. `cargo update --workspace --offline`
//! generates the initial lockfile, a member manifest is bumped, cargo
//! regenerates the lockfile, and our patcher must produce the same bytes
//! from the same starting lockfile — otherwise the patcher must have
//! declined (`NeedsNative`). Honesty gate: the differential *skips*
//! (with a note on stderr) when no cargo binary is available, instead of
//! weakening the comparison.
//!
//! Subprocesses here are test-only ground truth (invariant I6 governs
//! planning, not test harnesses; the precedent is the M1.5
//! `cargo metadata` differential).

use std::path::{Path, PathBuf};
use std::process::Command;

use halyard_core::EcosystemAdapter;
use halyard_core::lockfile::{LockfileVersionChange, NativeRefreshError};
use halyard_core::package::RelPath;
use halyard_eco_cargo::CargoAdapter;

type BoxError = Box<dyn std::error::Error>;

/// The root manifest: `app` depends on `lib` (path + req) and `tool`
/// depends on `lib` too, so the lockfile carries three member rows.
const ROOT: &str = "[workspace]\nmembers = [\"crates/*\"]\n\n[package]\nname = \"app\"\nversion = \"0.3.0\"\n\n[dependencies]\nlib = { version = \"0.1\", path = \"crates/lib\" }\ntool = { version = \"0.1\", path = \"crates/tool\" }\n";
const LIB: &str = "[package]\nname = \"lib\"\nversion = \"0.1.5\"\n\n[dependencies]\ntool = { version = \"0.1\", path = \"../tool\" }\n";
const TOOL: &str = "[package]\nname = \"tool\"\nversion = \"0.1.4\"\n";

/// Locates the cargo binary: the `CARGO` env (set by `cargo test` itself)
/// or a PATH lookup. `None` = skip the differential honestly.
fn cargo_binary() -> Option<PathBuf> {
    if let Some(from_env) = std::env::var_os("CARGO") {
        let candidate = PathBuf::from(from_env);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in ["cargo", "cargo.exe"] {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Runs `cargo update --workspace --offline` in `dir` (hermetic
/// `CARGO_HOME`); fails the test on a non-zero exit with cargo's stderr.
fn cargo_update_ws(cargo: &Path, dir: &Path) -> Result<(), BoxError> {
    let output = Command::new(cargo)
        .arg("update")
        .arg("--workspace")
        .arg("--offline")
        .current_dir(dir)
        .env("CARGO_HOME", dir.join("cargo-home"))
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "cargo update failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(())
}

/// Writes the fixture workspace into `dir` (empty lib targets: cargo
/// refuses target-less manifests).
fn write_workspace(dir: &Path) -> Result<(), BoxError> {
    for (path, body) in [
        ("Cargo.toml", ROOT),
        ("crates/lib/Cargo.toml", LIB),
        ("crates/tool/Cargo.toml", TOOL),
        ("crates/lib/src/lib.rs", ""),
        ("crates/tool/src/lib.rs", ""),
        // The root package `app` is the workspace root itself.
        ("src/lib.rs", ""),
    ] {
        let target = dir.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(target, body)?;
    }
    Ok(())
}

/// The native refresh fallback (§9.2 rule 4): `refresh_lockfile_native`
/// runs `cargo update --workspace --offline` in the workspace root and
/// the post-refresh verification (`verify_native_refresh`) proves only
/// workspace members moved. Skips honestly without a cargo binary.
#[test]
fn native_refresh_updates_the_lock_and_only_members_change() -> Result<(), BoxError> {
    let Some(cargo) = cargo_binary() else {
        eprintln!("skipping native refresh differential: no cargo binary available");
        return Ok(());
    };
    let _ = cargo;
    let tmp = tempfile::tempdir()?;
    let root = tmp.path();
    write_workspace(root)?;
    cargo_update_ws(&cargo, root)?;
    let before = std::fs::read(root.join("Cargo.lock"))?;

    // Drift the manifests ahead of the lockfile (the state the native
    // refresh exists for), then refresh without cargo's help.
    let bumped = LIB.replace("version = \"0.1.5\"", "version = \"0.1.6\"");
    std::fs::write(root.join("crates/lib/Cargo.toml"), &bumped)?;
    CargoAdapter::new().refresh_lockfile_native(root)?;

    let after = std::fs::read(root.join("Cargo.lock"))?;
    assert_ne!(before, after, "the refresh rewrote the lockfile");
    let after_text = String::from_utf8(after.clone())?;
    assert!(
        after_text.contains("name = \"lib\"\nversion = \"0.1.6\""),
        "the member moved to the manifest version: {after_text}"
    );
    // Verification: exactly the member moved; the rows it caused (dep
    // entries referencing it) are attributed to it.
    let changed = halyard_eco_cargo::lockfile::verify_native_refresh(
        &before,
        &after,
        &["app".to_owned(), "lib".to_owned(), "tool".to_owned()],
    )?;
    assert_eq!(changed, vec!["lib".to_owned()]);
    Ok(())
}

/// The verification half is pure: a refresh that moves a non-member row
/// (a resolution change) must fail closed (§9.2 rule 4).
#[test]
fn native_refresh_verification_rejects_non_member_moves() {
    use halyard_eco_cargo::lockfile::verify_native_refresh;
    /// House-style failure extraction (no `unwrap`/`unwrap_err` in
    /// tests; impossible paths are `unreachable!`).
    fn expect_rejection(result: Result<Vec<String>, NativeRefreshError>) -> NativeRefreshError {
        match result {
            Err(err) => err,
            Ok(moved) => unreachable!("expected a verification rejection, got {moved:?}"),
        }
    }
    let before = "version = 4\n\n[[package]]\nname = \"member\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"foreign\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"ab\"\n";
    // The foreign registry row moved: a resolution change, not a member
    // move.
    let after = before.replace(
        "name = \"foreign\"\nversion = \"1.0.0\"",
        "name = \"foreign\"\nversion = \"1.0.1\"",
    );
    let err = expect_rejection(verify_native_refresh(
        before.as_bytes(),
        after.as_bytes(),
        &["member".to_owned()],
    ));
    assert_eq!(err.code(), "E-LOCK-NATIVE");
    assert!(err.to_string().contains("foreign"), "{err}");
    // Same for a new row cargo added (an external dep was resolved).
    let added = format!(
        "{before}\n[[package]]\nname = \"surprise\"\nversion = \"2.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"cd\"\n"
    );
    let err = expect_rejection(verify_native_refresh(
        before.as_bytes(),
        added.as_bytes(),
        &["member".to_owned()],
    ));
    assert!(err.to_string().contains("surprise"), "{err}");
    // And for a changed lockfile format header.
    let header = before.replace("version = 4", "version = 5");
    let err = expect_rejection(verify_native_refresh(
        before.as_bytes(),
        header.as_bytes(),
        &["member".to_owned()],
    ));
    assert!(err.to_string().contains("format"), "{err}");
    // A member move with its referencing entry is the sanctioned shape.
    let member_moved = before.replace(
        "name = \"member\"\nversion = \"0.1.0\"",
        "name = \"member\"\nversion = \"0.1.1\"",
    );
    let changed = verify_native_refresh(
        before.as_bytes(),
        member_moved.as_bytes(),
        &["member".to_owned()],
    )
    .unwrap_or_else(|e| unreachable!("member-only move verifies: {e}"));
    assert_eq!(changed, vec!["member".to_owned()]);
}

/// The differential for one member bump: our patched lockfile must equal
/// cargo's regenerated lockfile byte for byte (or the patcher must have
/// declined — a mismatch without a decline fails the test).
#[test]
fn lockfile_patch_matches_cargo_update_on_member_bump() -> Result<(), BoxError> {
    let Some(cargo) = cargo_binary() else {
        eprintln!("skipping lockfile differential: no cargo binary available");
        return Ok(());
    };
    let tmp = tempfile::tempdir()?;
    let root = tmp.path();
    write_workspace(root)?;

    // 1. Cargo generates the starting lockfile (path-only workspace:
    //    offline-safe, hermetic CARGO_HOME).
    cargo_update_ws(&cargo, root)?;
    let before = std::fs::read(root.join("Cargo.lock"))?;
    let before_text = String::from_utf8(before)?;
    assert!(
        before_text.contains("name = \"lib\"\nversion = \"0.1.5\""),
        "starting lock records the manifest version: {before_text}"
    );

    // 2. Bump the member manifest (the plan's version move), keep the
    //    requirement `0.1` (still satisfied — a pure patch-level move).
    let bumped = LIB.replace("version = \"0.1.5\"", "version = \"0.1.6\"");
    std::fs::write(root.join("crates/lib/Cargo.toml"), &bumped)?;

    // 3. Cargo's ground truth for the same move.
    cargo_update_ws(&cargo, root)?;
    let expected = std::fs::read(root.join("Cargo.lock"))?;

    // 4. Our patcher over the *starting* lockfile with the same move.
    let changes = [LockfileVersionChange {
        name: "lib".to_owned(),
        from: "0.1.5".to_owned(),
        to: "0.1.6".to_owned(),
    }];
    let file = RelPath::new("Cargo.lock").ok_or("lockfile path")?;
    let outcome = CargoAdapter::new().patch_lockfile(&file, before_text.as_bytes(), &changes)?;
    match outcome {
        halyard_core::lockfile::LockfileOutcome::Patched { content, .. } => {
            assert_eq!(
                content,
                expected,
                "our patch must be byte-identical to cargo's rewrite\n--- ours ---\n{}\n--- \
                 cargo ---\n{}",
                String::from_utf8_lossy(&content),
                String::from_utf8_lossy(&expected),
            );
        }
        halyard_core::lockfile::LockfileOutcome::NeedsNative { reason } => {
            panic!("patcher declined a plain member bump: {reason}")
        }
    }

    // 5. Validity gate (docs/research/cargo-lockfile-grammar.md): the
    //    patched lock must satisfy cargo — re-running the update on our
    //    patched bytes must be a no-op for the members (nothing left to
    //    change). Write our bytes and compare against cargo's output.
    std::fs::write(root.join("Cargo.lock"), &expected)?;
    cargo_update_ws(&cargo, root)?;
    let settled = std::fs::read(root.join("Cargo.lock"))?;
    assert_eq!(
        settled, expected,
        "cargo must agree with the patched lockfile without further edits"
    );
    Ok(())
}
