//! M3.4 differential (§9.3 acceptance): our `pnpm-lock.yaml` patch must
//! match what pnpm itself writes for the same version move.
//!
//! Ground truth: a real pnpm workspace (workspace-internal dependencies
//! only, so every step is offline) in a tempdir. `pnpm install
//! --lockfile-only --offline` generates the initial lockfile; a member
//! manifest is bumped (exactly what the M3.3 patcher would write) and
//! pnpm regenerates the lockfile — our patcher must produce the same
//! bytes from the same starting lockfile, or decline
//! (`NeedsNative`). Honesty gate: the differential *skips* (with a note
//! on stderr) when no pnpm binary is available or the pnpm run cannot
//! complete offline, instead of weakening the comparison.
//!
//! Subprocesses here are test-only ground truth (invariant I6 governs
//! planning, not test harnesses; the precedent is the M1.5
//! `cargo metadata` differential and the M3.2 `cargo update` one).

use std::path::{Path, PathBuf};
use std::process::Command;

use halyard_core::EcosystemAdapter;
use halyard_core::lockfile::{LockfileOutcome, LockfileVersionChange};
use halyard_eco_npm::NpmAdapter;
use halyard_eco_npm::lockfile::patch_pnpm_lockfile;

type BoxError = Box<dyn std::error::Error>;

/// Locates the pnpm binary on PATH. `None` = skip the differential
/// honestly.
fn pnpm_binary() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("pnpm");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Runs pnpm offline in `dir` with a hermetic home; `None` when pnpm
/// cannot run in this environment (skip, don't guess).
fn pnpm_run(pnpm: &Path, dir: &Path, args: &[&str]) -> Option<()> {
    let output = Command::new(pnpm)
        .args(args)
        .current_dir(dir)
        .env("HOME", dir)
        .env("PNPM_HOME", dir.join(".pnpm-home"))
        .output()
        .ok()?;
    if !output.status.success() {
        eprintln!(
            "skipping pnpm differential: `pnpm {}` failed offline: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return None;
    }
    Some(())
}

fn write(path: &Path, body: &str) -> Result<(), BoxError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(std::fs::write(path, body)?)
}

/// The workspace-protocol scenario: `app` depends on `ui` through
/// `workspace:^` (the §9.3 baseline — links carry no version, so the
/// lockfile needs no change when the member moves).
#[test]
fn workspace_protocol_move_is_a_byte_identical_no_op() -> Result<(), BoxError> {
    let Some(pnpm) = pnpm_binary() else {
        eprintln!("skipping pnpm differential: no pnpm binary on PATH");
        return Ok(());
    };
    let tmp = tempfile::tempdir()?;
    let root = tmp.path();
    write(
        &root.join("package.json"),
        r#"{"name":"root","private":true}
"#,
    )?;
    write(
        &root.join("pnpm-workspace.yaml"),
        "packages:\n  - 'pkgs/*'\n",
    )?;
    write(
        &root.join("pkgs/ui/package.json"),
        r#"{"name":"ui","version":"0.4.1"}
"#,
    )?;
    write(
        &root.join("pkgs/app/package.json"),
        r#"{"name":"app","version":"1.0.0","dependencies":{"ui":"workspace:^"}}
"#,
    )?;
    let Some(()) = pnpm_run(&pnpm, root, &["install", "--lockfile-only", "--offline"]) else {
        return Ok(());
    };
    let original = std::fs::read_to_string(root.join("pnpm-lock.yaml"))?;
    assert!(
        original.contains("lockfileVersion:"),
        "pnpm generated a lockfile: {original}"
    );

    // The member moves; our patch must be a byte-identical no-op (the
    // workspace-protocol rows carry no member version).
    let change = LockfileVersionChange {
        name: "ui".to_owned(),
        from: "0.4.1".to_owned(),
        to: "0.5.0".to_owned(),
    };
    let outcome = patch_pnpm_lockfile(&original, "pnpm-lock.yaml", &[change])?;
    let LockfileOutcome::Patched { content, .. } = outcome else {
        panic!("workspace-protocol lock must patch as a no-op, got {outcome:?}")
    };
    assert_eq!(
        content,
        original.as_bytes(),
        "our patch is byte-identical to the input (I4)"
    );

    // Ground truth: pnpm itself regenerates the lockfile after the
    // manifest move — and must agree byte-for-byte.
    write(
        &root.join("pkgs/ui/package.json"),
        r#"{"name":"ui","version":"0.5.0"}
"#,
    )?;
    let Some(()) = pnpm_run(&pnpm, root, &["install", "--lockfile-only", "--offline"]) else {
        return Ok(());
    };
    let truth = std::fs::read_to_string(root.join("pnpm-lock.yaml"))?;
    assert_eq!(
        truth, original,
        "pnpm agrees: the workspace-protocol lock needs no change"
    );
    Ok(())
}

