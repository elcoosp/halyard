//! CLI `release --dry` smoke tests (§11.1 step 2, git-side forecast):
//! build a real git fixture workspace, run the compiled binary with
//! `release --dry` and assert the tag-phase forecast — the directives it
//! would carry out, the outcomes classified against the real ref state,
//! byte-stable JSON (I3), zero repository mutation, and the honest
//! decline of live mode (`E-CLI-RELEASE`).

use std::process::Command;

use halyard_test_support::{CommitSpec, GitFixture};

type BoxError = Box<dyn std::error::Error>;

fn fixture_dir(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir = std::env::temp_dir().join(format!(
        "halyard-cli-release-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn build_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let dir = fixture_dir(label)?;
    let mut fx = GitFixture::init_in(&dir)?;
    let scaffold = vec![
        (
            "Cargo.toml".to_owned(),
            "[package]\nname = \"solo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n".to_owned(),
        ),
        (
            "src/lib.rs".to_owned(),
            "pub const C: usize = 1;\n".to_owned(),
        ),
    ];
    fx.commit_with(&CommitSpec::files(scaffold, "feat: scaffold the crate"))?;
    Ok(fx)
}

/// The post-merge state (§11.1 step 2): the manifest says 0.1.1 while
/// only v0.1.0 is tagged — the tag phase owes v0.1.1.
fn build_merged_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let mut fx = build_workspace(label)?;
    fx.tag_annotated("v0.1.0", "solo v0.1.0")?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "Cargo.toml".to_owned(),
            "[package]\nname = \"solo\"\nversion = \"0.1.1\"\nedition = \"2021\"\n".to_owned(),
        )],
        "chore(main): release v0.1.1",
    ))?;
    Ok(fx)
}

/// The tagged state (I1): the manifest version == the latest tag —
/// nothing pending, the forecast owes nothing.
fn build_tagged_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let mut fx = build_workspace(label)?;
    fx.tag_annotated("v0.1.0", "solo v0.1.0")?;
    Ok(fx)
}

/// The `target` object of the first forecast directive (`None` when the
/// forecast carries no directive or no provenance).
fn first_target(value: &serde_json::Value) -> Option<&serde_json::Value> {
    value
        .get("directives")
        .and_then(serde_json::Value::as_array)
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("target"))
}

/// Runs the binary; returns (exit code, combined output).
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

fn tag_count(root: &std::path::Path) -> Result<usize, BoxError> {
    let out = Command::new("git")
        .current_dir(root)
        .args(["for-each-ref", "refs/tags", "--format=%(refname)"])
        .output()?;
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text.lines().filter(|line| !line.trim().is_empty()).count())
}

#[test]
fn release_live_mode_declines_honestly() -> Result<(), BoxError> {
    let fx = build_workspace("live-decline")?;
    let (code, text) = halyard(fx.root(), &["release"])?;
    assert_eq!(code, 1, "live mode is an error, not a silent no-op");
    assert!(
        text.contains("live release shell"),
        "the decline explains itself: {text}"
    );
    assert!(
        text.contains("--dry"),
        "the decline points at the forecast mode: {text}"
    );
    assert_eq!(tag_count(fx.root())?, 0, "the decline writes nothing");
    // The stable §15.4 code rides the JSON error document.
    let (code, out) = halyard(fx.root(), &["--format", "json", "release"])?;
    assert_eq!(code, 1, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    let code = value
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(serde_json::Value::as_str);
    assert_eq!(code, Some("E-CLI-RELEASE"));
    Ok(())
}

#[test]
fn release_dry_forecasts_pending_tags_without_writing() -> Result<(), BoxError> {
    let fx = build_merged_workspace("pending")?;
    let refs_before = tag_count(fx.root())?;
    let (code, text) = halyard(fx.root(), &["release", "--dry", "--format", "json"])?;
    assert_eq!(code, 0, "a forecast exits 0: {text}");
    assert!(
        text.contains("\"tag\": \"v0.1.1\""),
        "the owed tag is named: {text}"
    );
    assert!(
        text.contains("would_create"),
        "the outcome is a forecast: {text}"
    );
    assert!(text.contains("\"created\": 1"), "one directive: {text}");
    assert!(text.contains("\"failed\": 0"));
    assert_eq!(
        tag_count(fx.root())?,
        refs_before,
        "the forecast never writes (I1 guard)"
    );
    Ok(())
}

#[test]
fn release_dry_is_byte_identical_across_runs() -> Result<(), BoxError> {
    let fx = build_merged_workspace("determinism")?;
    let (first_code, first) = halyard(fx.root(), &["release", "--dry", "--format", "json"])?;
    let (second_code, second) = halyard(fx.root(), &["release", "--dry", "--format", "json"])?;
    assert_eq!(first_code, 0);
    assert_eq!(second_code, 0);
    assert_eq!(first, second, "I3: the forecast is byte-stable");
    Ok(())
}

#[test]
fn release_dry_on_tagged_repo_owes_nothing() -> Result<(), BoxError> {
    let fx = build_tagged_workspace("tagged")?;
    let (code, text) = halyard(fx.root(), &["release", "--dry", "--format", "json"])?;
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("\"created\": 0"),
        "no directive when every manifest version is tagged: {text}"
    );
    assert!(text.contains("\"already_tagged\": 0"));
    assert!(text.contains("\"failed\": 0"));
    assert_eq!(tag_count(fx.root())?, 1, "the existing tag is untouched");
    Ok(())
}

