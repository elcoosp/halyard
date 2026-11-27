//! CLI `apply` smoke test (M3.6): builds a real git fixture workspace,
//! runs the compiled binary with `apply --dry` (no writes) and then a real
//! apply, asserting the working tree changed.

use std::process::Command;

use halyard_test_support::{CommitSpec, GitFixture};

type BoxError = Box<dyn std::error::Error>;

fn fixture_dir(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir =
        std::env::temp_dir().join(format!("halyard-cli-apply-{label}-{}", std::process::id()));
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
            "[workspace]\nresolver = \"2\"\nmembers = [\"crates/p1\", \"crates/p2\"]\n".to_owned(),
        ),
        (
            "crates/p1/Cargo.toml".to_owned(),
            "[package]\nname = \"p1\"\nversion = \"0.1.0\"\nedition = \"2021\"\n".to_owned(),
        ),
        (
            "crates/p2/Cargo.toml".to_owned(),
            "[package]\nname = \"p2\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\np1 = \"0.1\"\n".to_owned(),
        ),
        ("crates/p1/src/lib.rs".to_owned(), "pub const C: usize = 1;\n".to_owned()),
        ("crates/p2/src/lib.rs".to_owned(), "pub const C: usize = 2;\n".to_owned()),
    ];
    fx.commit_with(&CommitSpec::files(scaffold, "feat: scaffold workspace"))?;
    Ok(fx)
}

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

#[test]
fn apply_dry_prints_the_edit_set_without_writing() -> Result<(), BoxError> {
    let fx = build_workspace("dry")?;
    let (ok, out) = halyard(fx.root(), &["apply", "--dry"])?;
    assert!(ok, "apply --dry failed: {out}");
    assert!(out.contains("dry run"), "{out}");
    // The CLI outputs forward slashes for file paths in dry-run output
    assert!(out.contains("crates/p1/Cargo.toml"), "{out}");
    assert!(out.contains("\"0.1.1\""), "{out}");
    // --dry wrote nothing.
    let manifest = std::fs::read_to_string(fx.root().join("crates/p1/Cargo.toml"))?;
    assert!(manifest.contains("version = \"0.1.0\""), "{manifest}");
    Ok(())
}

#[test]
fn apply_rewrites_versions_in_the_working_tree() -> Result<(), BoxError> {
    let fx = build_workspace("real")?;
    let (ok, out) = halyard(fx.root(), &["apply"])?;
    assert!(ok, "apply failed: {out}");
    assert!(out.contains("applied"), "{out}");
    let p1 = std::fs::read_to_string(fx.root().join("crates/p1/Cargo.toml"))?;
    assert!(p1.contains("version = \"0.1.1\""), "{p1}");
    // The dependent requirement stays (^0.1-style bare "0.1" admits
    // 0.1.1, so no rewrite fires — only own-version rows change).
    let p2 = std::fs::read_to_string(fx.root().join("crates/p2/Cargo.toml"))?;
    assert!(p2.contains("p1 = \"0.1\""), "{p2}");
    Ok(())
}

// ---------------------------------------------------------------------------
// M8 (ADR-0058): the maven lane — the semantic in-place pom edit. The
// plan's own-version patch lands on the `<project>`-owned `<version>`
// element's value span; the surrounding markup never moves (I4).
// ---------------------------------------------------------------------------

/// A maven multi-module workspace: a root aggregator plus one member with
/// its own version, a feat commit driving the member's plan row (minor:
/// 1.0.0 → 1.1.0).
fn build_maven_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let dir = fixture_dir(label)?;
    let mut fx = GitFixture::init_in(&dir)?;
    let scaffold = vec![
        (
            "pom.xml".to_owned(),
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
                "<project xmlns=\"http://maven.apache.org/POM/4.0.0\">\n",
                "  <modelVersion>4.0.0</modelVersion>\n",
                "  <groupId>com.example</groupId>\n",
                "  <artifactId>halyard-mvn</artifactId>\n",
                "  <version>1.0.0</version>\n",
                "  <packaging>pom</packaging>\n",
                "  <modules>\n    <module>api</module>\n  </modules>\n",
                "</project>\n",
            )
            .to_owned(),
        ),
        (
            "api/pom.xml".to_owned(),
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
                "<project xmlns=\"http://maven.apache.org/POM/4.0.0\">\n",
                "  <modelVersion>4.0.0</modelVersion>\n",
                "  <parent>\n",
                "    <groupId>com.example</groupId>\n",
                "    <artifactId>halyard-mvn</artifactId>\n",
                "    <version>1.0.0</version>\n",
                "  </parent>\n",
                "  <artifactId>halyard-mvn-api</artifactId>\n",
                "  <version>1.0.0</version>\n",
                "</project>\n",
            )
            .to_owned(),
        ),
    ];
    fx.commit_with(&CommitSpec::files(
        scaffold,
        "chore: scaffold maven workspace",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("api/README.md".to_owned(), "member change\n".to_owned())],
        "feat: the member changes",
    ))?;
    Ok(fx)
}

