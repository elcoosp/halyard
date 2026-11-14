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

/// Runs the binary; returns (exit code, combined output). `envs` are set
/// for the child (the live tests isolate the tagger identity through
/// `GIT_CONFIG_GLOBAL`).
fn halyard_env(
    root: &std::path::Path,
    args: &[&str],
    envs: &[(&str, &std::path::Path)],
) -> Result<(u8, String), BoxError> {
    let out = Command::new(env!("CARGO_BIN_EXE_halyard"))
        .arg("-C")
        .arg(root)
        .args(args)
        .env("NO_COLOR", "1")
        .envs(envs.iter().map(|(key, value)| (*key, *value)))
        .output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let code = u8::try_from(out.status.code().unwrap_or_default()).unwrap_or_default();
    Ok((code, text))
}

/// Writes an isolated global git config carrying a complete tagger
/// identity, so the live tag phase is hermetic (the writer fails closed
/// on an incomplete identity — the identity must be configured, never
/// synthesized).
fn identity_config(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let path = std::env::temp_dir().join(format!(
        "halyard-cli-release-{label}-{}-gitconfig",
        std::process::id()
    ));
    std::fs::write(
        &path,
        "[user]\n\tname = halyard fixture\n\temail = fixture@example.invalid\n",
    )?;
    Ok(path)
}

/// Writes an empty global git config (no `user.*`) — the incomplete
/// identity the fail-closed test exercises.
fn empty_identity_config(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let path = std::env::temp_dir().join(format!(
        "halyard-cli-release-{label}-{}-empty-gitconfig",
        std::process::id()
    ));
    std::fs::write(&path, "")?;
    Ok(path)
}

/// A bare `origin` wired into the fixture (the §11.2 push target).
fn with_origin(fx: &GitFixture, label: &str) -> Result<std::path::PathBuf, BoxError> {
    let origin = fx.root().join(format!("{label}-origin.git"));
    fx.run_git(&["init", "--bare", origin.to_string_lossy().as_ref()])?;
    fx.run_git(&["remote", "add", "origin", origin.to_string_lossy().as_ref()])?;
    Ok(origin)
}

/// The remote's tag names, as bare `git ls-remote` ground truth.
fn remote_tags(fx: &GitFixture) -> Result<Vec<String>, BoxError> {
    let out = fx.run_git(&["ls-remote", "--tags", "origin"])?;
    let mut names = Vec::new();
    for line in out.lines() {
        let Some((_, reference)) = line.split_once('\t') else {
            continue;
        };
        let Some(name) = reference.strip_prefix("refs/tags/") else {
            continue;
        };
        names.push(name.strip_suffix("^{}").unwrap_or(name).to_owned());
    }
    names.sort();
    names.dedup();
    Ok(names)
}

/// The `push` section of a `release` JSON document.
fn push_section(value: &serde_json::Value) -> &serde_json::Value {
    value
        .get("push")
        .ok_or("the JSON document carries the push section")
        .unwrap_or(&serde_json::Value::Null)
}

#[test]
fn release_live_creates_and_pushes_tags() -> Result<(), BoxError> {
    let fx = build_merged_workspace("live-push")?;
    with_origin(&fx, "live-push")?;
    let identity = identity_config("live")?;
    let (code, text) = halyard_env(
        fx.root(),
        &["release", "--format", "json"],
        &[("GIT_CONFIG_GLOBAL", identity.as_path())],
    )?;
    assert_eq!(code, 0, "{text}");
    let value: serde_json::Value = serde_json::from_str(text.trim())?;
    assert_eq!(
        value.get("mode").and_then(serde_json::Value::as_str),
        Some("live")
    );
    assert_eq!(
        value.get("created").and_then(serde_json::Value::as_i64),
        Some(1)
    );
    let push = push_section(&value);
    assert_eq!(
        push.get("remote").and_then(serde_json::Value::as_str),
        Some("origin")
    );
    assert_eq!(
        push.get("pushed").and_then(serde_json::Value::as_i64),
        Some(1)
    );
    assert_eq!(
        value
            .get("forge_releases")
            .and_then(serde_json::Value::as_str),
        Some("deferred (M4.1b)"),
        "the forge half is honestly deferred: {text}"
    );
    // Ground truth: the annotated tag exists locally and on the remote.
    let kind = fx.run_git(&["cat-file", "-t", "v0.1.1"])?;
    assert_eq!(kind.trim(), "tag", "the tag is an annotated tag object");
    assert_eq!(remote_tags(&fx)?, vec!["v0.1.1".to_owned()]);
    Ok(())
}

