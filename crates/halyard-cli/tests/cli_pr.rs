//! CLI `pr --dry` smoke tests (M4.2b step 1): build a real git fixture
//! workspace, run the compiled binary with `pr --dry` and assert the
//! §11.1 prepare-phase forecast — the open path with its §11.5 call
//! budget, the close probe on an empty plan, the §11.3 body, and the
//! honest decline of live mode (`E-CLI-PR`).

use std::process::Command;

use halyard_test_support::{CommitSpec, GitFixture};

type BoxError = Box<dyn std::error::Error>;

fn fixture_dir(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir = std::env::temp_dir().join(format!("halyard-cli-pr-{label}-{}", std::process::id()));
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

/// A workspace where v0.1.0 is already tagged and one feat commit is
/// unreleased: manifest == latest tag (Tagged), so the machine reaches
/// the prepare phase — the §11.3 open/update/close forecast.
fn build_released_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let mut fx = build_workspace(label)?;
    fx.tag_annotated("v0.1.0", "solo v0.1.0")?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "src/lib.rs".to_owned(),
            "pub const C: usize = 2;\n".to_owned(),
        )],
        "feat: widen the constant",
    ))?;
    Ok(fx)
}

/// The post-merge Release-PR state (§11.1 step 2): the manifest says
/// 0.1.1 while only v0.1.0 is tagged — the tag phase owes v0.1.1 and the
/// PROPAGATE rule defers the prepare phase.
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

/// The reverted-merge anomaly (§11.1 step 2): the manifest went back
/// below the released tag — reported as Behind, never auto-tagged.
fn build_behind_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let mut fx = build_workspace(label)?;
    fx.tag_annotated("v0.2.0", "solo v0.2.0")?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "src/lib.rs".to_owned(),
            "pub const C: usize = 3;\n".to_owned(),
        )],
        "feat: tweak the constant",
    ))?;
    Ok(fx)
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
    // The child's exit code: a byte in practice (the CLI's codes are
    // 0/1/3); `try_from` keeps the cast lint-free.
    let code = u8::try_from(out.status.code().unwrap_or_default()).unwrap_or_default();
    Ok((code, text))
}

#[test]
fn pr_dry_forecasts_the_open_path_with_the_call_budget() -> Result<(), BoxError> {
    let fx = build_released_workspace("open")?;
    let (code, out) = halyard(fx.root(), &["pr", "--dry"])?;
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("dry run"), "{out}");
    assert!(out.contains("branch: halyard/release/HEAD"), "{out}");
    assert!(out.contains("would open PR #0 (preview)"), "{out}");
    assert!(out.contains("forge calls forecast: 2"), "{out}");
    // The §11.3 body is the previewed artifact.
    assert!(out.contains("## 🚀 Release plan"), "{out}");
    assert!(out.contains("solo"), "{out}");
    Ok(())
}

#[test]
fn pr_dry_title_is_conventional_and_tag_named() -> Result<(), BoxError> {
    let fx = build_released_workspace("title")?;
    let (code, out) = halyard(fx.root(), &["pr", "--dry"])?;
    assert_eq!(code, 0, "{out}");
    // Single-package repos use the `v{version}` tag template (appendix
    // A.3); monorepos get `{name}-v{version}` per package.
    assert!(out.contains("title: \"chore: release v0.1.1\""), "{out}");
    Ok(())
}

#[test]
fn pr_dry_forecasts_the_initial_tag_phase_on_an_untagged_repo() -> Result<(), BoxError> {
    // §11.1 step 2 on a fresh repo: no tag ever matched, so the manifest
    // version is owed — the initial release tags what exists.
    let fx = build_workspace("initial-tag")?;
    let (code, out) = halyard(fx.root(), &["pr", "--dry"])?;
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("would create release `v0.1.0` (preview"),
        "{out}"
    );
    assert!(out.contains("prepare phase: deferred (PROPAGATE"), "{out}");
    assert!(out.contains("forge calls forecast: 1"), "{out}");
    Ok(())
}

#[test]
fn pr_dry_forecasts_the_tag_phase_after_a_release_pr_merge() -> Result<(), BoxError> {
    // The post-merge state: the manifest says 0.1.1, only v0.1.0 is
    // tagged — the reconciliation queues v0.1.1 and PROPAGATE defers the
    // Release PR (§11.1 step 2).
    let fx = build_merged_workspace("merged")?;
    let (code, out) = halyard(fx.root(), &["pr", "--dry"])?;
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("pending tags (manifest-vs-tags, §11.1 step 2): 1"),
        "{out}"
    );
    assert!(out.contains("- v0.1.1 (solo 0.1.1)"), "{out}");
    assert!(
        out.contains("would create release `v0.1.1` (preview"),
        "{out}"
    );
    assert!(out.contains("prepare phase: deferred (PROPAGATE"), "{out}");
    // §11.5 budget: one create call, no find probe (the prepare phase is
    // gated).
    assert!(out.contains("forge calls forecast: 1"), "{out}");
    Ok(())
}

