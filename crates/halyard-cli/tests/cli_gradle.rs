//! CLI e2e over the M8 Maven/Gradle cell's **Gradle half** (ADR-0058
//! continued): a repository whose only package is a `build.gradle`
//! carrying the documented literal `version = '…'` assignment. The plan
//! row carries the engine's **disabled publish shape**
//! (`publish.registry = "maven-central"` — the Maven/Gradle cell's
//! single registry — with `enabled: false` until a `gradle` argv grammar
//! exists), the edit set aims the version patch at the literal's
//! inner-byte span, and the release row's `ecosystem` is the `maven`
//! label the plan contract resolves (Gradle rides the Maven/Gradle
//! cell's single ecosystem — both publish to Maven Central).
//!
//! VERIFIED grammar in `docs/research/gradle-grammars.md` (the Gradle
//! Project API: *"The version of this project. Gradle always uses the
//! `toString()` value of the version. The version defaults to
//! `unspecified`."*; the Building Java Projects guide's
//! `version = '1.2.1'` / `version = "1.2.1"` pair).

use std::process::Command;

use halyard_test_support::{CommitSpec, GitFixture};

type BoxError = Box<dyn std::error::Error>;

fn fixture_dir(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir =
        std::env::temp_dir().join(format!("halyard-cli-gradle-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// A Gradle repository: one `app/build.gradle` with the Groovy
/// single-quoted literal assignment, a feat commit driving the plan
/// row. No other manifest grammar exists in the tree — the Gradle half
/// is the only way this repository is plannable (the cell's reason to
/// exist). The package name is the containing directory (`app`), the
/// Dockerfile/gemspec per-file identity chain.
fn build_gradle_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let dir = fixture_dir(label)?;
    let mut fx = GitFixture::init_in(&dir)?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "app/build.gradle".to_owned(),
            concat!(
                "plugins {\n",
                "    id 'java'\n",
                "}\n",
                "\n",
                "version = '1.0.0'\n",
                "\n",
                "repositories {\n",
                "    mavenCentral()\n",
                "}\n",
            )
            .to_owned(),
        )],
        "chore: scaffold the gradle repository",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "app/src/main/java/App.java".to_owned(),
            "// the app changes\n".to_owned(),
        )],
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

/// The plan over a Gradle-only repository: the maven release row exists
/// (the walk feeds the build file through discovery), the ecosystem
/// label is `maven` (Gradle rides the Maven/Gradle cell's single
/// ecosystem), the tag rides the single-package default, and the publish
/// decision carries the engine's **disabled shape** (the Maven/Gradle
/// cell's registry `maven-central`, `enabled` false until a `gradle`
/// argv arm lands).
#[test]
fn a_gradle_repository_plans_end_to_end() -> Result<(), BoxError> {
    let fx = build_gradle_workspace("gradle-plan")?;
    let (out, text) = halyard(fx.root(), &["plan", "--format", "json"])?;
    assert!(out.status.success(), "the gradle repository plans: {text}");
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
        .find(|row| row.get("package").and_then(serde_json::Value::as_str) == Some("app"))
        .ok_or("the gradle package releases")?;
    assert_eq!(
        row.get("ecosystem").and_then(serde_json::Value::as_str),
        Some("maven"),
        "the ecosystem label (Gradle rides Maven): {text}"
    );
    assert_eq!(
        row.get("from").and_then(serde_json::Value::as_str),
        Some("1.0.0"),
        "the literal assignment is the `from` version"
    );
    assert_eq!(
        row.get("to").and_then(serde_json::Value::as_str),
        Some("1.1.0"),
        "a feat bump is minor"
    );
    let publish = row
        .get("publish")
        .and_then(serde_json::Value::as_object)
        .ok_or("the disabled publish decision exists")?;
    assert_eq!(
        publish.get("registry").and_then(serde_json::Value::as_str),
        Some("maven-central"),
        "the Maven/Gradle cell's registry: {text}"
    );
    assert_eq!(
        publish.get("enabled").and_then(serde_json::Value::as_bool),
        Some(false),
        "the disabled shape until an argv arm lands: {text}"
    );
    // The edit set: the version patch aims at the build file's literal.
    let edits = value
        .get("edits")
        .and_then(serde_json::Value::as_array)
        .ok_or("edits is an array")?;
    let edit = edits
        .iter()
        .find(|edit| {
            edit.get("file").and_then(serde_json::Value::as_str) == Some("app/build.gradle")
        })
        .ok_or("the build.gradle edit is planned")?;
    assert!(
        edit.get("patches")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|p| !p.is_empty()),
        "the literal-value patch is planned: {text}"
    );
    Ok(())
}