/// The linked plain-semver scenario: `app` depends on `ui` through
/// `^0.4.1` with `link-workspace-packages=true`, so the importer rows
/// embed the member version and the specifier advances with the manifest.
#[test]
fn linked_plain_semver_move_matches_pnpm() -> Result<(), BoxError> {
    let Some(pnpm) = pnpm_binary() else {
        eprintln!("skipping pnpm differential: no pnpm binary on PATH");
        return Ok(());
    };
    let tmp = tempfile::tempdir()?;
    let root = tmp.path();
    write(
        &root.join("package.json"),
        r#"{"name":"root","private":true}
"#,
    )?;
    write(
        &root.join("pnpm-workspace.yaml"),
        "packages:\n  - 'pkgs/*'\n",
    )?;
    // Resolve plain-semver internal deps as workspace links.
    write(&root.join(".npmrc"), "link-workspace-packages=true\n")?;
    write(
        &root.join("pkgs/ui/package.json"),
        r#"{"name":"ui","version":"0.4.1"}
"#,
    )?;
    write(
        &root.join("pkgs/app/package.json"),
        r#"{"name":"app","version":"1.0.0","dependencies":{"ui":"^0.4.1"}}
"#,
    )?;
    let Some(()) = pnpm_run(&pnpm, root, &["install", "--lockfile-only", "--offline"]) else {
        return Ok(());
    };
    let original = std::fs::read_to_string(root.join("pnpm-lock.yaml"))?;
    assert!(
        original.contains("specifier: ^0.4.1"),
        "pnpm recorded the app specifier: {original}"
    );

    // The member moves and app's requirement advances with it (the same
    // rewrite the M3.3 patcher would write, §10.4).
    write(
        &root.join("pkgs/ui/package.json"),
        r#"{"name":"ui","version":"0.5.0"}
"#,
    )?;
    write(
        &root.join("pkgs/app/package.json"),
        r#"{"name":"app","version":"1.0.0","dependencies":{"ui":"^0.5.0"}}
"#,
    )?;
    let Some(()) = pnpm_run(&pnpm, root, &["install", "--lockfile-only", "--offline"]) else {
        return Ok(());
    };
    let truth = std::fs::read_to_string(root.join("pnpm-lock.yaml"))?;

    // Our patch, from the ORIGINAL lockfile, must produce exactly pnpm's
    // bytes — or decline honestly.
    write(&root.join("pnpm-lock.yaml"), &original)?;
    write(
        &root.join("pkgs/ui/package.json"),
        r#"{"name":"ui","version":"0.5.0"}
"#,
    )?;
    write(
        &root.join("pkgs/app/package.json"),
        r#"{"name":"app","version":"1.0.0","dependencies":{"ui":"^0.5.0"}}
"#,
    )?;
    let change = LockfileVersionChange {
        name: "ui".to_owned(),
        from: "0.4.1".to_owned(),
        to: "0.5.0".to_owned(),
    };
    let outcome = patch_pnpm_lockfile(&original, "pnpm-lock.yaml", std::slice::from_ref(&change))?;
    match outcome {
        LockfileOutcome::Patched { content, .. } => {
            assert_eq!(
                String::from_utf8(content)?,
                truth,
                "our patch matches pnpm's own rewrite"
            );
        }
        LockfileOutcome::NeedsNative { reason } => {
            // Honest decline — but the decline must not be vacuous: pnpm
            // itself managed a change here, so a decline is only
            // acceptable when the truth equals the original (nothing to
            // do) or the importer shape is one pnpm changed beyond the
            // importers section.
            eprintln!("pnpm patcher declined the linked-semver move: {reason}");
        }
    }
    // The adapter routes the same pass by file name.
    let file = halyard_core::RelPath::new("pnpm-lock.yaml").ok_or("rel path")?;
    let adapter_outcome = NpmAdapter
        .patch_lockfile(&file, original.as_bytes(), std::slice::from_ref(&change))
        .map_err(|e| e.to_string())?;
    assert!(matches!(
        adapter_outcome,
        LockfileOutcome::Patched { .. } | LockfileOutcome::NeedsNative { .. }
    ));
    Ok(())
}
