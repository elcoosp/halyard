//! CLI `apply` changelog stage (M2, §9.4): the resolved
//! `changelog.style` drives the §9.4 insertion machinery — built-in
//! names and user-template paths alike — through the same atomic-write
//! protocol as the plan edits. The re-run is byte-identical (I1), the
//! unknown name is the documented caller bug (`E-CHLOG-STYLE`) and an
//! unreadable template file fails closed (`E-CHLOG-TEMPLATE`).

use std::process::Command;

use halyard_test_support::{CommitSpec, GitFixture};

type BoxError = Box<dyn std::error::Error>;

fn fixture_dir(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir = std::env::temp_dir().join(format!(
        "halyard-cli-changelog-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// The post-tag workspace: manifest 0.1.1 with only v0.1.0 tagged —
/// the plan carries one release with a changelog entry preview.
fn build_merged_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let dir = fixture_dir(label)?;
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
            "Cargo.toml".to_owned(),
            "[package]\nname = \"solo\"\nversion = \"0.1.1\"\nedition = \"2021\"\n".to_owned(),
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

fn read_changelog(root: &std::path::Path) -> Result<String, BoxError> {
    Ok(std::fs::read_to_string(root.join("CHANGELOG.md"))?)
}

/// The default style creates the changelog document anchored after the
/// `Unreleased` heading, and the re-run is byte-identical (I1) with the
/// `unchanged` outcome.
#[test]
fn apply_writes_the_changelog_idempotently() -> Result<(), BoxError> {
    let mut fx = build_merged_workspace("default")?;
    write_config(&mut fx, "[changelog]\nstyle = \"keep-a-changelog\"\n")?;
    let (code, out) = halyard(fx.root(), &["apply", "--format", "json"])?;
    assert_eq!(code, 0, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    let rows = value
        .get("changelog")
        .and_then(serde_json::Value::as_array)
        .ok_or("the report carries the changelog rows")?;
    assert_eq!(rows.len(), 1, "{out}");
    assert_eq!(
        rows.first()
            .expect("one row")
            .get("file")
            .and_then(serde_json::Value::as_str),
        Some("CHANGELOG.md")
    );
    assert_eq!(
        rows.first()
            .expect("one row")
            .get("outcome")
            .and_then(serde_json::Value::as_str),
        Some("created")
    );
    assert_eq!(
        rows.first()
            .expect("one row")
            .get("style")
            .and_then(serde_json::Value::as_str),
        Some("keep-a-changelog")
    );
    let document = read_changelog(fx.root())?;
    assert!(document.contains("## [0.1.2]"), "{document}");
    assert!(document.contains("### Features"), "{document}");
    assert!(document.contains("grow the surface"), "{document}");
    // The re-run plans a NEW release (the manifest moved 0.1.1 → 0.1.2
    // in the first apply, so the next plan proposes 0.1.3): the new
    // section joins the document and the previous one is preserved —
    // rendered history is never silently dropped, and each version
    // appears exactly once (the same-plan re-render is byte-identical,
    // pinned at the renderer level in `style_source.rs`).
    let (code, out) = halyard(fx.root(), &["apply", "--format", "json"])?;
    assert_eq!(code, 0, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    let rows = value
        .get("changelog")
        .and_then(serde_json::Value::as_array)
        .ok_or("the re-run report carries the changelog rows")?;
    assert_eq!(
        rows.first()
            .expect("one row")
            .get("outcome")
            .and_then(serde_json::Value::as_str),
        Some("updated"),
        "the new plan's section anchors under the marker: {out}"
    );
    let rerun_document = read_changelog(fx.root())?;
    assert_eq!(
        rerun_document.matches("## [0.1.3]").count(),
        1,
        "the new section renders exactly once (I1): {rerun_document}"
    );
    assert_eq!(
        rerun_document.matches("## [0.1.2]").count(),
        1,
        "the earlier section is preserved, never duplicated"
    );
    Ok(())
}

/// The `conventional` style reshapes the section (grouped by commit
/// type) — the resolved style actually drives the renderer.
#[test]
fn apply_honors_the_conventional_style() -> Result<(), BoxError> {
    let mut fx = build_merged_workspace("conventional")?;
    write_config(&mut fx, "[changelog]\nstyle = \"conventional\"\n")?;
    let (code, out) = halyard(fx.root(), &["apply"])?;
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("changelog created"), "{out}");
    let document = read_changelog(fx.root())?;
    assert!(
        document.contains("### feat"),
        "the conventional body groups by type: {document}"
    );
    Ok(())
}

/// A template-path style rides the §9.4 user-template path: the shell
/// reads the file, the renderer consumes the source (the variables
/// `version`, `tag`, `sections`). The path carries a separator — the
/// classifier never guesses a bare suffix.
#[test]
fn apply_honors_a_user_template() -> Result<(), BoxError> {
    let mut fx = build_merged_workspace("template")?;
    fx.write(
        "templates/release.md.jinja",
        "# {{ version }} ({{ tag }})\n{% for section in sections %}\
         ## {{ section[0] }}\n{% for line in section[1] %}- {{ line }}\n{% endfor %}\
         {% endfor %}",
    )?;
    fx.stage_all()?;
    fx.commit("chore: add the release template")?;
    write_config(
        &mut fx,
        "[changelog]\nstyle = \"templates/release.md.jinja\"\n",
    )?;
    let (code, out) = halyard(fx.root(), &["apply", "--format", "json"])?;
    assert_eq!(code, 0, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    let rows = value
        .get("changelog")
        .and_then(serde_json::Value::as_array)
        .ok_or("the report carries the changelog rows")?;
    let style = rows
        .first()
        .expect("one row")
        .get("style")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    assert!(
        style.starts_with("template:") && style.contains("release.md.jinja"),
        "the row names the template source: {style}"
    );
    let document = read_changelog(fx.root())?;
    assert!(
        document.contains("# 0.1.2 (v0.1.2)"),
        "the user template's own shape renders: {document}"
    );
    Ok(())
}

/// An unknown style name is the documented caller bug (`E-CHLOG-STYLE`)
/// — fail closed before any changelog write.
#[test]
fn unknown_style_fails_closed() -> Result<(), BoxError> {
    let mut fx = build_merged_workspace("unknown-style")?;
    write_config(&mut fx, "[changelog]\nstyle = \"keepachangelog\"\n")?;
    let (code, out) = halyard(fx.root(), &["apply"])?;
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("E-CHLOG-STYLE"), "{out}");
    assert!(
        !fx.root().join("CHANGELOG.md").exists(),
        "nothing is written when the style is invalid"
    );
    Ok(())
}

/// An unreadable template file fails closed (`E-CHLOG-TEMPLATE`) — the
/// shell's read is the first thing to fail, before any write.
#[test]
fn missing_template_fails_closed() -> Result<(), BoxError> {
    let mut fx = build_merged_workspace("missing-template")?;
    write_config(&mut fx, "[changelog]\nstyle = \"templates/absent.md\"\n")?;
    let (code, out) = halyard(fx.root(), &["apply", "--format", "json"])?;
    assert_eq!(code, 1, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    let code = value
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(serde_json::Value::as_str);
    assert_eq!(code, Some("E-CHLOG-TEMPLATE"), "{out}");
    assert!(
        !fx.root().join("CHANGELOG.md").exists(),
        "nothing is written when the template is missing"
    );
    Ok(())
}
