//! CLI e2e over the M8 Maven/Gradle cell's Gradle half **dependency
//! edges** (ADR-0058 continued — the recorded-open
//! `project(":sub")` remainder). A settings-governed repository whose
//! root build file declares `implementation project(':library')`: the
//! plan's topological publish order moves the member **before** its
//! dependent — the differential against the same repository without
//! the edge (discovery order) is the proof the proven edge landed in
//! the workspace graph. The VERIFIED grammar in
//! `docs/research/gradle-grammars.md` §7; the requirement rides
//! `ReqSpec::PathOnly` (a project dependency carries no version
//! constraint — nothing to rewrite; the cascade policy stays the
//! planner's own and `[[link]]` remains the forced-cascade surface).

use std::process::Command;

use halyard_test_support::{CommitSpec, GitFixture};

type BoxError = Box<dyn std::error::Error>;

fn fixture_dir(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir = std::env::temp_dir().join(format!(
        "halyard-cli-gradle-deps-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// A settings-governed repository whose root depends on the library
/// member with the documented project-path notation. `with_dep`
/// toggles the `dependencies { … }` row — the differential control.
fn build_deps_workspace(label: &str, with_dep: bool) -> Result<GitFixture, BoxError> {
    let dir = fixture_dir(label)?;
    let mut fx = GitFixture::init_in(&dir)?;
    let mut files: Vec<(String, String)> = vec![
        (
            "settings.gradle".to_owned(),
            concat!(
                "rootProject.name = 'polyglot-parent'\n",
                "\n",
                "include 'library'\n",
            )
            .to_owned(),
        ),
        (
            "build.gradle".to_owned(),
            if with_dep {
                concat!(
                    "version = '1.0.0'\n",
                    "\n",
                    "dependencies {\n",
                    "    implementation project(':library')\n",
                    "}\n",
                )
                .to_owned()
            } else {
                "version = '1.0.0'\n".to_owned()
            },
        ),
        (
            "library/build.gradle".to_owned(),
            "version = '2.0.0'\n".to_owned(),
        ),
    ];
    files.push(("src/App.java".to_owned(), "// the root\n".to_owned()));
    files.push((
        "library/src/Lib.java".to_owned(),
        "// the library\n".to_owned(),
    ));
    fx.commit_with(&CommitSpec::files(
        files,
        "chore: scaffold the gradle build",
    ))?;
    // Both packages release: a feat commit on each subtree.
    fx.commit_with(&CommitSpec::files(
        vec![(
            "src/App.java".to_owned(),
            "// the root changes\n".to_owned(),
        )],
        "feat: the root changes",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "library/src/Lib.java".to_owned(),
            "// the library changes\n".to_owned(),
        )],
        "feat: the library changes",
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

/// The proven edge lands in the workspace graph: the plan's
/// topological publish order puts the dependency `library` before its
/// dependent `polyglot-parent` — reversed against the same repository
/// without the edge row (the differential control's discovery order).
#[test]
fn the_project_edge_orders_the_dependency_before_its_dependent() -> Result<(), BoxError> {
    let with_edge = build_deps_workspace("edge", true)?;
    let (out, text) = halyard(with_edge.root(), &["plan", "--format", "json"])?;
    assert!(out.status.success(), "the edge repository plans: {text}");
    let value = report(&text)?;
    let order: Vec<&str> = value
        .get("order")
        .and_then(serde_json::Value::as_array)
        .ok_or("order is an array")?
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    assert_eq!(
        order,
        vec!["library", "polyglot-parent"],
        "the dependency publishes before its dependent: {order:?}"
    );
    // The control: the same repository without the `project(':…')` row
    // keeps the discovery order — the reversal is the edge's work.
    let control = build_deps_workspace("control", false)?;
    let (out, text) = halyard(control.root(), &["plan", "--format", "json"])?;
    assert!(out.status.success(), "the control repository plans: {text}");
    let value = report(&text)?;
    let order: Vec<&str> = value
        .get("order")
        .and_then(serde_json::Value::as_array)
        .ok_or("order is an array")?
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    assert_eq!(
        order,
        vec!["polyglot-parent", "library"],
        "no edge, no reordering: {order:?}"
    );
    Ok(())
}

/// The path-only edge carries no version constraint: the dependent
/// plans from its own commits (the planner's when-unsatisfied policy —
/// a `PathOnly` edge never mismatches, nothing is rewritten, and the
/// row's `from` is its own build-file literal).
#[test]
fn the_path_only_edge_rewrites_nothing() -> Result<(), BoxError> {
    let fx = build_deps_workspace("path-only", true)?;
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
        .ok_or("the dependent releases")?;
    assert_eq!(
        root.get("from").and_then(serde_json::Value::as_str),
        Some("1.0.0"),
        "the dependent plans from its own literal"
    );
    assert_eq!(
        root.get("to").and_then(serde_json::Value::as_str),
        Some("1.1.0"),
        "the feat commit drives the bump — the edge adds no cascade rewrite"
    );
    let edits = value
        .get("edits")
        .and_then(serde_json::Value::as_array)
        .ok_or("edits is an array")?;
    let edit_files: Vec<&str> = edits
        .iter()
        .filter_map(|edit| edit.get("file").and_then(serde_json::Value::as_str))
        .collect();
    // The edit set carries exactly the two build files' version
    // literals — the dependency edge contributes no patch (a
    // `PathOnly` edge has no requirement text to rewrite).
    let mut sorted: Vec<&str> = edit_files.clone();
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        vec!["build.gradle", "library/build.gradle"],
        "the edit set is the two version literals, nothing from the edge: {edit_files:?}"
    );
    Ok(())
}