#[test]
fn pr_dry_reports_a_behind_manifest_without_tagging_it() -> Result<(), BoxError> {
    // The reverted-merge anomaly: the manifest is below the released
    // tag — visible in the report, never queued (no downward tags).
    let fx = build_behind_workspace("behind")?;
    let (code, out) = halyard(fx.root(), &["pr", "--dry"])?;
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("pending tags (manifest-vs-tags, §11.1 step 2): 0"),
        "{out}"
    );
    assert!(out.contains("behind, not tagged: v0.2.0"), "{out}");
    assert!(!out.contains("would create release"), "{out}");
    // Nothing pending ⇒ the prepare phase runs (no PROPAGATE).
    assert!(!out.contains("prepare phase: deferred"), "{out}");
    Ok(())
}

#[test]
fn pr_dry_is_deterministic() -> Result<(), BoxError> {
    let fx = build_merged_workspace("determinism")?;
    let (_, first) = halyard(fx.root(), &["pr", "--dry"])?;
    let (_, second) = halyard(fx.root(), &["pr", "--dry"])?;
    assert_eq!(
        first, second,
        "same repo state ⇒ byte-identical forecast (I3)"
    );
    Ok(())
}

#[test]
fn pr_dry_json_is_a_stable_document() -> Result<(), BoxError> {
    let fx = build_released_workspace("json")?;
    let (code, out) = halyard(fx.root(), &["--format", "json", "pr", "--dry"])?;
    assert_eq!(code, 0, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    assert_eq!(
        value.get("mode").and_then(serde_json::Value::as_str),
        Some("dry")
    );
    assert_eq!(
        value
            .get("pending_plan")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        value.get("calls").and_then(serde_json::Value::as_u64),
        Some(2)
    );
    // The §11.1 step-2 reconciliation rides in the document: nothing
    // pending (the manifest version is tagged), no PROPAGATE.
    assert_eq!(
        value
            .get("pending_releases")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(0)
    );
    assert_eq!(
        value.get("propagated").and_then(serde_json::Value::as_bool),
        Some(false)
    );
    let effects = value
        .get("effects")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("effects rows missing: {out}"));
    assert_eq!(effects.len(), 1, "{out}");
    let first = effects
        .first()
        .unwrap_or_else(|| unreachable!("one effect"));
    assert_eq!(
        first.get("type").and_then(serde_json::Value::as_str),
        Some("pr_opened")
    );
    assert_eq!(
        first.get("number").and_then(serde_json::Value::as_u64),
        Some(0)
    );
    assert!(
        value
            .get("body")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|b| b.contains("Release plan"))
    );
    Ok(())
}

#[test]
fn pr_dry_json_carries_the_tag_phase_forecast() -> Result<(), BoxError> {
    let fx = build_merged_workspace("json-tag-phase")?;
    let (code, out) = halyard(fx.root(), &["--format", "json", "pr", "--dry"])?;
    assert_eq!(code, 0, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    let pending = value
        .get("pending_releases")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("pending_releases rows missing: {out}"));
    assert_eq!(pending.len(), 1, "{out}");
    let release = pending
        .first()
        .unwrap_or_else(|| unreachable!("one pending release"));
    assert_eq!(
        release.get("tag").and_then(serde_json::Value::as_str),
        Some("v0.1.1")
    );
    assert_eq!(
        release.get("package").and_then(serde_json::Value::as_str),
        Some("solo")
    );
    assert_eq!(
        release.get("version").and_then(serde_json::Value::as_str),
        Some("0.1.1")
    );
    assert_eq!(
        value.get("propagated").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        value.get("calls").and_then(serde_json::Value::as_u64),
        Some(1)
    );
    Ok(())
}

#[test]
fn pr_dry_empty_plan_reports_the_close_probe() -> Result<(), BoxError> {
    // A repo whose only commit is non-conventional and whose manifest
    // version is already tagged: the plan is empty and nothing is
    // pending, so the machine spends the one find probe and forecasts
    // nothing.
    let dir = fixture_dir("close-probe")?;
    let mut fx = GitFixture::init_in(&dir)?;
    fx.commit_with(&CommitSpec::files(
        vec![
            (
                "Cargo.toml".to_owned(),
                "[package]\nname = \"solo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n".to_owned(),
            ),
            (
                "src/lib.rs".to_owned(),
                "pub const C: usize = 1;\n".to_owned(),
            ),
        ],
        "update stuff without the conventional grammar",
    ))?;
    fx.tag_annotated("v0.1.0", "solo v0.1.0")?;
    let (code, out) = halyard(fx.root(), &["pr", "--dry"])?;
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("nothing to do"), "{out}");
    assert!(out.contains("forge calls forecast: 1"), "{out}");
    Ok(())
}

#[test]
fn live_mode_declines_with_e_cli_pr() -> Result<(), BoxError> {
    let fx = build_workspace("live")?;
    let (code, out) = halyard(fx.root(), &["pr"])?;
    assert_eq!(code, 1, "live mode has no adapter yet: {out}");
    assert!(out.contains("forge adapter (M4.1b"), "{out}");
    let (code, out) = halyard(fx.root(), &["--format", "json", "pr"])?;
    assert_eq!(code, 1, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    let code = value
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(serde_json::Value::as_str);
    assert_eq!(code, Some("E-CLI-PR"));
    Ok(())
}