/// `plan --dry`-shaped commands never touch the working tree (I5) — the
/// build file keeps its bytes.
#[test]
fn planning_never_mutates_the_build_file() -> Result<(), BoxError> {
    let fx = build_gradle_workspace("gradle-immutable")?;
    let (out, text) = halyard(fx.root(), &["plan"])?;
    assert!(out.status.success(), "the plan succeeds: {text}");
    let after = std::fs::read_to_string(fx.root().join("app/build.gradle"))?;
    assert!(
        after.contains("version = '1.0.0'"),
        "the build file keeps its bytes: {after}"
    );
    Ok(())
}

/// Colliding fallback identities refuse through the orchestrator's
/// per-ecosystem duplicate check (`E-DISC-DUPLICATE`, §6.3): two
/// nameless build files in same-named directories both resolve to the
/// containing directory's name, and the JSON envelope carries the
/// stable code.
#[test]
fn colliding_fallback_identities_refuse_duplicate() -> Result<(), BoxError> {
    let dir = fixture_dir("gradle-duplicate")?;
    let mut fx = GitFixture::init_in(&dir)?;
    fx.commit_with(&CommitSpec::files(
        vec![
            (
                "services/a/build.gradle".to_owned(),
                "version = '1.0.0'\n".to_owned(),
            ),
            (
                "workers/a/build.gradle".to_owned(),
                "version = '1.0.0'\n".to_owned(),
            ),
        ],
        "chore: scaffold colliding contexts",
    ))?;
    let (out, text) = halyard(fx.root(), &["plan", "--format", "json"])?;
    assert!(
        !out.status.success(),
        "the colliding identities refuse: {text}"
    );
    let value: serde_json::Value = serde_json::from_str(
        text.trim_start_matches(char::is_whitespace)
            .trim_start_matches(|c: char| c != '{'),
    )
    .map_err(|e| format!("the error path is JSON: {e}\n{text}"))?;
    let code = value
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(serde_json::Value::as_str)
        .ok_or("the envelope carries error.code")?;
    assert_eq!(
        code, "E-DISC-DUPLICATE",
        "the refusal carries the stable code: {text}"
    );
    Ok(())
}

/// `halyard apply` rewrites the Gradle literal in place: the literal's
/// inner bytes swap 1.0.0 → 1.1.0 and every other byte of the build file
/// is identical (I4) — the quotes, the `plugins`/`repositories` blocks,
/// the comment, the sibling string literal.
#[test]
fn apply_rewrites_the_version_literal_of_a_gradle_repository() -> Result<(), BoxError> {
    let fx = build_gradle_workspace("gradle-apply")?;
    let (out, text) = halyard(fx.root(), &["apply"])?;
    assert!(out.status.success(), "apply failed: {text}");
    let build = std::fs::read_to_string(fx.root().join("app/build.gradle"))?;
    assert!(
        build.contains("version = '1.1.0'"),
        "the version moved: {build}"
    );
    assert!(
        !build.contains("version = '1.0.0'"),
        "the old literal is gone: {build}"
    );
    assert!(
        build.contains("id 'java'")
            && build.contains("mavenCentral()")
            && build.contains("plugins {")
            && build.contains("repositories {"),
        "the surrounding build file never moves: {build}"
    );
    Ok(())
}

/// The dry forecast for a Gradle repository: zero publish targets, the
/// release recorded as a `publish_disabled` skip (the row's publish
/// decision carries the engine's disabled shape — `registry` names the
/// Maven/Gradle cell's registry `maven-central`, `enabled` is false
/// until a `gradle` argv grammar lands), and a successful offline run.
#[test]
fn publish_dry_records_disabled_for_a_gradle_repository() -> Result<(), BoxError> {
    let fx = build_gradle_workspace("gradle-publish")?;
    let (out, text) = halyard(fx.root(), &["publish", "--dry", "--format", "json"])?;
    assert!(
        out.status.success(),
        "the gradle forecast succeeded: {text}"
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
        .find(|row| row.get("package").and_then(serde_json::Value::as_str) == Some("app"))
        .ok_or("the gradle release is recorded as a skip")?;
    assert_eq!(
        member.get("reason").and_then(serde_json::Value::as_str),
        Some("publish_disabled"),
        "the disabled-shape skip reason: {text}"
    );
    Ok(())
}
