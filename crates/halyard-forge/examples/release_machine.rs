//! The §11.1 state machine consumer demo (M4.2a): drives a full
//! Release-PR lifecycle against the deterministic [`MemoryForge`] — the
//! same pass shapes `halyard release` / `halyard pr` will run against a
//! real forge adapter (M4.1b swaps the adapter, not the machine). The
//! PR body is rendered by the §11.3 renderer (`halyard-changelog`, the
//! real producer/consumer composition a release run performs).
//!
//! Run: `cargo run -p halyard-forge --example release_machine`

use halyard_changelog::render_release_body;
use halyard_core::plan::{Bump, Plan, PlanReason, PlanRelease};
use halyard_forge::{
    BranchName, Forge, MachineInput, MemoryForge, PrSpec, ReleaseSpec, run_release_machine,
};

/// Builds a small release plan and renders it into the §11.3 PR body —
/// the producer side of the machine's `release_pr` input. `with_cascade`
/// adds a dependent release so the "plan moved" pass renders a different
/// body.
fn plan_pr_spec_impl(
    branch: &BranchName,
    with_cascade: bool,
) -> Result<PrSpec, Box<dyn std::error::Error>> {
    let mut plan = Plan {
        schema: "halyard-plan/v1".to_owned(),
        base_branch: "main".to_owned(),
        head: "9f2ca67".to_owned(),
        plan_hash: String::new(),
        releases: vec![PlanRelease {
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
            changelog: None,
            publish: None,
        }],
        edits: Vec::new(),
        order: vec!["core".to_owned()],
        skipped: Vec::new(),
    };
    if with_cascade {
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
        plan.order.push("cli".to_owned());
    }
    Ok(PrSpec {
        branch: branch.clone(),
        title: "chore: release v0.5.0".to_owned(),
        body: render_release_body(&plan)?,
    })
}

/// The prepared-phase plan: one `core` release.
fn plan_pr_spec(branch: &BranchName) -> Result<PrSpec, Box<dyn std::error::Error>> {
    plan_pr_spec_impl(branch, false)
}

/// The moved plan: `core` plus a cascaded `cli` release.
fn plan_pr_spec_with_cascade(branch: &BranchName) -> Result<PrSpec, Box<dyn std::error::Error>> {
    plan_pr_spec_impl(branch, true)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let forge = MemoryForge::new();
    let branch = BranchName::new("halyard/release/main")?;

    // Pass 1 — tag phase (§11.1 step 2): pending releases gate the
    // prepare phase (PROPAGATE). The shell's git-side tag effects happen
    // around this pass; the machine owns the forge calls.
    let tag_input = MachineInput {
        branch: branch.clone(),
        pending_releases: vec![
            ReleaseSpec {
                tag: "core-v0.5.0".to_owned(),
                notes: "## Features\n\n- the painter walk".to_owned(),
            },
            ReleaseSpec {
                tag: "cli-v0.3.0".to_owned(),
                notes: "## Bug Fixes\n\n- apply exit codes".to_owned(),
            },
        ],
        release_pr: Some(PrSpec {
            branch: branch.clone(),
            title: "chore: release v0.5.0".to_owned(),
            body: "## 🚀 Release plan".to_owned(),
        }),
    };
    let report = run_release_machine(&forge, &tag_input)?;
    println!("pass 1 (tag phase): propagated = {}", report.propagated);
    for effect in &report.effects {
        println!("  {effect:?}");
    }
    println!(
        "  Release PR opened? {} (calls: {})",
        forge.find_release_pr(&branch)?.is_some(),
        report.calls
    );

    // Pass 2 — prepare phase (§11.1 step 3): nothing pending, the plan
    // has releases → the Release PR is opened (find + upsert = 2 calls,
    // the §11.5 update-path budget). The body comes from the §11.3
    // renderer — plan in, deterministic markdown out.
    let pr_input = MachineInput {
        branch: branch.clone(),
        pending_releases: Vec::new(),
        release_pr: Some(plan_pr_spec(&branch)?),
    };
    let report = run_release_machine(&forge, &pr_input)?;
    println!("pass 2 (prepare): {:?}", report.effects);
    println!("  calls: {} (budget ≤ 2)", report.calls);

    // Pass 3 — the identical body is a no-op (I1): find only, the PR is
    // not rewritten. The renderer's determinism (I3) is what makes this
    // pass a no-op: the same plan renders byte-for-byte the same body.
    let report = run_release_machine(&forge, &pr_input)?;
    println!("pass 3 (no-op):    {:?}", report.effects);

    // Pass 4 — the plan moved (a cascade joined): the body differs →
    // rewritten in place, same PR number (I1).
    let moved = MachineInput {
        branch: branch.clone(),
        pending_releases: Vec::new(),
        release_pr: Some(plan_pr_spec_with_cascade(&branch)?),
    };
    let report = run_release_machine(&forge, &moved)?;
    println!("pass 4 (update):   {:?}", report.effects);

    // Pass 5 — the Release PR merged upstream: the plan is empty and the
    // stale PR is closed (§11.1 step 3). The re-run (pass 6) finds
    // nothing and double-closes nothing (I1).
    let empty = MachineInput {
        branch: branch.clone(),
        pending_releases: Vec::new(),
        release_pr: None,
    };
    let report = run_release_machine(&forge, &empty)?;
    println!("pass 5 (close):    {:?}", report.effects);
    let report = run_release_machine(&forge, &empty)?;
    println!(
        "pass 6 (re-close): effects = {:?} (calls: {})",
        report.effects, report.calls
    );

    // Observability (§11.5): the machine's passes ride the adapter's
    // call log — this is the end-of-run usage summary's source.
    println!("call log:");
    for record in forge.calls() {
        println!("  {:>16}  {}", record.method, record.target);
    }
    Ok(())
}