#[test]
fn release_live_rerun_is_a_no_op_and_byte_identical() -> Result<(), BoxError> {
    let fx = build_merged_workspace("live-rerun")?;
    with_origin(&fx, "live-rerun")?;
    let identity = identity_config("live")?;
    let env = [("GIT_CONFIG_GLOBAL", identity.as_path())];
    let (first_code, first) = halyard_env(fx.root(), &["release", "--format", "json"], &env)?;
    assert_eq!(first_code, 0, "{first}");
    let (second_code, second) = halyard_env(fx.root(), &["release", "--format", "json"], &env)?;
    assert_eq!(second_code, 0, "{second}");
    let (third_code, third) = halyard_env(fx.root(), &["release", "--format", "json"], &env)?;
    assert_eq!(third_code, 0);
    let value: serde_json::Value = serde_json::from_str(second.trim())?;
    // The re-run reconciles to *nothing owed* (the created tag now
    // carries the manifest version) — the strong I1 no-op shape.
    assert_eq!(
        value
            .get("already_tagged")
            .and_then(serde_json::Value::as_i64),
        Some(0),
        "the re-run is a no-op (I1): {second}"
    );
    assert_eq!(
        value.get("created").and_then(serde_json::Value::as_i64),
        Some(0)
    );
    assert!(
        value
            .get("pending_releases")
            .and_then(serde_json::Value::as_array)
            .is_some_and(Vec::is_empty),
        "nothing is pending after the tags landed: {second}"
    );
    let push = push_section(&value);
    assert_eq!(
        push.get("pushed").and_then(serde_json::Value::as_i64),
        Some(0),
        "the remote re-run sends nothing (I1): {second}"
    );
    assert_eq!(second, third, "I3: the no-op report is byte-stable");
    assert_eq!(tag_count(fx.root())?, 2, "no duplicate tags (I2)");
    Ok(())
}

#[test]
fn release_live_no_push_skips_the_phase() -> Result<(), BoxError> {
    let fx = build_merged_workspace("live-nopush")?;
    let identity = identity_config("live")?;
    let (code, text) = halyard_env(
        fx.root(),
        &["release", "--no-push", "--format", "json"],
        &[("GIT_CONFIG_GLOBAL", identity.as_path())],
    )?;
    assert_eq!(code, 0, "{text}");
    let value: serde_json::Value = serde_json::from_str(text.trim())?;
    let push = push_section(&value);
    assert!(
        push.get("remote").is_none_or(serde_json::Value::is_null),
        "--no-push skips the push phase: {text}"
    );
    assert_eq!(
        value.get("created").and_then(serde_json::Value::as_i64),
        Some(1)
    );
    assert_eq!(tag_count(fx.root())?, 2, "the local tags still land");
    Ok(())
}

#[test]
fn release_live_without_remote_skips_push_honestly() -> Result<(), BoxError> {
    let fx = build_merged_workspace("live-noremote")?;
    let identity = identity_config("live")?;
    let (code, text) = halyard_env(
        fx.root(),
        &["release", "--format", "json"],
        &[("GIT_CONFIG_GLOBAL", identity.as_path())],
    )?;
    assert_eq!(
        code, 0,
        "a local-only repository releases without a push: {text}"
    );
    let value: serde_json::Value = serde_json::from_str(text.trim())?;
    let push = push_section(&value);
    assert!(
        push.get("remote").is_none_or(serde_json::Value::is_null),
        "no remote configured is an honest skip: {text}"
    );
    assert_eq!(tag_count(fx.root())?, 2, "the local tags still land");
    Ok(())
}

