//! The walk-provenance reporting contract (the M2 stats wiring): the
//! git engine's own counters ([`GitHistory`]'s tree-diff stats, merge
//! stats, degraded/truncated markers) are public data the shell used to
//! drop on the floor — every report now carries them, identically
//! rendered (I3: deterministic counters, no clock).

use halyard_test_support::{CommitSpec, GitFixture};

type BoxError = Box<dyn std::error::Error>;

/// The solo-package fixture (one release commit after the tag).
fn build_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let dir =
        std::env::temp_dir().join(format!("halyard-cli-stats-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
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
        "feat: scaffold the crate",
    ))?;
    fx.tag_annotated("v0.1.0", "solo v0.1.0")?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "src/lib.rs".to_owned(),
            "pub const C: usize = 2;\n".to_owned(),
        )],
        "feat: extend the crate",
    ))?;
    Ok(fx)
}

fn halyard(root: &std::path::Path, args: &[&str]) -> Result<(u8, String), BoxError> {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_halyard"))
        .arg("-C")
        .arg(root)
        .args(args)
        .env("NO_COLOR", "1")
        .env("GITHUB_TOKEN", "")
        .output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let code = u8::try_from(out.status.code().unwrap_or_default()).unwrap_or_default();
    Ok((code, text))
}

/// `pr --dry --format json` carries the walk provenance: the engine's
/// tree-diff counters, the merge-policy counters and the degraded /
/// truncated markers — all deterministic (the counters are the same on
/// every run of the same tree; pinned byte-stable below).
#[test]
fn pr_dry_json_carries_the_walk_provenance() -> Result<(), BoxError> {
    let fx = build_workspace("pr-json")?;
    let (code, out) = halyard(
        fx.root(),
        &["--no-cache", "pr", "--dry", "--format", "json"],
    )?;
    assert_eq!(code, 0, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    let history = value
        .get("history")
        .ok_or("the report carries the walk provenance")?;
    assert!(
        history
            .get("degraded")
            .and_then(serde_json::Value::as_bool)
            .is_some(),
        "the degraded marker is a bool — honest about the commit-graph shape: {history}"
    );
    assert_eq!(
        history
            .get("truncated")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    for counter in ["trees_read", "entries_read", "memo_hits", "bloom_skips"] {
        let count = history
            .get(counter)
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| format!("history.{counter} is a counter: {history}"))?;
        assert!(
            count > 0 || counter == "memo_hits" || counter == "bloom_skips",
            "{counter}: {history}"
        );
    }
    assert!(
        history
            .get("bloom_unavailable")
            .and_then(serde_json::Value::as_bool)
            .is_some(),
        "the bloom marker is a bool (the documented ceiling is visible)"
    );
    for counter in [
        "merges_dropped",
        "merges_kept",
        "reverts_canceled",
        "reverts_as_fix",
    ] {
        assert!(
            history
                .get(counter)
                .and_then(serde_json::Value::as_u64)
                .is_some(),
            "history.{counter} is a counter: {history}"
        );
    }
    // Determinism (I3): the provenance block is identical across
    // uncached runs (a warm §8.7 cache run skips the tree diff by
    // design — steady-state O(new commits) — so the cold counters are
    // pinned against a second cold run).
    let (_, second) = halyard(
        fx.root(),
        &["--no-cache", "pr", "--dry", "--format", "json"],
    )?;
    let second_value: serde_json::Value = serde_json::from_str(second.trim())?;
    assert_eq!(
        history,
        second_value
            .get("history")
            .ok_or("second run carries the provenance")?,
        "same tree ⇒ same counters"
    );
    Ok(())
}

/// `release --dry --format json` carries the same provenance block.
#[test]
fn release_dry_json_carries_the_walk_provenance() -> Result<(), BoxError> {
    let fx = build_workspace("release-json")?;
    let (code, out) = halyard(fx.root(), &["release", "--dry", "--format", "json"])?;
    assert_eq!(code, 0, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    let history = value
        .get("history")
        .ok_or("the release report carries the walk provenance")?;
    assert!(
        history
            .get("trees_read")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default()
            >= 1,
        "the guided walk reads at least the changed tree: {history}"
    );
    Ok(())
}

/// The human renders surface the walk provenance as one skimmable line
/// (§16.3: output is skimmable) — `plan`, `pr --dry`, `release --dry`.
#[test]
fn human_renders_surface_the_walk_line() -> Result<(), BoxError> {
    let fx = build_workspace("human")?;
    let (_, plan_out) = halyard(fx.root(), &["plan"])?;
    assert!(
        plan_out.contains("walk:"),
        "plan carries the walk line: {plan_out}"
    );
    assert!(
        plan_out.contains("merges") || plan_out.contains("bloom"),
        "the walk line names the counter families: {plan_out}"
    );
    let (_, pr_out) = halyard(fx.root(), &["pr", "--dry"])?;
    assert!(
        pr_out.contains("walk:"),
        "pr carries the walk line: {pr_out}"
    );
    let (_, release_out) = halyard(fx.root(), &["release", "--dry"])?;
    assert!(
        release_out.contains("walk:"),
        "release carries the walk line: {release_out}"
    );
    Ok(())
}

/// The `halyard-plan/v1` JSON stays frozen: `plan --json` must NOT grow
/// a provenance key (the schema is the public contract, §5.8 — the
/// provenance surfaces ride the report documents, not the plan).
#[test]
fn plan_json_stays_frozen() -> Result<(), BoxError> {
    let fx = build_workspace("plan-json")?;
    let (code, out) = halyard(fx.root(), &["plan", "--format", "json"])?;
    assert_eq!(code, 0, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    assert!(
        value.get("history").is_none(),
        "the plan schema is frozen — no provenance key: {out}"
    );
    assert!(
        value.get("schema").and_then(serde_json::Value::as_str) == Some("halyard-plan/v1"),
        "{out}"
    );
    Ok(())
}
