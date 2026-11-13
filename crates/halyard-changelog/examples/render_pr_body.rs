//! The §11.3 Release-PR body demo (M4.3a): renders a plan into the
//! stable, parseable Release-PR body the §11.1 state machine carries in
//! its `PrSpec`, then reads the embedded plan back out (the I8 verifier
//! `release`/`publish` run against a merged Release PR).
//!
//! Run: `cargo run -p halyard-changelog --example render_pr_body`

use halyard_changelog::{extract_embedded_plan, render_release_body};
use halyard_core::canonical::plan_hash;
use halyard_core::plan::{Bump, Plan, PlanChangelog, PlanReason, PlanRelease, PlanSkip};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut plan = Plan {
        schema: "halyard-plan/v1".to_owned(),
        base_branch: "main".to_owned(),
        head: "9f2ca67".to_owned(),
        plan_hash: String::new(),
        releases: Vec::new(),
        edits: Vec::new(),
        order: vec!["core".to_owned(), "cli".to_owned()],
        skipped: vec![PlanSkip {
            package: "docs".to_owned(),
            reason: "no_changes".to_owned(),
        }],
    };
    plan.releases.push(PlanRelease {
        package: "core".to_owned(),
        ecosystem: "cargo".to_owned(),
        from: "0.4.1".to_owned(),
        to: "0.5.0".to_owned(),
        bump: Bump::Minor,
        tag: "core-v0.5.0".to_owned(),
        reasons: vec![PlanReason::Commit {
            oid: "ab12cd34ef567890ab12cd34ef567890ab12cd34".to_owned(),
            kind: "feat".to_owned(),
            breaking: false,
        }],
        changelog: Some(PlanChangelog {
            file: "CHANGELOG.md".to_owned(),
            entry_md: "## 0.5.0\n\n### ⚠ BREAKING CHANGES\n\n- reshape contract\n\n### Features\n\n- grow surface\n".to_owned(),
        }),
        publish: None,
    });
    plan.releases.push(PlanRelease {
        package: "cli".to_owned(),
        ecosystem: "cargo".to_owned(),
        from: "1.0.3".to_owned(),
        to: "1.0.4".to_owned(),
        bump: Bump::Patch,
        tag: "cli-v1.0.4".to_owned(),
        reasons: vec![PlanReason::Cascade {
            via: "core".to_owned(),
            edge: "normal".to_owned(),
            why: "requirement_unsatisfied".to_owned(),
        }],
        changelog: None,
        publish: None,
    });

    let body = render_release_body(&plan)?;
    println!("{body}");

    // The I8 inverse: read the plan back out of the rendered body and
    // verify it against a fresh recomputation.
    let extracted = extract_embedded_plan(&body)?;
    println!(
        "extracted plan: {} releases, hash verified: {}",
        extracted.releases.len(),
        extracted.plan_hash == plan_hash(&extracted)
    );
    Ok(())
}
