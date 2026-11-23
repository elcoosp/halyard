//! CLI change-file wiring (§9.4, ADR-0028): the shell scans
//! `.halyard/changes/*.md` under the repo root, parses them through the
//! changelog grammar and feeds the pure planner — an explicit bump
//! replaces the commit-derived level and the note merges into the
//! changelog preview. A malformed file refuses the whole plan
//! (`E-CHLOG-CHANGE-FILE`): one bad input never silently drops.

use std::process::Command;

use halyard_test_support::{CommitSpec, GitFixture};

type BoxError = Box<dyn std::error::Error>;

fn fixture_dir(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir = std::env::temp_dir().join(format!(
        "halyard-cli-changes-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// One crate at `1.2.3`, tagged `v1.2.3` — the zero-config shape the
/// change files extend.
fn build_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let dir = fixture_dir(label)?;
    let mut fx = GitFixture::init_in(&dir)?;
    fx.commit_with(&CommitSpec::files(
        vec![
            (
                "Cargo.toml".to_owned(),
                "[package]\nname = \"solo\"\nversion = \"1.2.3\"\nedition = \"2021\"\n".to_owned(),
            ),
            (
                "src/lib.rs".to_owned(),
                "pub const C: usize = 1;\n".to_owned(),
            ),
        ],
        "chore: scaffold the crate",
    ))?;
    fx.tag_annotated("v1.2.3", "solo v1.2.3")?;
    Ok(fx)
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

/// The change file's explicit minor outranks the `fix` commit's patch
/// and the reason chain names the file; the note rides the changelog
/// preview.
#[test]
fn the_change_file_drives_the_plan() -> Result<(), BoxError> {
    let mut fx = build_workspace("drives")?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "src/fix.rs".to_owned(),
            "pub const PATCHED: usize = 2;\n".to_owned(),
        )],
        "fix: mend the edge",
    ))?;
    fx.write(
        ".halyard/changes/next.md",
        "---\npackages = { solo = \"minor\" }\n---\n\nUpgrade notes:\n\n- flip the flag\n",
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
        Some("1.3.0"),
        "the explicit minor outranks the fix commit's patch: {out}"
    );
    let reasons = release
        .get("reasons")
        .and_then(serde_json::Value::as_array)
        .ok_or("the release carries reasons")?;
    assert!(
        reasons
            .iter()
            .any(|r| r.get("type").and_then(serde_json::Value::as_str) == Some("changeset")),
        "the wire format names the change file: {reasons:?}"
    );
    assert!(
        out.contains("flip the flag"),
        "the note merges into the changelog preview: {out}"
    );
    Ok(())
}

/// A malformed change file refuses the plan (`E-CHLOG-CHANGE-FILE`) —
/// the grammar is strict, nothing is silently skipped.
#[test]
fn a_malformed_change_file_refuses_the_plan() -> Result<(), BoxError> {
    let mut fx = build_workspace("refuse")?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "src/fix.rs".to_owned(),
            "pub const PATCHED: usize = 2;\n".to_owned(),
        )],
        "fix: mend the edge",
    ))?;
    fx.write(
        ".halyard/changes/broken.md",
        "just a note, no front matter\n",
    )?;
    let (code, out) = halyard(fx.root(), &["plan", "--format", "json"])?;
    assert_ne!(code, 0, "a malformed file must refuse: {out}");
    assert!(out.contains("E-CHLOG-CHANGE-FILE"), "{out}");
    Ok(())
}

/// `explain` renders the change-file reason through the existing
/// `changeset` reason line — the §5.6 chain is observable end-to-end.
#[test]
fn explain_names_the_change_file() -> Result<(), BoxError> {
    let fx = build_workspace("explain")?;
    fx.write(
        ".halyard/changes/next.md",
        "---\npackages = { solo = \"minor\" }\n---\n",
    )?;
    let (code, out) = halyard(fx.root(), &["explain", "solo"])?;
    assert_eq!(code, 0, "{out}");
    let expected = format!(
        "changeset .halyard{}changes{}next.md",
        std::path::MAIN_SEPARATOR_STR,
        std::path::MAIN_SEPARATOR_STR
    );
    assert!(
        out.contains(&expected),
        "the reason chain names the file: {out}"
    );
    assert!(
        out.contains("1.3.0"),
        "the explicit minor is the target: {out}"
    );
    Ok(())
}
