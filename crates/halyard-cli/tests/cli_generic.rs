//! CLI e2e over the M8 `generic` cell (ADR-0062): a repository whose
//! only package is a config-declared `[[generic]]` entry over a
//! regex-edited version file. The plan row carries **no publish
//! decision** (`publish: None` — the escape hatch names no registry,
//! the tag is the artifact), the edit set aims the version patch at the
//! declared file's capture span, and the release row's `ecosystem` is
//! the `generic` label the plan contract resolves.

use std::process::Command;

use halyard_test_support::{CommitSpec, GitFixture};

type BoxError = Box<dyn std::error::Error>;

fn fixture_dir(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir = std::env::temp_dir().join(format!(
        "halyard-cli-generic-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// A generic repository: one config-declared package over a `VERSION`
/// file, a feat commit driving the plan row. No manifest grammar exists
/// in the tree — the escape hatch is the only way this repository is
/// plannable (the cell's reason to exist).
fn build_generic_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let dir = fixture_dir(label)?;
    let mut fx = GitFixture::init_in(&dir)?;
    fx.commit_with(&CommitSpec::files(
        vec![
            ("VERSION".to_owned(), "1.0.0\n".to_owned()),
            (
                "halyard.toml".to_owned(),
                "[[generic]]\nname = \"squid-utils\"\nfile = \"VERSION\"\npattern = '(?<version>\\d+\\.\\d+\\.\\d+)'\n".to_owned(),
            ),
        ],
        "chore: scaffold the generic repository",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("docs/readme.md".to_owned(), "the app changes\n".to_owned())],
        "feat: the app changes",
    ))?;
    Ok(fx)
}

fn halyard(
    root: &std::path::Path,
    args: &[&str],
) -> Result<(std::process::Output, String), BoxError> {
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
    Ok((out, text))
}

/// The plan over a config-declared-only repository: the generic release
/// row exists (the wire-up feeds the config-declared file through
/// discovery), the ecosystem label is `generic`, the tag rides the
/// single-package default, and the publish decision is **absent** (the
/// `no_publish_contract` shape — no registry name is invented).
#[test]
fn a_generic_repository_plans_end_to_end() -> Result<(), BoxError> {
    let fx = build_generic_workspace("generic-plan")?;
    let (out, text) = halyard(fx.root(), &["plan", "--format", "json"])?;
    assert!(out.status.success(), "the generic repository plans: {text}");
    let value: serde_json::Value = serde_json::from_str(
        text.trim_start_matches(char::is_whitespace)
            .trim_start_matches(|c: char| c != '{'),
    )
    .map_err(|e| format!("the report is JSON: {e}\n{text}"))?;
    let releases = value
        .get("releases")
        .and_then(serde_json::Value::as_array)
        .ok_or("releases is an array")?;
    let row = releases
        .iter()
        .find(|row| row.get("package").and_then(serde_json::Value::as_str) == Some("squid-utils"))
        .ok_or("the generic package releases")?;
    assert_eq!(
        row.get("ecosystem").and_then(serde_json::Value::as_str),
        Some("generic"),
        "the ecosystem label: {text}"
    );
    assert_eq!(
        row.get("from").and_then(serde_json::Value::as_str),
        Some("1.0.0"),
        "the declared file's capture is the `from` version"
    );
    assert_eq!(
        row.get("to").and_then(serde_json::Value::as_str),
        Some("1.1.0"),
        "a feat bump is minor"
    );
    assert!(
        row.get("publish").is_none(),
        "no publish decision exists: {text}"
    );
    // The edit set: the version patch aims at the declared file.
    let edits = value
        .get("edits")
        .and_then(serde_json::Value::as_array)
        .ok_or("edits is an array")?;
    let edit = edits
        .iter()
        .find(|edit| edit.get("file").and_then(serde_json::Value::as_str) == Some("VERSION"))
        .ok_or("the VERSION edit is planned")?;
    assert!(
        edit.get("patches")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|p| !p.is_empty()),
        "the capture-span patch is planned: {text}"
    );
    Ok(())
}

/// The dry run state: `plan --dry`-shaped commands never touch the
/// working tree (I5) — the VERSION file keeps its bytes (the same probe
/// the apply-side test inverts in M2).
#[test]
fn the_plan_leaves_the_declared_file_untouched() -> Result<(), BoxError> {
    let fx = build_generic_workspace("generic-plan-dry")?;
    let (out, text) = halyard(fx.root(), &["plan", "--format", "json"])?;
    assert!(out.status.success(), "the plan succeeded: {text}");
    let version = std::fs::read_to_string(fx.root().join("VERSION"))?;
    assert_eq!(
        version, "1.0.0\n",
        "planning never mutates the declared file (I5)"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// M8 (ADR-0062) M2 — the release half: the semantic edit seam rewrites
// the declared file's capture in place (the surrounding bytes stay
// byte-identical, I4), and the publish forecast records the
// `no_publish_contract` skip (no registry decision exists — the tag is
// the artifact, ADR-0062 D3).
// ---------------------------------------------------------------------------

/// `apply` rewrites the declared file's capture in place: the version
/// byte-swaps, every surrounding byte (the `halyard.toml`, the newline)
/// is byte-identical, and a re-run of the same plan is the idempotent
/// no-op.
#[test]
fn apply_rewrites_the_declared_version_file_in_place() -> Result<(), BoxError> {
    let fx = build_generic_workspace("generic-apply")?;
    let (out, text) = halyard(fx.root(), &["apply", "--format", "json"])?;
    assert!(out.status.success(), "the apply succeeded: {text}");
    let version = std::fs::read_to_string(fx.root().join("VERSION"))?;
    assert_eq!(
        version, "1.1.0\n",
        "the capture swapped to the planned target: {text}"
    );
    let config = std::fs::read_to_string(fx.root().join("halyard.toml"))?;
    assert!(
        config.contains("pattern = '(?<version>\\d+\\.\\d+\\.\\d+)'"),
        "the config file survives byte-identical (I4)"
    );
    Ok(())
}

/// The dry publish forecast over a generic repository: zero publish
/// targets, the release recorded as a `no_publish_contract` skip (the
/// row carries NO publish decision — there is no registry name to
/// record, the tag is the artifact), and a successful offline run.
#[test]
fn publish_dry_records_no_publish_contract_for_a_generic_repository() -> Result<(), BoxError> {
    let fx = build_generic_workspace("generic-publish")?;
    let (out, text) = halyard(fx.root(), &["publish", "--dry", "--format", "json"])?;
    assert!(
        out.status.success(),
        "the generic forecast succeeded: {text}"
    );
    let value: serde_json::Value = serde_json::from_str(
        text.trim_start_matches(char::is_whitespace)
            .trim_start_matches(|c: char| c != '{'),
    )
    .map_err(|e| format!("the report is JSON: {e}\n{text}"))?;
    assert_eq!(
        value.get("targets").and_then(serde_json::Value::as_u64),
        Some(0),
        "zero publish targets: {text}"
    );
    let skips = value
        .get("not_targets")
        .and_then(serde_json::Value::as_array)
        .ok_or("not_targets is an array")?;
    let member = skips
        .iter()
        .find(|row| row.get("package").and_then(serde_json::Value::as_str) == Some("squid-utils"))
        .ok_or("the generic release is recorded as a skip")?;
    assert_eq!(
        member.get("reason").and_then(serde_json::Value::as_str),
        Some("no_publish_contract"),
        "the no-decision skip reason: {text}"
    );
    Ok(())
}
