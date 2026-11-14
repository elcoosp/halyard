//! CLI smart-minimal-fetch tests (§8.8): `halyard fetch` probes the
//! remote, selects the per-package latest release tags and fetches them
//! plus the base branch — idempotent (I1), byte-stable JSON (I3), an
//! honest no-op without a configured remote, and a human render that
//! names the selection provenance.

use std::process::Command;

use halyard_test_support::fixture::GitFixture;

type BoxError = Box<dyn std::error::Error>;

/// An origin with two commits and two annotated release tags (the
/// `app` package's `v{version}` template).
fn origin_fixture(label: &str) -> Result<GitFixture, BoxError> {
    let mut fx = GitFixture::new(label)?;
    fx.write(
        "Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fx.write("src/lib.rs", "pub const C: usize = 1;\n")?;
    fx.stage_all()?;
    fx.commit("feat: scaffold")?;
    fx.tag_annotated("v0.1.0", "app 0.1.0")?;
    fx.write(
        "Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.2.0\"\nedition = \"2021\"\n",
    )?;
    fx.stage_all()?;
    fx.commit("feat: next")?;
    fx.tag_annotated("v0.2.0", "app 0.2.0")?;
    Ok(fx)
}

/// A full clone of `origin` that carries no tags (the CI checkout shape
/// §8.8 exists for).
fn tagless_clone(origin: &GitFixture, label: &str) -> Result<GitFixture, BoxError> {
    let clone = GitFixture::new(label)?;
    clone.run_git(&["remote", "add", "origin", origin.file_uri().as_str()])?;
    clone.run_git(&[
        "fetch",
        "--quiet",
        "--no-tags",
        "origin",
        "+refs/heads/*:refs/remotes/origin/*",
    ])?;
    clone.run_git(&["reset", "--hard", "origin/main"])?;
    Ok(clone)
}

/// Runs the binary; returns (exit code, stdout, stderr) so warnings can
/// be asserted separately from the stable stdout document.
fn halyard_streams(
    root: &std::path::Path,
    args: &[&str],
) -> Result<(u8, String, String), BoxError> {
    let out = Command::new(env!("CARGO_BIN_EXE_halyard"))
        .arg("-C")
        .arg(root)
        .args(args)
        .env("NO_COLOR", "1")
        .output()?;
    let code = u8::try_from(out.status.code().unwrap_or_default()).unwrap_or_default();
    Ok((
        code,
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

#[test]
fn fetch_pulls_missing_tags_and_is_idempotent() -> Result<(), BoxError> {
    let origin = origin_fixture("cli-fetch-origin")?;
    let clone = tagless_clone(&origin, "cli-fetch-clone")?;
    assert!(clone.ls_tags()?.is_empty(), "the clone starts tagless");
    let (code, out, _) = halyard_streams(clone.root(), &["--format", "json", "fetch"])?;
    assert_eq!(code, 0, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    assert_eq!(value.get("remote"), Some(&serde_json::json!("origin")));
    assert_eq!(value.get("base_branch"), Some(&serde_json::json!("HEAD")));
    assert_eq!(
        value.get("fetched"),
        Some(&serde_json::json!(1)),
        "v0.2.0 is the only selected tag"
    );
    assert_eq!(value.get("already_local"), Some(&serde_json::json!(0)));
    assert_eq!(
        value.get("selection").and_then(|s| s.get("app")),
        Some(&serde_json::json!("v0.2.0"))
    );
    // Ground truth: the tag is visible to git afterwards.
    assert_eq!(clone.ls_tags()?, vec!["v0.2.0".to_owned()]);
    // I1: the re-run requests nothing.
    let (code, out, _) = halyard_streams(clone.root(), &["--format", "json", "fetch"])?;
    assert_eq!(code, 0, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    assert_eq!(value.get("fetched"), Some(&serde_json::json!(0)));
    assert_eq!(value.get("already_local"), Some(&serde_json::json!(1)));
    Ok(())
}

#[test]
fn fetch_json_is_byte_stable_across_runs() -> Result<(), BoxError> {
    // I3: two runs over the same repository state produce byte-identical
    // JSON documents — no probe-order or map-iteration leakage. (The
    // state-changing first fetch happens before the pinned pair.)
    let origin = origin_fixture("cli-fetch-det-origin")?;
    let clone = tagless_clone(&origin, "cli-fetch-det-clone")?;
    let (_, warmup, _) = halyard_streams(clone.root(), &["--format", "json", "fetch"])?;
    let warm: serde_json::Value = serde_json::from_str(warmup.trim())?;
    assert_eq!(warm.get("fetched"), Some(&serde_json::json!(1)));
    let (_, first, _) = halyard_streams(clone.root(), &["--format", "json", "fetch"])?;
    let (_, second, _) = halyard_streams(clone.root(), &["--format", "json", "fetch"])?;
    assert_eq!(first, second);
    Ok(())
}

#[test]
fn fetch_without_a_configured_remote_is_an_honest_no_op() -> Result<(), BoxError> {
    let mut fx = GitFixture::new("cli-fetch-noremote")?;
    fx.write(
        "Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fx.stage_all()?;
    fx.commit("feat: scaffold")?;
    let (code, out, _) = halyard_streams(fx.root(), &["--format", "json", "fetch"])?;
    assert_eq!(code, 0, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    assert!(
        value.get("remote").is_some_and(serde_json::Value::is_null),
        "no remote configured: {value}"
    );
    assert_eq!(value.get("fetched"), Some(&serde_json::json!(0)));
    Ok(())
}

#[test]
fn fetch_human_render_names_the_selection() -> Result<(), BoxError> {
    let origin = origin_fixture("cli-fetch-human-origin")?;
    let clone = tagless_clone(&origin, "cli-fetch-human-clone")?;
    let (code, out, _) = halyard_streams(clone.root(), &["fetch"])?;
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("app → v0.2.0"), "selection provenance: {out}");
    assert!(out.contains("fetched"), "outcome counters: {out}");
    Ok(())
}