#[test]
fn release_live_conflict_fails_closed() -> Result<(), BoxError> {
    let fx = build_merged_workspace("live-conflict")?;
    with_origin(&fx, "live-conflict")?;
    // Hostile pre-state: the remote carries v0.1.1 at the *parent* commit
    // (an outside actor moved it), while the pinpoint targets the bump
    // commit. The push must refuse — release tags are never moved.
    let parent = fx.resolve("HEAD~1")?;
    fx.run_git(&["tag", "-f", "v0.1.1", &parent])?;
    fx.run_git(&["push", "origin", "refs/tags/v0.1.1"])?;
    fx.run_git(&["tag", "-d", "v0.1.1"])?;
    let identity = identity_config("live")?;
    let (code, text) = halyard_env(
        fx.root(),
        &["release", "--format", "json"],
        &[("GIT_CONFIG_GLOBAL", identity.as_path())],
    )?;
    assert_eq!(code, 1, "a divergent remote tag fails the run: {text}");
    let value: serde_json::Value = serde_json::from_str(text.trim())?;
    let push = push_section(&value);
    assert_eq!(
        push.get("failed").and_then(serde_json::Value::as_i64),
        Some(1)
    );
    let row = push
        .get("rows")
        .and_then(serde_json::Value::as_array)
        .and_then(|rows| rows.first())
        .ok_or("the push row exists")?;
    assert_eq!(
        row.get("error_code").and_then(serde_json::Value::as_str),
        Some("E-GIT-TAG-CONFLICT"),
        "the remote divergence carries the stable conflict code: {text}"
    );
    assert_eq!(
        remote_tags(&fx)?,
        vec!["v0.1.1".to_owned()],
        "nothing forced"
    );
    Ok(())
}

#[test]
fn release_live_incomplete_identity_fails_closed() -> Result<(), BoxError> {
    let fx = build_merged_workspace("live-identity")?;
    with_origin(&fx, "live-identity")?;
    // An empty global config: no user.name/user.email. The writer never
    // synthesizes a tagger (E-GIT-TAG) and nothing lands anywhere.
    let empty = empty_identity_config("live-identity")?;
    let (code, text) = halyard_env(
        fx.root(),
        &["release", "--format", "json"],
        &[("GIT_CONFIG_GLOBAL", empty.as_path())],
    )?;
    assert_eq!(code, 1, "{text}");
    let value: serde_json::Value = serde_json::from_str(text.trim())?;
    assert_eq!(
        value.get("failed").and_then(serde_json::Value::as_i64),
        Some(1)
    );
    let row = value
        .get("directives")
        .and_then(serde_json::Value::as_array)
        .and_then(|rows| rows.first())
        .ok_or("the directive row exists")?;
    assert_eq!(
        row.get("error_code").and_then(serde_json::Value::as_str),
        Some("E-GIT-TAG"),
        "an incomplete identity is a fail-closed E-GIT-TAG: {text}"
    );
    assert_eq!(tag_count(fx.root())?, 1, "nothing was written");
    assert_eq!(
        remote_tags(&fx)?,
        Vec::<String>::new(),
        "nothing was pushed"
    );
    Ok(())
}

#[test]
fn release_dry_forecasts_the_push_offline() -> Result<(), BoxError> {
    let fx = build_merged_workspace("dry-push")?;
    with_origin(&fx, "dry-push")?;
    let (code, text) = halyard(fx.root(), &["release", "--dry", "--format", "json"])?;
    assert_eq!(code, 0, "{text}");
    let value: serde_json::Value = serde_json::from_str(text.trim())?;
    let push = push_section(&value);
    assert_eq!(
        push.get("remote").and_then(serde_json::Value::as_str),
        Some("origin")
    );
    assert_eq!(
        push.get("forecast").and_then(serde_json::Value::as_bool),
        Some(true),
        "the dry push section is a forecast"
    );
    assert_eq!(
        push.get("rows")
            .and_then(serde_json::Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("outcome"))
            .and_then(serde_json::Value::as_str),
        Some("would_push"),
        "the forecast names what a live run would send"
    );
    // The forecast cannot write: the remote stays empty.
    assert_eq!(remote_tags(&fx)?, Vec::<String>::new());
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
