//! CLI maintenance-line gates (§10.3, M5.6 / ADR-0027): a
//! `[versioning].maintenance_branches = ["1.x"]` entry pins the checked
//! out branch's targets to the line it names — a breaking commit on the
//! branch refuses (`E-PLAN-MAINTENANCE`) instead of planning a release
//! across the line, while the same commit on the default branch plans
//! free.

use std::process::Command;

use halyard_test_support::{CommitSpec, GitFixture};

type BoxError = Box<dyn std::error::Error>;

fn fixture_dir(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir = std::env::temp_dir().join(format!(
        "halyard-cli-maintenance-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// One crate at `1.2.3`, tagged `v1.2.3`, with the maintenance config
/// committed on `main` and a `1.x` branch pinned at that commit — the
/// branch inherits the config, so every test drives the same file.
fn build_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let dir = fixture_dir(label)?;
    let mut fx = GitFixture::init_in(&dir)?;
    fx.commit_with(&CommitSpec::files(
        vec![
            (
                "Cargo.toml".to_owned(),
                "[package]\nname = \"solo\"\nversion = \"1.2.3\"\nedition = \"2021\"\n".to_owned(),
            ),
            (
                "src/lib.rs".to_owned(),
                "pub const C: usize = 1;\n".to_owned(),
            ),
        ],
        // The scaffold is a `chore` (no bump): the walk's attribution —
        // base-derived or initial — then owes exactly the backport
        // commit, so the gates assert the line semantics, not the
        // scaffold's bump.
        "chore: scaffold the crate",
    ))?;
    fx.tag_annotated("v1.2.3", "solo v1.2.3")?;
    fx.write(
        "halyard.toml",
        "[versioning]\nmaintenance_branches = [\"1.x\"]\n",
    )?;
    fx.stage_all()?;
    fx.commit("chore: configure halyard")?;
    fx.checkout_new("1.x", None)?;
    Ok(fx)
}

fn halyard(root: &std::path::Path, args: &[&str]) -> Result<(u8, String), BoxError> {
    let out = Command::new(env!("CARGO_BIN_EXE_halyard"))
        .arg("-C")
        .arg(root)
        .args(args)
        .env("NO_COLOR", "1")
        .output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let code = u8::try_from(out.status.code().unwrap_or_default()).unwrap_or_default();
    Ok((code, text))
}

/// A breaking commit on the pinned `1.x` branch refuses across the line
/// (`E-PLAN-MAINTENANCE`), naming the offending target.
#[test]
fn breaking_commit_on_the_maintenance_branch_refuses() -> Result<(), BoxError> {
    let mut fx = build_workspace("refuse")?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "src/api.rs".to_owned(),
            "pub const REMOVED: usize = 0;\n".to_owned(),
        )],
        "feat!: drop the api",
    ))?;
    let (code, out) = halyard(fx.root(), &["plan", "--format", "json"])?;
    assert_ne!(code, 0, "a cross-line release must refuse: {out}");
    assert!(out.contains("E-PLAN-MAINTENANCE"), "{out}");
    assert!(
        out.contains("2.0.0"),
        "the offending target is named: {out}"
    );
    Ok(())
}

/// A backport `fix` on the branch stays in line and plans the patch.
#[test]
fn backport_fix_on_the_maintenance_branch_plans() -> Result<(), BoxError> {
    let mut fx = build_workspace("in-line")?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "src/fix.rs".to_owned(),
            "pub const PATCHED: usize = 2;\n".to_owned(),
        )],
        "fix: backport the repair",
    ))?;
    let (code, out) = halyard(fx.root(), &["plan", "--format", "json"])?;
    assert_eq!(code, 0, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    let release = value
        .get("releases")
        .and_then(serde_json::Value::as_array)
        .and_then(|rows| rows.first().cloned())
        .ok_or("the plan carries one release")?;
    assert_eq!(
        release.get("to").and_then(serde_json::Value::as_str),
        Some("1.2.4"),
        "the backport stays in the line: {out}"
    );
    Ok(())
}

/// The same breaking commit on `main` — a branch the config does not
/// pin — plans free (`2.0.0`): the guard is branch-scoped.
#[test]
fn the_same_breaking_commit_on_main_plans_free() -> Result<(), BoxError> {
    let mut fx = build_workspace("main")?;
    // The fixture sits on `1.x`; `main` carries the same config (the
    // branch inherited it) but is not pinned — the branch differs, not
    // the file.
    fx.checkout("main")?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "src/api.rs".to_owned(),
            "pub const REMOVED: usize = 0;\n".to_owned(),
        )],
        "feat!: drop the api",
    ))?;
    let (code, out) = halyard(fx.root(), &["plan", "--format", "json"])?;
    assert_eq!(code, 0, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    let release = value
        .get("releases")
        .and_then(serde_json::Value::as_array)
        .and_then(|rows| rows.first().cloned())
        .ok_or("the plan carries one release")?;
    assert_eq!(
        release.get("to").and_then(serde_json::Value::as_str),
        Some("2.0.0"),
        "an unpinned branch plans across the line: {out}"
    );
    Ok(())
}