#[test]
fn release_dry_human_render_lists_directives() -> Result<(), BoxError> {
    let fx = build_merged_workspace("human")?;
    let (code, text) = halyard(fx.root(), &["release", "--dry"])?;
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("v0.1.1"), "the owed tag appears: {text}");
    assert!(
        text.contains("would create"),
        "the outcome reads as a forecast: {text}"
    );
    assert!(
        text.contains("nothing written"),
        "the header disclaims writes: {text}"
    );
    Ok(())
}

/// The worktree-only bump state: the manifest says 0.1.1 on disk while
/// HEAD's manifest still says 0.1.0 — the version was never committed,
/// so the pinpoint walk cannot verify a target and the documented HEAD
/// fallback applies (the phase still runs, honestly).
fn build_dirty_workspace(label: &str) -> Result<(GitFixture, String), BoxError> {
    let mut fx = build_workspace(label)?;
    fx.tag_annotated("v0.1.0", "solo v0.1.0")?;
    fx.write(
        "Cargo.toml",
        "[package]\nname = \"solo\"\nversion = \"0.1.1\"\nedition = \"2021\"\n",
    )?;
    let head = fx
        .head_oid()?
        .ok_or_else(|| "fixture without HEAD".to_owned())?;
    Ok((fx, head))
}

#[test]
fn release_dry_targets_the_commit_that_introduced_the_version() -> Result<(), BoxError> {
    let fx = build_merged_workspace("pinpoint")?;
    let bump = fx
        .head_oid()?
        .ok_or_else(|| "fixture without HEAD".to_owned())?;
    let (code, text) = halyard(fx.root(), &["release", "--dry", "--format", "json"])?;
    assert_eq!(code, 0, "{text}");
    let value: serde_json::Value = serde_json::from_str(text.trim())?;
    let target = first_target(&value).ok_or("the directive carries no target")?;
    assert_eq!(
        target.get("source").and_then(serde_json::Value::as_str),
        Some("manifest_pinpoint"),
        "the bump commit is pinpointed: {text}"
    );
    assert_eq!(
        target.get("commit").and_then(serde_json::Value::as_str),
        Some(bump.as_str()),
        "the target is the commit that introduced 0.1.1, not HEAD-agnostic: {text}"
    );
    Ok(())
}

#[test]
fn release_dry_falls_back_to_head_when_the_version_was_never_committed() -> Result<(), BoxError> {
    let (fx, head) = build_dirty_workspace("dirty")?;
    let (code, text) = halyard(fx.root(), &["release", "--dry", "--format", "json"])?;
    assert_eq!(code, 0, "{text}");
    let value: serde_json::Value = serde_json::from_str(text.trim())?;
    let target = first_target(&value).ok_or("the directive carries no target")?;
    assert_eq!(
        target.get("source").and_then(serde_json::Value::as_str),
        Some("head_fallback"),
        "a never-committed version cannot be pinpointed: {text}"
    );
    assert_eq!(
        target.get("commit").and_then(serde_json::Value::as_str),
        Some(head.as_str()),
        "the fallback targets the snapshot HEAD: {text}"
    );
    Ok(())
}

#[test]
fn release_dry_human_render_names_the_target_provenance() -> Result<(), BoxError> {
    let fx = build_merged_workspace("human-pinpoint")?;
    let (code, text) = halyard(fx.root(), &["release", "--dry"])?;
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("manifest pinpoint"),
        "the pinpoint provenance is named: {text}"
    );
    assert!(
        text.contains("target "),
        "the resolved target commit is shown: {text}"
    );

    let (fx, _head) = build_dirty_workspace("human-fallback")?;
    let (code, text) = halyard(fx.root(), &["release", "--dry"])?;
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("HEAD fallback"),
        "the fallback provenance is named: {text}"
    );
    Ok(())
}
