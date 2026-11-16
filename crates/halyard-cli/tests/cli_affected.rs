//! CLI `affected` gates (§16.3/M10, the affected-package selector): the
//! shell walks the pending history (the same pipeline `plan` uses),
//! closes over the workspace's reverse edges and renders the selection
//! in all three formats. Pins the CI-consumer contracts:
//!
//! * a change under one package root affects that package (`changed`)
//!   and every transitive dependent (`dependent`, with the nearest
//!   changed ancestor as `via`);
//! * `--base <ref>` attributes against an explicit ref instead of the
//!   release bases (the PR-selection shape);
//! * an unresolvable ref refuses fail-closed (`E-GIT-REF` naming the
//!   ref);
//! * a fully-tagged repo is an honest empty selection (exit 0);
//! * the JSON document is byte-stable across runs (I3).

use std::process::Command;

use halyard_test_support::fixture::{CommitSpec, GitFixture};

type BoxError = Box<dyn std::error::Error>;

/// Runs the binary; returns (exit ok, combined output).
fn halyard(root: &std::path::Path, args: &[&str]) -> Result<(bool, String), BoxError> {
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
    Ok((out.status.success(), text))
}

fn parse_json(text: &str) -> Result<serde_json::Value, BoxError> {
    serde_json::from_str(text.trim())
        .map_err(|e| format!("stdout is one JSON document: {e}\n{text}").into())
}

