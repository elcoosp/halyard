//! The `pr --dry` forecast demo (M4.2b step 1): drives the §11.1
//! machine's three pass shapes against the dry-run [`PreviewForge`] —
//! the same passes `halyard pr --dry` forecasts before a real forge
//! adapter (M4.1b) is involved. The PR body comes from the §11.3
//! renderer (the real producer/consumer composition).
//!
//! Run: `cargo run -p halyard-forge --example pr_preview`

use halyard_changelog::render_release_body;
use halyard_core::plan::{Bump, Plan, PlanReason, PlanRelease};
use halyard_forge::{
    BranchName, Forge, MachineInput, PrSpec, PreviewForge, ReleaseSpec, run_release_machine,
};

/// Builds the §11.3 body for a one-release plan (the shell's producer
/// side).
fn body() -> String {
    let plan = Plan {
        schema: "halyard-plan/v1".to_owned(),
        base_branch: "main".to_owned(),
        head: "9f2ca67".to_owned(),
        plan_hash: String::new(),
        releases: vec![PlanRelease {
            package: "p1".to_owned(),
            ecosystem: "cargo".to_owned(),
            from: "0.1.0".to_owned(),
            to: "0.1.1".to_owned(),
            bump: Bump::Patch,
            tag: "p1-v0.1.1".to_owned(),
            reasons: vec![PlanReason::Commit {
                oid: "ab12cd34ef567890ab12cd34ef567890ab12cd34".to_owned(),
                kind: "fix".to_owned(),
                breaking: false,
            }],
            changelog: None,
            publish: None,
        }],
        edits: Vec::new(),
        order: vec!["p1".to_owned()],
        skipped: Vec::new(),
    };
    render_release_body(&plan).unwrap_or_else(|e| unreachable!("the plan body must render: {e}"))
}

fn main() {
    let forge = PreviewForge::new();
    let branch = BranchName::new("halyard/release/main")
        .unwrap_or_else(|e| unreachable!("branch fixture must be valid: {e}"));

    // Pass 1 — the prepare-phase open forecast (nothing pending, a plan
    // with releases: the §11.5 update path, find + upsert).
    let report = run_release_machine(
        &forge,
        &MachineInput {
            branch: branch.clone(),
            pending_releases: Vec::new(),
            release_pr: Some(PrSpec {
                branch: branch.clone(),
                title: "chore: release p1-v0.1.1".to_owned(),
                body: body(),
            }),
        },
    )
    .unwrap_or_else(|e| unreachable!("preview passes never fail: {e}"));
    println!("pass 1 (prepare, plan with releases): {report:?}");

    // Pass 2 — the empty-plan probe (find, nothing else: no PR to close).
    let report = run_release_machine(
        &forge,
        &MachineInput {
            branch: branch.clone(),
            pending_releases: Vec::new(),
            release_pr: None,
        },
    )
    .unwrap_or_else(|e| unreachable!("preview passes never fail: {e}"));
    println!("pass 2 (empty plan, close probe):     {report:?}");

    // Pass 3 — the tag-phase forecast (pending releases ⇒ PROPAGATE,
    // §11.1 step 2: releases are forecast, the PR is untouched).
    let report = run_release_machine(
        &forge,
        &MachineInput {
            branch: branch.clone(),
            pending_releases: vec![ReleaseSpec {
                tag: "p1-v0.1.1".to_owned(),
                notes: "## Features\n\n- fix the thing\n".to_owned(),
            }],
            release_pr: None,
        },
    )
    .unwrap_or_else(|e| unreachable!("preview passes never fail: {e}"));
    println!("pass 3 (tag phase, PROPAGATE):        {report:?}");

    println!("\ncall log (§11.5, oldest first):");
    for call in forge.calls() {
        println!("  {} {}", call.method, call.target);
    }
}