/// `halyard apply` rewrites the member pom's own version in place: the
/// value swaps 1.0.0 → 1.1.0 and every other byte of the pom is
/// identical (I4).
#[test]
fn apply_rewrites_the_pom_version_of_a_maven_workspace() -> Result<(), BoxError> {
    let fx = build_maven_workspace("maven-apply")?;
    let (ok, out) = halyard(fx.root(), &["apply"])?;
    assert!(ok, "apply failed: {out}");
    let api = std::fs::read_to_string(fx.root().join("api/pom.xml"))?;
    assert!(
        api.contains("<version>1.1.0</version>"),
        "the member's own version moved: {api}"
    );
    assert!(
        api.contains("    <version>1.0.0</version>\n  </parent>"),
        "the parent block's version row never moves: {api}"
    );
    // The untouched aggregator: its own version stays 1.0.0 (no commits
    // touched the root's paths — the plan skipped it).
    let root = std::fs::read_to_string(fx.root().join("pom.xml"))?;
    assert!(
        root.contains("<version>1.0.0</version>"),
        "the untouched aggregator keeps its version: {root}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// M8 (ADR-0059): the nuget lane — the semantic in-place csproj edit. The
// plan's own-version patch lands on the element pack consumes
// (`PackageVersion` when present, else `Version`); the surrounding
// markup never moves (I4).
// ---------------------------------------------------------------------------

/// A nuget SDK-style workspace: one project with a `PackageVersion` row,
/// a feat commit driving the plan row (minor: 1.0.0 → 1.1.0).
fn build_nuget_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let dir = fixture_dir(label)?;
    let mut fx = GitFixture::init_in(&dir)?;
    let scaffold = vec![(
        "src/app/App.csproj".to_owned(),
        concat!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n",
            "<Project Sdk=\"Microsoft.NET.Sdk\">\n",
            "  <PropertyGroup>\n",
            "    <TargetFramework>net10.0</TargetFramework>\n",
            "    <PackageId>Contoso.App</PackageId>\n",
            "    <PackageVersion>1.0.0</PackageVersion>\n",
            "  </PropertyGroup>\n",
            "</Project>\n",
        )
        .to_owned(),
    )];
    fx.commit_with(&CommitSpec::files(
        scaffold,
        "chore: scaffold nuget workspace",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "src/app/README.md".to_owned(),
            "the app changes\n".to_owned(),
        )],
        "feat: the app changes",
    ))?;
    Ok(fx)
}

/// `halyard apply` rewrites the csproj's pack version in place: the
/// value swaps 1.0.0 → 1.1.0 and every other byte of the project file
/// is identical (I4).
#[test]
fn apply_rewrites_the_csproj_version_of_a_nuget_workspace() -> Result<(), BoxError> {
    let fx = build_nuget_workspace("nuget-apply")?;
    let (ok, out) = halyard(fx.root(), &["apply"])?;
    assert!(ok, "apply failed: {out}");
    let app = std::fs::read_to_string(fx.root().join("src/app/App.csproj"))?;
    assert!(
        app.contains("<PackageVersion>1.1.0</PackageVersion>"),
        "the pack version moved: {app}"
    );
    assert!(
        app.contains("    <TargetFramework>net10.0</TargetFramework>"),
        "the surrounding markup never moves: {app}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// M8 (ADR-0060): the dart lane — the semantic in-place pubspec edit. The
// plan's own-version patch lands on the `version` value's span; the
// surrounding YAML (quotes, comments, every other row) never moves (I4).
// ---------------------------------------------------------------------------

/// A dart pub workspace: the root plus one member with a canonical
/// `version` row, a feat commit driving the member's plan row (minor:
/// 1.0.0 → 1.1.0).
fn build_dart_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let dir = fixture_dir(label)?;
    let mut fx = GitFixture::init_in(&dir)?;
    let scaffold = vec![
        (
            "pubspec.yaml".to_owned(),
            concat!(
                "# the pub workspace root (the documented shape)\n",
                "name: _\n",
                "publish_to: none\n",
                "environment:\n  sdk: ^3.6.0\n",
                "workspace:\n  - packages/app\n",
            )
            .to_owned(),
        ),
        (
            "packages/app/pubspec.yaml".to_owned(),
            concat!(
                "# the app member\n",
                "name: app\n",
                "version: 1.0.0\n",
                "resolution: workspace\n",
                "dependencies:\n  http: ^1.2.0\n",
            )
            .to_owned(),
        ),
    ];
    fx.commit_with(&CommitSpec::files(
        scaffold,
        "chore: scaffold dart workspace",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "packages/app/README.md".to_owned(),
            "the app changes\n".to_owned(),
        )],
        "feat: the app changes",
    ))?;
    Ok(fx)
}

/// `halyard apply` rewrites the pubspec's version in place: the value
/// swaps 1.0.0 → 1.1.0 and every other byte of the pubspec is identical
/// (I4) — the comment, the constraint row, the resolution marker.
#[test]
fn apply_rewrites_the_pubspec_version_of_a_dart_workspace() -> Result<(), BoxError> {
    let fx = build_dart_workspace("dart-apply")?;
    let (ok, out) = halyard(fx.root(), &["apply"])?;
    assert!(ok, "apply failed: {out}");
    let app = std::fs::read_to_string(fx.root().join("packages/app/pubspec.yaml"))?;
    assert!(app.contains("version: 1.1.0\n"), "the version moved: {app}");
    assert!(
        app.contains("# the app member\nname: app\n") && app.contains("http: ^1.2.0"),
        "the surrounding YAML never moves: {app}"
    );
    Ok(())
}
