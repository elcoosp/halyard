//! Demos the release-notes pipeline (§9.4 `github-release-notes`): the
//! two entry points the release flow uses — the plan-driven render (the
//! entry preview the planner produced) and the changelog-section render
//! (the forge release for a version that is already built, the §11.1 tag
//! phase's `create_release` payload) — with and without the forge
//! context (the compare link derived from a GitHub-shaped remote).
//!
//! Run: `cargo run -p halyard-changelog --example render_release_notes`

use std::error::Error;

use halyard_changelog::{
    NotesContext, compare_url, extract_version_section, render_release_notes,
    render_release_notes_for_version,
};
use halyard_core::Bump;
use halyard_core::plan::{PlanChangelog, PlanRelease};

fn main() -> Result<(), Box<dyn Error>> {
    // The plan-driven shape: the planner's entry preview re-grouped as
    // the forge-release body (breaking first, no version heading — the
    // forge release title carries the tag).
    let release = PlanRelease {
        package: "app".to_owned(),
        ecosystem: "cargo".to_owned(),
        from: "0.1.0".to_owned(),
        to: "0.2.0".to_owned(),
        bump: Bump::Minor,
        tag: "v0.2.0".to_owned(),
        reasons: Vec::new(),
        changelog: Some(PlanChangelog {
            file: "CHANGELOG.md".to_owned(),
            entry_md: "## 0.2.0\n\n### ⚠ BREAKING CHANGES\n\n- reshape the contract\n\n### Features\n\n- grow the surface\n".to_owned(),
        }),
        publish: None,
    };
    // The forge context: origin URL → previous tag → this tag. A
    // non-GitHub remote (or an initial release) derives no link.
    let compare = compare_url("git@github.com:example/app.git", Some("v0.1.0"), "v0.2.0");
    println!("compare = {compare:?}\n");
    let context = NotesContext {
        compare: compare.clone(),
    };
    println!("--- plan-driven notes (with forge context) ---");
    println!("{}", render_release_notes(&release, &context)?);

    // The tag-phase shape: the release that is already built renders
    // from the package changelog's own section for that version.
    let changelog =
        "# Changelog\n\n## [0.1.1]\n\n### Bug Fixes\n\n- stop the parser from eating spaces\n";
    println!("--- tag-phase notes from the changelog section ---");
    println!(
        "{}",
        render_release_notes_for_version(changelog, "0.1.1", &NotesContext { compare: None })
    );

    // No section, no fabrication: the honest empty body.
    let section = extract_version_section(changelog, "9.9.9");
    println!("section for 9.9.9 = {section:?} (notes would be empty)");
    Ok(())
}
