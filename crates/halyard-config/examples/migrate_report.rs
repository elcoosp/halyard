//! Foreign release-tool migration demo (M4.7 step 1, §16.2): the
//! release-please and changesets configs are surveyed into a migration
//! report — mapped rows, the semantic differences and a validated draft
//! `halyard.toml`. The report is data; this demo writes nothing (the
//! CLI `migrate` command renders the same shape).
//!
//! Run: `cargo run -p halyard-config --example migrate_report`

use std::error::Error;

use halyard_config::{ForeignFiles, ToolSurvey, default_sources, migrate};

/// Prints one survey's mapped rows, semantic differences, package rows
/// and draft (the human renderer's shape, minus the tool header).
fn print_survey(survey: &ToolSurvey) {
    for row in &survey.rows {
        println!(
            "mapped:     {} → {} = {}",
            row.foreign_key, row.halyard_key, row.value
        );
        if let Some(note) = &row.note {
            println!("            note: {note}");
        }
    }
    for row in &survey.unmapped {
        println!("unmapped:   {} — {}", row.foreign_key, row.reason);
    }
    for package in &survey.packages {
        println!(
            "package:    {} (component: {}, tag: {})",
            package.path,
            package.component.as_deref().unwrap_or("(path)"),
            package.tag
        );
        for note in &package.notes {
            println!("            {note}");
        }
    }
    println!("draft (never written by this demo):");
    println!(
        "{}",
        survey.draft.as_deref().unwrap_or("(nothing to draft)")
    );
}

fn main() -> Result<(), Box<dyn Error>> {
    // A repo migrating off release-please: the manifest config (the
    // VERIFIED grammar — see docs/research/migrate-config-grammars.md)
    // plus the version-tracking manifest.
    let mut files = ForeignFiles::new();
    files.insert(
        "release-please-config.json",
        r#"{
  "packages": {
    "crates/p1": { "package-name": "p1-crate", "changelog-path": "docs/CHANGELOG.md" },
    "crates/p2": {}
  },
  "bump-minor-pre-major": true,
  "plugins": ["cargo-workspace"]
}"#,
    );
    files.insert(
        ".release-please-manifest.json",
        r#"{ "crates/p1": "1.2.3", "crates/p2": "0.5.0" }"#,
    );

    let report = migrate(&files, &default_sources())?;
    let survey = report
        .tools
        .iter()
        .find(|t| t.tool == "release-please")
        .ok_or("release-please surveys every ForeignFiles input")?;
    println!(
        "tool:       {} (configured: {})",
        survey.tool, survey.configured
    );
    println!("files:      {}", survey.files.join(", "));
    print_survey(survey);

    // A repo migrating off release-plz: the Tera tag template
    // translates onto the appendix-A.3 grammar, the documented
    // changelog default pins the style, and a release-disabled package
    // becomes a halyard override. A repo migrating off knope rides the
    // same registry: its multi-package tag rule (`{package_name}/v{version}`)
    // maps onto `repo.tag_style` directly. (The VERIFIED grammars —
    // see docs/research/migrate-config-grammars.md.)
    let mut plz_files = ForeignFiles::new();
    plz_files.insert(
        "release-plz.toml",
        r#"
[workspace]
git_tag_name = "{{ package }}-v{{ version }}"
publish_timeout = "20m"

[[package]]
name = "engine"

[[package]]
name = "cli"
release = false
version_group = "bundle"
"#,
    );
    let report = migrate(&plz_files, &default_sources())?;
    let plz = report
        .tools
        .iter()
        .find(|t| t.tool == "release-plz")
        .ok_or("release-plz surveys every ForeignFiles input")?;
    println!("tool:       {} (configured: {})", plz.tool, plz.configured);
    print_survey(plz);

    // Absence is a value: no foreign files ⇒ every source reports
    // not-detected, no error, no rows.
    let report = migrate(&ForeignFiles::new(), &default_sources())?;
    for survey in &report.tools {
        println!(
            "absent:     {} configured={} (no error, no rows)",
            survey.tool, survey.configured
        );
    }
    Ok(())
}
