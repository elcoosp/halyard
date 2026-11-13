//! Renders keep-a-changelog output for a small in-memory plan — once for
//! a fresh document and once for an existing document with an
//! `## [Unreleased]` section (both printed to stdout).
//!
//! Run with `cargo run -p halyard-changelog --example render`.

use halyard_changelog::render_plan;
use halyard_core::plan::{Plan, PlanChangelog, PlanRelease};
use halyard_core::{Bump, PLAN_SCHEMA_V1};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let plan = sample_plan();

    let fresh = render_plan(&plan, "keep-a-changelog", None)?;
    println!("=== fresh document ===\n{fresh}");

    let existing = "# Changelog\n\n## [Unreleased]\n\n- old note\n\n## [0.1.0] - 2020-01-01\n\n### Added\n\n- initial\n";
    let merged = render_plan(&plan, "keep-a-changelog", Some(existing))?;
    println!("=== merged into existing document ===\n{merged}");

    Ok(())
}

/// The same plan shape as the crate's `tests::sample_plan`.
fn sample_plan() -> Plan {
    Plan {
        schema: PLAN_SCHEMA_V1.to_owned(),
        base_branch: "main".to_owned(),
        head: "a".repeat(40),
        plan_hash: "blake3:aa".to_owned(),
        releases: vec![PlanRelease {
            package: "app".to_owned(),
            ecosystem: "cargo".to_owned(),
            from: "0.1.0".to_owned(),
            to: "0.2.0".to_owned(),
            bump: Bump::Minor,
            tag: "v0.2.0".to_owned(),
            reasons: Vec::new(),
            changelog: Some(PlanChangelog {
                file: "CHANGELOG.md".to_owned(),
                entry_md: "## 0.2.0\n\n### ⚠ BREAKING CHANGES\n\n- reshape contract\n\n### \
                           Features\n\n- grow surface\n"
                    .to_owned(),
            }),
            publish: None,
        }],
        edits: Vec::new(),
        order: vec!["app".to_owned()],
        skipped: Vec::new(),
    }
}
