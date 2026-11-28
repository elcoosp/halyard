//! CLI e2e over the M8 Maven/Gradle cell's Gradle half **settings-file
//! governance** (ADR-0058 continued — the multi-project aggregator
//! shape, the recorded-open remainder the Gradle-half note names). A
//! repository whose `settings.gradle` names the root
//! (`rootProject.name = '…'`) and proves two members via `include`:
//! the plan rows carry the settings-proven names, the versions ride the
//! build files' documented literal assignments, and the edit set aims
//! at the governed manifests.
//!
//! VERIFIED grammar in `docs/research/gradle-grammars.md` §6 (the
//! Settings API: the root project's name *"defaults to the name of the
//! directory containing the settings file"*; `include` — *"The last
//! element of the supplied path is used as the project name"* and
//! *"the supplied path is converted to a project directory relative to
//! the root project directory"*).

use std::process::Command;

use halyard_test_support::{CommitSpec, GitFixture};

type BoxError = Box<dyn std::error::Error>;

fn fixture_dir(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir = std::env::temp_dir().join(format!(
        "halyard-cli-settings-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// A settings-governed Gradle repository: the root build file named by
/// `rootProject.name`, two include-proven members, and one unincluded
/// build file that keeps the standalone row. The feat commit drives the
/// root row's plan.
fn build_settings_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let dir = fixture_dir(label)?;
    let mut fx = GitFixture::init_in(&dir)?;
    fx.commit_with(&CommitSpec::files(
        vec![
            (
                "settings.gradle".to_owned(),
                concat!(
                    "rootProject.name = 'polyglot-parent'\n",
                    "\n",
                    "include 'library'\n",
                    "include ':services:api'\n",
                )
                .to_owned(),
            ),
            ("build.gradle".to_owned(), "version = '1.0.0'\n".to_owned()),
            (
                "library/build.gradle".to_owned(),
                "version = '2.0.0'\n".to_owned(),
            ),
            (
                "services/api/build.gradle.kts".to_owned(),
                "version = \"0.3.0\"\n".to_owned(),
            ),
            (
                "extra/build.gradle".to_owned(),
                "version = '3.0.0'\n".to_owned(),
            ),
        ],
        "chore: scaffold the settings-governed gradle repository",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "src/main/java/App.java".to_owned(),
            "// the root changes\n".to_owned(),
        )],
        "feat: the root changes",
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

fn report(text: &str) -> Result<serde_json::Value, BoxError> {
    serde_json::from_str(
        text.trim_start_matches(char::is_whitespace)
            .trim_start_matches(|c: char| c != '{'),
    )
    .map_err(|e| format!("the report is JSON: {e}\n{text}").into())
}

/// The governed repository plans end-to-end: the root row is named by
/// `rootProject.name`, the members by their include paths' last
/// elements, the unincluded build file keeps the standalone shape, and
/// the release rows ride the Maven ecosystem (the Maven/Gradle cell's
/// single ecosystem — the settings governance changes names, never the
/// row's other shapes).
#[test]
fn a_settings_governed_repository_plans_the_proven_names() -> Result<(), BoxError> {
    let fx = build_settings_workspace("plan")?;
    let (out, text) = halyard(fx.root(), &["plan", "--format", "json"])?;
    assert!(
        out.status.success(),
        "the settings-governed repository plans: {text}"
    );
    let value = report(&text)?;
    let releases = value
        .get("releases")
        .and_then(serde_json::Value::as_array)
        .ok_or("releases is an array")?;
    let skipped = value
        .get("skipped")
        .and_then(serde_json::Value::as_array)
        .ok_or("skipped is an array")?;
    let release_names: Vec<&str> = releases
        .iter()
        .filter_map(|row| row.get("package").and_then(serde_json::Value::as_str))
        .collect();
    let skipped_names: Vec<&str> = skipped
        .iter()
        .filter_map(|row| row.get("package").and_then(serde_json::Value::as_str))
        .collect();
    // The root row releases (the feat commit touches its subtree); the
    // members without relevant commits land in `skipped` with their
    // proven names — every governed row carries the settings-proven
    // identity.
    assert_eq!(
        release_names,
        vec!["polyglot-parent"],
        "the root is named by rootProject.name: {release_names:?}"
    );
    for name in ["library", "api", "extra"] {
        assert!(
            skipped_names.contains(&name),
            "the {name:?} row carries its proven name in skipped: {skipped_names:?}"
        );
    }
    Ok(())
}

/// The plan is idempotent and byte-stable across runs (invariant I3):
/// the same governed tree yields the same report twice.
#[test]
fn the_governed_plan_is_deterministic_across_runs() -> Result<(), BoxError> {
    let fx = build_settings_workspace("determinism")?;
    let (first_out, first) = halyard(fx.root(), &["plan", "--format", "json"])?;
    let (second_out, second) = halyard(fx.root(), &["plan", "--format", "json"])?;
    assert!(first_out.status.success(), "the first plan: {first}");
    assert!(second_out.status.success(), "the second plan: {second}");
    let first_value = report(&first)?;
    let second_value = report(&second)?;
    assert_eq!(
        first_value.get("plan_hash"),
        second_value.get("plan_hash"),
        "the same tree hashes to the same plan (I3)"
    );
    Ok(())
}

/// The governed root's edit set aims at the root build file's version
/// literal — the settings governance names the row; the edit seam is
/// unchanged (the version lives in the build file, not the settings
/// file, which carries no version slot).
#[test]
fn the_governed_root_edit_targets_the_build_file_literal() -> Result<(), BoxError> {
    let fx = build_settings_workspace("edit")?;
    let (out, text) = halyard(fx.root(), &["plan", "--format", "json"])?;
    assert!(out.status.success(), "the plan: {text}");
    let value = report(&text)?;
    let releases = value
        .get("releases")
        .and_then(serde_json::Value::as_array)
        .ok_or("releases is an array")?;
    let root = releases
        .iter()
        .find(|row| {
            row.get("package").and_then(serde_json::Value::as_str) == Some("polyglot-parent")
        })
        .ok_or("the governed root releases")?;
    assert_eq!(
        root.get("from").and_then(serde_json::Value::as_str),
        Some("1.0.0"),
        "the root build file's literal is the from version"
    );
    let edits = value
        .get("edits")
        .and_then(serde_json::Value::as_array)
        .ok_or("edits is an array")?;
    let edit = edits
        .iter()
        .find(|edit| edit.get("file").and_then(serde_json::Value::as_str) == Some("build.gradle"))
        .ok_or("the root build file carries the edit")?;
    let patches = edit
        .get("patches")
        .and_then(serde_json::Value::as_array)
        .ok_or("patches is an array")?;
    assert!(
        !patches.is_empty(),
        "the version patch aims at the root build file's literal"
    );
    Ok(())
}