/// A two-package cargo workspace (`lib` ← `app`): the scaffold commit
/// touches both packages, then one commit touches only `lib`.
fn two_package_fixture(label: &str) -> Result<GitFixture, BoxError> {
    let mut fx = GitFixture::new(label)?;
    let scaffold = vec![
        (
            "Cargo.toml".to_owned(),
            "[workspace]\nresolver = \"2\"\nmembers = [\"crates/lib\", \"crates/app\"]\n".to_owned(),
        ),
        (
            "crates/lib/Cargo.toml".to_owned(),
            "[package]\nname = \"lib\"\nversion = \"0.1.0\"\nedition = \"2021\"\n".to_owned(),
        ),
        (
            "crates/lib/src/lib.rs".to_owned(),
            "pub const L: usize = 1;\n".to_owned(),
        ),
        (
            "crates/app/Cargo.toml".to_owned(),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nlib = { version = \"0.1\", path = \"../lib\" }\n".to_owned(),
        ),
        (
            "crates/app/src/lib.rs".to_owned(),
            "pub const A: usize = 2;\n".to_owned(),
        ),
    ];
    fx.commit_with(&CommitSpec::files(scaffold, "feat: scaffold workspace"))?;
    fx.write("crates/lib/src/lib.rs", "pub const L: usize = 2;\n")?;
    fx.stage_all()?;
    fx.commit("fix(lib): patch")?;
    Ok(fx)
}

#[test]
fn affected_reports_changed_and_dependents() -> Result<(), BoxError> {
    let fx = two_package_fixture("cli-affected-closure")?;
    // Both packages released at the scaffold; the lib patch is pending.
    fx.run_git(&["tag", "-a", "lib-v0.1.0", "-m", "lib 0.1.0", "HEAD~1"])?;
    fx.run_git(&["tag", "-a", "app-v0.1.0", "-m", "app 0.1.0", "HEAD~1"])?;
    let (ok, text) = halyard(fx.root(), &["affected", "--format", "json"])?;
    assert!(ok, "the selection is the honest happy path: {text}");
    let value = parse_json(&text)?;
    assert_eq!(value.get("total"), Some(&serde_json::json!(2)));
    assert_eq!(value.get("changed"), Some(&serde_json::json!(["lib"])));
    let packages = value
        .get("packages")
        .and_then(|p| p.as_array())
        .ok_or("the report carries a packages array")?;
    assert_eq!(packages.len(), 2, "lib changed, app is its dependent");
    let app = packages
        .iter()
        .find(|row| row.get("package") == Some(&serde_json::json!("app")))
        .ok_or("app is a reported row")?;
    assert_eq!(app.get("reason"), Some(&serde_json::json!("dependent")));
    assert_eq!(app.get("via"), Some(&serde_json::json!("lib")));
    let lib = packages
        .iter()
        .find(|row| row.get("package") == Some(&serde_json::json!("lib")))
        .ok_or("lib is a reported row")?;
    assert_eq!(lib.get("reason"), Some(&serde_json::json!("changed")));
    Ok(())
}

#[test]
fn affected_base_override_selects_against_the_ref() -> Result<(), BoxError> {
    let mut fx = two_package_fixture("cli-affected-base")?;
    // `origin/main` sits at the scaffold; the lib patch is after it.
    fx.branch("origin/main", Some("HEAD~1"))?;
    let (ok, text) = halyard(
        fx.root(),
        &["affected", "--base", "origin/main", "--format", "json"],
    )?;
    assert!(ok, "the override run succeeds: {text}");
    let value = parse_json(&text)?;
    assert_eq!(value.get("base"), Some(&serde_json::json!("origin/main")));
    assert_eq!(
        value.get("changed"),
        Some(&serde_json::json!(["lib"])),
        "against the ref only the lib commit is attributed"
    );
    let packages = value
        .get("packages")
        .and_then(|p| p.as_array())
        .ok_or("the report carries a packages array")?;
    assert_eq!(packages.len(), 2);
    let app = packages
        .iter()
        .find(|row| row.get("package") == Some(&serde_json::json!("app")))
        .ok_or("app is a reported row")?;
    assert_eq!(app.get("reason"), Some(&serde_json::json!("dependent")));
    Ok(())
}

#[test]
fn affected_unknown_base_refuses_fail_closed() -> Result<(), BoxError> {
    let fx = two_package_fixture("cli-affected-unknown-ref")?;
    let (ok, text) = halyard(
        fx.root(),
        &["affected", "--base", "does-not-exist", "--format", "json"],
    )?;
    assert!(!ok, "an unresolvable ref refuses");
    let value = parse_json(&text)?;
    assert_eq!(
        value.pointer("/error/code"),
        Some(&serde_json::json!("E-GIT-REF")),
        "the refusal carries the stable code: {text}"
    );
    assert!(
        value
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .is_some_and(|m| m.contains("does-not-exist")),
        "the refusal names the ref: {text}"
    );
    Ok(())
}

#[test]
fn affected_fully_tagged_is_an_honest_empty() -> Result<(), BoxError> {
    let mut fx = two_package_fixture("cli-affected-empty")?;
    // Both packages released at HEAD: nothing pending, nobody affected.
    fx.tag_annotated("lib-v0.1.1", "lib 0.1.1")?;
    fx.tag_annotated("app-v0.1.1", "app 0.1.1")?;
    let (ok, text) = halyard(fx.root(), &["affected", "--format", "json"])?;
    assert!(ok, "an empty selection is still a successful run: {text}");
    let value = parse_json(&text)?;
    assert_eq!(value.get("total"), Some(&serde_json::json!(0)));
    assert_eq!(value.get("changed"), Some(&serde_json::json!([])));
    assert_eq!(
        value.get("packages"),
        Some(&serde_json::json!([])),
        "no rows — nothing changed, nothing depends on a change"
    );
    Ok(())
}

#[test]
fn affected_renders_are_stable_across_runs() -> Result<(), BoxError> {
    let fx = two_package_fixture("cli-affected-stable")?;
    fx.run_git(&["tag", "-a", "lib-v0.1.0", "-m", "lib 0.1.0", "HEAD~1"])?;
    fx.run_git(&["tag", "-a", "app-v0.1.0", "-m", "app 0.1.0", "HEAD~1"])?;
    let (ok, first) = halyard(fx.root(), &["affected", "--format", "json"])?;
    assert!(ok, "{first}");
    let (ok, second) = halyard(fx.root(), &["affected", "--format", "json"])?;
    assert!(ok, "{second}");
    assert_eq!(first, second, "I3: the JSON document is byte-stable");
    let (ok, human) = halyard(fx.root(), &["affected"])?;
    assert!(ok, "{human}");
    assert!(
        human.contains("app") && human.contains("dependent") && human.contains("lib"),
        "the human table names the package, its reason and the via hop: {human}"
    );
    let (ok, gha) = halyard(fx.root(), &["affected", "--format", "gha"])?;
    assert!(ok, "{gha}");
    assert!(
        gha.contains("affected=2"),
        "the gha render carries the selection summary: {gha}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// The CI-selector adapters (§16.3/M10 step 2, ADR-0034): `affected --ci
// turbo|nx|bazel` renders the selection in the CI system's own grammar
// (the VERIFIED quotes live in `docs/research/ci-selector-grammars.md`).
// The three formats stay untouched; `--ci` is the machine-consumer
// surface. Written first (red), implemented second.
// ---------------------------------------------------------------------------

#[test]
fn affected_ci_turbo_renders_the_dependent_filters() -> Result<(), BoxError> {
    let fx = two_package_fixture("cli-affected-ci-turbo")?;
    fx.run_git(&["tag", "-a", "lib-v0.1.0", "-m", "lib 0.1.0", "HEAD~1"])?;
    fx.run_git(&["tag", "-a", "app-v0.1.0", "-m", "app 0.1.0", "HEAD~1"])?;
    let (ok, text) = halyard(fx.root(), &["affected", "--ci", "turbo"])?;
    assert!(ok, "the turbo adapter renders: {text}");
    assert!(
        text.contains("--filter=...lib"),
        "the changed root's dependent filter: {text}"
    );
    Ok(())
}

#[test]
fn affected_ci_nx_renders_the_projects_flag() -> Result<(), BoxError> {
    let fx = two_package_fixture("cli-affected-ci-nx")?;
    fx.run_git(&["tag", "-a", "lib-v0.1.0", "-m", "lib 0.1.0", "HEAD~1"])?;
    fx.run_git(&["tag", "-a", "app-v0.1.0", "-m", "app 0.1.0", "HEAD~1"])?;
    let (ok, text) = halyard(fx.root(), &["affected", "--ci", "nx"])?;
    assert!(ok, "the nx adapter renders: {text}");
    assert!(
        text.contains("--projects=app,lib"),
        "the affected closure name-sorted: {text}"
    );
    Ok(())
}

#[test]
fn affected_ci_bazel_renders_the_rdeps_query() -> Result<(), BoxError> {
    let fx = two_package_fixture("cli-affected-ci-bazel")?;
    fx.run_git(&["tag", "-a", "lib-v0.1.0", "-m", "lib 0.1.0", "HEAD~1"])?;
    fx.run_git(&["tag", "-a", "app-v0.1.0", "-m", "app 0.1.0", "HEAD~1"])?;
    let (ok, text) = halyard(fx.root(), &["affected", "--ci", "bazel"])?;
    assert!(ok, "the bazel adapter renders: {text}");
    assert!(
        text.contains("rdeps(//..., //crates/lib/...)"),
        "the rdeps query over the changed package's recursive pattern: {text}"
    );
    Ok(())
}
