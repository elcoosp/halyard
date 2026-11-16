//! CLI prerelease-channel gates (§10.3, M5.6 step 1): a
//! `[versioning].channel = "rc"` config turns the plan's release rows
//! into the `{target}-{channel}.{n}` forecast (manifest edit included)
//! while a config without the key keeps the stable goldens — the plan
//! JSON stays byte-stable either way.

use std::process::Command;

use halyard_test_support::{CommitSpec, GitFixture};

type BoxError = Box<dyn std::error::Error>;

fn fixture_dir(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir = std::env::temp_dir().join(format!(
        "halyard-cli-channel-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// One crate, scaffolded and tagged `v0.1.0`, with one unreleased `feat`.
fn build_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let dir = fixture_dir(label)?;
    let mut fx = GitFixture::init_in(&dir)?;
    fx.commit_with(&CommitSpec::files(
        vec![
            (
                "Cargo.toml".to_owned(),
                "[package]\nname = \"solo\"\nversion = \"1.0.0\"\nedition = \"2021\"\n".to_owned(),
            ),
            (
                "src/lib.rs".to_owned(),
                "pub const C: usize = 1;\n".to_owned(),
            ),
        ],
        "feat: scaffold the crate",
    ))?;
    fx.tag_annotated("v1.0.0", "solo v1.0.0")?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "src/extra.rs".to_owned(),
            "pub const E: usize = 2;\n".to_owned(),
        )],
        "feat: grow the surface",
    ))?;
    Ok(fx)
}

/// Writes `halyard.toml` (committed — the config is part of the repo).
fn write_config(fx: &mut GitFixture, body: &str) -> Result<(), BoxError> {
    fx.write("halyard.toml", body)?;
    fx.stage_all()?;
    fx.commit("chore: configure halyard")?;
    Ok(())
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

/// The configured channel turns the plan's release row into the
/// prerelease forecast — version, tag and manifest edit alike.
#[test]
fn channel_plan_forecasts_the_prerelease() -> Result<(), BoxError> {
    let mut fx = build_workspace("on")?;
    write_config(&mut fx, "[versioning]\nchannel = \"rc\"\n")?;
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
        Some("1.1.0-rc.1"),
        "{out}"
    );
    assert_eq!(
        release.get("tag").and_then(serde_json::Value::as_str),
        Some("v1.1.0-rc.1"),
        "{out}"
    );
    Ok(())
}

/// Without the key the plan is the stable forecast — the frozen default.
#[test]
fn plan_without_channel_stays_stable() -> Result<(), BoxError> {
    let fx = build_workspace("off")?;
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
        Some("1.1.0"),
        "{out}"
    );
    assert_eq!(
        release.get("tag").and_then(serde_json::Value::as_str),
        Some("v1.1.0"),
        "{out}"
    );
    Ok(())
}

/// An invalid channel name fails closed at load (`E-CFG-CHANNEL`),
/// before any planning.
#[test]
fn invalid_channel_fails_closed() -> Result<(), BoxError> {
    let mut fx = build_workspace("bad")?;
    write_config(&mut fx, "[versioning]\nchannel = \"rc.\"\n")?;
    let (code, out) = halyard(fx.root(), &["plan", "--format", "json"])?;
    assert_ne!(code, 0, "invalid channel must fail: {out}");
    assert!(out.contains("E-CFG-CHANNEL"), "{out}");
    Ok(())
}

// ---------------------------------------------------------------------
// Branch-bound channels (§10.3 "next branch ⇒ rc", ADR-0025 step 2):
// `channel_branches` binds a channel to a branch name; the checked-out
// branch decides, and every other branch keeps the unbound default.

/// The `next` branch plans its bound `rc` channel; `main` (the default
/// branch the fixture committed on) stays stable — the same config, two
/// checkouts, two plans.
#[test]
fn branch_binding_plans_its_channel() -> Result<(), BoxError> {
    let mut fx = build_workspace("bound")?;
    write_config(
        &mut fx,
        "[versioning]\nchannel_branches = { next = \"rc\" }\n",
    )?;
    fx.checkout_new("next", None)?;

    // On `next`: the binding applies.
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
        Some("1.1.0-rc.1"),
        "the bound channel plans the prerelease: {out}"
    );

    // Back on the default branch: stable — the binding never leaks.
    fx.checkout("main")?;
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
        Some("1.1.0"),
        "the default branch stays stable: {out}"
    );
    Ok(())
}

/// An invalid channel inside a branch binding fails closed at load,
/// naming the branch.
#[test]
fn invalid_branch_binding_fails_closed() -> Result<(), BoxError> {
    let mut fx = build_workspace("badbound")?;
    write_config(
        &mut fx,
        "[versioning]\nchannel_branches = { next = \"rc.\" }\n",
    )?;
    let (code, out) = halyard(fx.root(), &["plan", "--format", "json"])?;
    assert_ne!(code, 0, "invalid bound channel must fail: {out}");
    assert!(out.contains("E-CFG-CHANNEL"), "{out}");
    Ok(())
}

// ---------------------------------------------------------------------
// Per-package channel overrides (§10.3, M5.6 step 3 / ADR-0026): a
// `[[package]] channel` entry names that package's train; the shell
// plans the override without any process-level wiring (the resolved
// map rides `ResolvedConfig::plan_config`).

/// The override plans the train with no repo-wide channel configured.
#[test]
fn package_override_plans_the_prerelease() -> Result<(), BoxError> {
    let mut fx = build_workspace("pkg-on")?;
    write_config(&mut fx, "[[package]]\nname = \"solo\"\nchannel = \"rc\"\n")?;
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
        Some("1.1.0-rc.1"),
        "the override names the train: {out}"
    );
    Ok(())
}

/// The override is the most explicit policy: it beats the repo-wide
/// default the same way it beats a branch binding (ADR-0026).
#[test]
fn package_override_beats_the_repo_wide_channel() -> Result<(), BoxError> {
    let mut fx = build_workspace("pkg-wins")?;
    write_config(
        &mut fx,
        "[versioning]\nchannel = \"beta\"\n\n[[package]]\nname = \"solo\"\nchannel = \"rc\"\n",
    )?;
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
        Some("1.1.0-rc.1"),
        "the override wins over the default: {out}"
    );
    Ok(())
}

/// An invalid override channel fails closed at load (`E-CFG-CHANNEL`),
/// naming the package.
#[test]
fn invalid_package_override_channel_fails_closed() -> Result<(), BoxError> {
    let mut fx = build_workspace("pkg-bad")?;
    write_config(&mut fx, "[[package]]\nname = \"solo\"\nchannel = \"rc.\"\n")?;
    let (code, out) = halyard(fx.root(), &["plan", "--format", "json"])?;
    assert_ne!(code, 0, "invalid override channel must fail: {out}");
    assert!(out.contains("E-CFG-CHANNEL"), "{out}");
    assert!(out.contains("solo"), "the failing package is named: {out}");
    Ok(())
}
