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
