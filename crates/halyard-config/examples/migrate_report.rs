//! Foreign release-tool migration demo (M4.7 step 1, §16.2): the
//! release-please and changesets configs are surveyed into a migration
//! report — mapped rows, the semantic differences and a validated draft
//! `halyard.toml`. The report is data; this demo writes nothing (the
//! CLI `migrate` command renders the same shape).
//!
//! Run: `cargo run -p halyard-config --example migrate_report`

use std::error::Error;

use halyard_config::{ForeignFiles, default_sources, migrate};

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
    println!("draft (never written by this slice):");
    println!(
        "{}",
        survey.draft.as_deref().unwrap_or("(nothing to draft)")
    );

    // Absence is a value: no changesets files ⇒ configured: false.
    let report = migrate(&ForeignFiles::new(), &default_sources())?;
    for survey in &report.tools {
        println!(
            "absent:     {} configured={} (no error, no rows)",
            survey.tool, survey.configured
        );
    }
    Ok(())
}
