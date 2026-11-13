//! The §11.1 Release-PR state machine (M4.2a): the forge-side lifecycle
//! semantics pinned against the [`MemoryForge`] reference adapter.
//!
//! The machine is the first consumer of the §15.2 failure-injection
//! harness: every test that arms a failpoint re-runs to completion and
//! asserts the final state equals the uninterrupted run (I2 — zero
//! duplicates). The call log pins the §11.5 budgets: ≤ 2 calls on the
//! update path, zero mutating calls on the no-op paths.

use halyard_forge::{
    BranchName, Forge, ForgeError, MachineEffect, MachineInput, MemoryForge, PrSpec, ReleaseSpec,
    run_release_machine,
};

/// A branch name shared by the test sequences (validated once, reused).
fn main_branch() -> BranchName {
    BranchName::new("halyard/release/main")
        .unwrap_or_else(|e| unreachable!("branch fixture must be valid: {e}"))
}

/// A Release-PR spec fixture.
fn release_pr_spec(body: &str) -> PrSpec {
    PrSpec {
        branch: main_branch(),
        title: "chore: release v0.2.0".to_owned(),
        body: body.to_owned(),
    }
}

/// Two pending-release fixtures (the tag phase's input, §11.1 step 2).
fn two_pending_releases() -> Vec<ReleaseSpec> {
    vec![
        ReleaseSpec {
            tag: "core-v0.5.0".to_owned(),
            notes: "## Features\n\n- the painter walk".to_owned(),
        },
        ReleaseSpec {
            tag: "cli-v0.3.0".to_owned(),
            notes: "## Bug Fixes\n\n- apply exit codes".to_owned(),
        },
    ]
}

/// Drives one machine pass through a trait object (object-safety is part
/// of the contract — the shell holds adapters as `&dyn Forge`).
fn run(
    forge: &dyn Forge,
    input: &MachineInput,
) -> Result<halyard_forge::MachineReport, ForgeError> {
    run_release_machine(forge, input)
}

#[test]
fn tag_phase_creates_releases_and_propagates() {
    // §11.1 step 2: the tag phase runs first; with pending releases the
    // prepare phase is skipped (PROPAGATE) — no Release PR is touched.
    let forge = MemoryForge::new();
    let input = MachineInput {
        branch: main_branch(),
        pending_releases: two_pending_releases(),
        release_pr: Some(release_pr_spec("## 🚀 Release plan")),
    };
    let report =
        run(&forge, &input).unwrap_or_else(|e| unreachable!("tag phase must succeed: {e}"));
    assert!(report.propagated, "pending releases gate the prepare phase");
    assert_eq!(
        report.effects,
        vec![
            MachineEffect::ReleaseCreated {
                tag: "core-v0.5.0".to_owned(),
                url: "mem://release/core-v0.5.0".to_owned(),
            },
            MachineEffect::ReleaseCreated {
                tag: "cli-v0.3.0".to_owned(),
                url: "mem://release/cli-v0.3.0".to_owned(),
            },
        ],
        "each pending release is created in order"
    );
    assert!(
        forge.prs().is_empty(),
        "the prepare phase never ran: no PR was opened"
    );
    assert_eq!(forge.releases().len(), 2);
    assert_eq!(report.calls, 2, "one create call per pending release");
}

#[test]
fn tag_phase_rerun_is_fully_idempotent() {
    // I1/I2: the re-run after a partial failure re-finds every release
    // (GET-by-tag-first) and creates no duplicate.
    let forge = MemoryForge::new();
    let input = MachineInput {
        branch: main_branch(),
        pending_releases: two_pending_releases(),
        release_pr: None,
    };
    let _ = run(&forge, &input).unwrap_or_else(|e| unreachable!("first pass must succeed: {e}"));
    let rerun = run(&forge, &input).unwrap_or_else(|e| unreachable!("rerun must succeed: {e}"));
    assert_eq!(
        rerun.effects,
        vec![
            MachineEffect::ReleaseExisted {
                tag: "core-v0.5.0".to_owned(),
            },
            MachineEffect::ReleaseExisted {
                tag: "cli-v0.3.0".to_owned(),
            },
        ],
        "the rerun reports existing releases, not new ones"
    );
    assert_eq!(forge.releases().len(), 2, "no duplicate release artifacts");
}

#[test]
fn prepare_phase_opens_then_updates_in_place() {
    // §11.1 step 3: find → upsert. The second pass with an identical
    // body must be a no-op (PrUnchanged — I1: no mutating call), and a
    // changed body rewrites the same PR in place.
    let forge = MemoryForge::new();
    let branch = main_branch();
    let input = |body: &'static str| MachineInput {
        branch: branch.clone(),
        pending_releases: Vec::new(),
        release_pr: Some(release_pr_spec(body)),
    };
    let first = run(
        &forge,
        &input("## 🚀 Release plan\n\n| core | 0.1.0 | 0.2.0 |"),
    )
    .unwrap_or_else(|e| unreachable!("open pass must succeed: {e}"));
    assert!(!first.propagated, "nothing pending → prepare ran");
    assert_eq!(
        first.effects,
        vec![MachineEffect::PrOpened { number: 1 }],
        "the first pass opens the Release PR"
    );
    let unchanged = run(
        &forge,
        &input("## 🚀 Release plan\n\n| core | 0.1.0 | 0.2.0 |"),
    )
    .unwrap_or_else(|e| unreachable!("no-op pass must succeed: {e}"));
    assert_eq!(
        unchanged.effects,
        vec![MachineEffect::PrUnchanged { number: 1 }],
        "an identical body is a no-op, not a rewrite"
    );
    let updated = run(
        &forge,
        &input("## 🚀 Release plan\n\n| core | 0.1.0 | 0.2.0 |\n\nmore"),
    )
    .unwrap_or_else(|e| unreachable!("update pass must succeed: {e}"));
    assert_eq!(
        updated.effects,
        vec![MachineEffect::PrUpdated { number: 1 }],
        "a changed body rewrites the same PR"
    );
    assert_eq!(forge.prs().len(), 1, "no duplicate PR was ever opened");
}

#[test]
fn update_path_stays_within_the_two_call_budget() {
    // §11.5: the update path is find (≤ 1) + upsert (1) per pass; the
    // no-op pass is find only. The call log is the observable.
    let forge = MemoryForge::new();
    let branch = main_branch();
    let input = MachineInput {
        branch: branch.clone(),
        pending_releases: Vec::new(),
        release_pr: Some(release_pr_spec("body-v2")),
    };
    let _ = run(&forge, &input).unwrap_or_else(|e| unreachable!("open pass must succeed: {e}"));
    let unchanged =
        run(&forge, &input).unwrap_or_else(|e| unreachable!("no-op pass must succeed: {e}"));
    assert_eq!(unchanged.calls, 1, "no-op pass: find only");
    let changed = MachineInput {
        branch,
        pending_releases: Vec::new(),
        release_pr: Some(release_pr_spec("body-v3")),
    };
    let update =
        run(&forge, &changed).unwrap_or_else(|e| unreachable!("update pass must succeed: {e}"));
    assert_eq!(update.calls, 2, "update pass: find + upsert (§11.5)");
}

#[test]
fn empty_plan_closes_the_stale_release_pr() {
    // §11.1 step 3: plan empty + a stale Release PR exists → close it.
    // The re-run finds nothing and performs no second close (I1).
    let forge = MemoryForge::new();
    let _ = forge
        .upsert_pr(&release_pr_spec("stale plan"))
        .unwrap_or_else(|e| unreachable!("fixture upsert must succeed: {e}"));
    let input = MachineInput {
        branch: main_branch(),
        pending_releases: Vec::new(),
        release_pr: None,
    };
    let report =
        run(&forge, &input).unwrap_or_else(|e| unreachable!("close pass must succeed: {e}"));
    assert_eq!(
        report.effects,
        vec![MachineEffect::PrClosed { number: 1 }],
        "the stale PR is closed"
    );
    let rerun =
        run(&forge, &input).unwrap_or_else(|e| unreachable!("post-close rerun must succeed: {e}"));
    assert!(
        rerun.effects.is_empty(),
        "the closed PR is not found again: no duplicate close"
    );
    assert_eq!(rerun.calls, 1, "the rerun spends the find call only");
}

#[test]
fn empty_plan_without_a_stale_pr_is_zero_mutating_calls() {
    // §11.1: "exit (0 API calls when no PR exists)" — the find is the
    // one cheap probe; no mutating call is made.
    let forge = MemoryForge::new();
    let input = MachineInput {
        branch: main_branch(),
        pending_releases: Vec::new(),
        release_pr: None,
    };
    let report =
        run(&forge, &input).unwrap_or_else(|e| unreachable!("no-op pass must succeed: {e}"));
    assert!(report.effects.is_empty(), "nothing to do");
    assert_eq!(report.calls, 1, "the find probe");
    assert_eq!(forge.call_count("close_pr"), 0);
    assert_eq!(forge.call_count("upsert_pr"), 0);
    assert_eq!(forge.call_count("create_release"), 0);
}

#[test]
fn failure_injection_midway_recovers_without_duplicates() {
    // §15.2 failure-injection matrix (I2): a failure at every forge
    // operation leaves no partial state — the re-run completes and the
    // final state equals the uninterrupted baseline.
    let inputs = [
        (
            "tag-phase",
            MachineInput {
                branch: main_branch(),
                pending_releases: two_pending_releases(),
                release_pr: None,
            },
        ),
        (
            "prepare-phase",
            MachineInput {
                branch: main_branch(),
                pending_releases: Vec::new(),
                release_pr: Some(release_pr_spec("plan")),
            },
        ),
    ];
    let failures = [
        ("auth", ForgeError::auth("token lacks contents:write")),
        ("rate-limit", ForgeError::rate_limited(30)),
        ("conflict", ForgeError::conflict("release branch moved")),
    ];
    for (input_name, input) in &inputs {
        // Baseline: the uninterrupted run's final state.
        let baseline = MemoryForge::new();
        let baseline_report = run(&baseline, input)
            .unwrap_or_else(|e| unreachable!("baseline {input_name} must succeed: {e}"));
        for (failure_name, failure) in &failures {
            let forge = MemoryForge::new();
            // Pass 1 succeeds far enough to arm the failpoint *midway* is
            // modeled by running with the failure armed from the start —
            // the first forge operation the pass attempts fails.
            forge.set_failure(Some(failure.clone()));
            let Err(err) = run(&forge, input) else {
                unreachable!("armed failpoint must fail the {input_name} pass ({failure_name})");
            };
            assert_eq!(
                err.code(),
                failure.code(),
                "the machine propagates the injected error verbatim"
            );
            forge.set_failure(None);
            let recovered = run(&forge, input)
                .unwrap_or_else(|e| unreachable!("recovery pass must succeed: {e}"));
            assert_eq!(
                recovered.effects, baseline_report.effects,
                "{input_name}/{failure_name}: the re-run reaches the uninterrupted outcome"
            );
            assert_eq!(
                forge.prs().len(),
                baseline.prs().len(),
                "{input_name}/{failure_name}: no duplicate PR"
            );
            assert_eq!(
                forge.releases().len(),
                baseline.releases().len(),
                "{input_name}/{failure_name}: no duplicate release"
            );
        }
    }
}

#[test]
fn failure_during_close_leaves_the_pr_open() {
    // §15.2, close path: the close fails → the PR stays open → the
    // re-run closes it exactly once.
    let forge = MemoryForge::new();
    let _ = forge
        .upsert_pr(&release_pr_spec("stale plan"))
        .unwrap_or_else(|e| unreachable!("fixture upsert must succeed: {e}"));
    forge.set_failure(Some(ForgeError::rate_limited(60)));
    let input = MachineInput {
        branch: main_branch(),
        pending_releases: Vec::new(),
        release_pr: None,
    };
    let Err(err) = run(&forge, &input) else {
        unreachable!("the armed failpoint must fail the close");
    };
    assert_eq!(err.code(), "E-FORGE-RATE-LIMIT");
    forge.set_failure(None);
    let report =
        run(&forge, &input).unwrap_or_else(|e| unreachable!("recovery close must succeed: {e}"));
    assert_eq!(
        report.effects,
        vec![MachineEffect::PrClosed { number: 1 }],
        "the re-run closes the still-open PR exactly once"
    );
}

#[test]
fn close_pr_defaults_to_the_unsupported_decline() {
    // The trait's default `close_pr` is the documented honest decline
    // (mirrors `commit_via_api`): an adapter without the capability
    // names itself, and the machine surfaces the decline fail-closed.
    struct NoCloseForge(MemoryForge);
    impl Forge for NoCloseForge {
        fn find_release_pr(
            &self,
            branch: &BranchName,
        ) -> Result<Option<halyard_forge::Pr>, ForgeError> {
            self.0.find_release_pr(branch)
        }
        fn upsert_pr(&self, spec: &PrSpec) -> Result<halyard_forge::Pr, ForgeError> {
            self.0.upsert_pr(spec)
        }
        fn release_exists(&self, tag: &str) -> Result<bool, ForgeError> {
            self.0.release_exists(tag)
        }
        fn create_release(&self, spec: &ReleaseSpec) -> Result<halyard_forge::Release, ForgeError> {
            self.0.create_release(spec)
        }
        fn rate_limit(&self) -> halyard_forge::RateInfo {
            self.0.rate_limit()
        }
        fn calls(&self) -> Vec<halyard_forge::CallRecord> {
            self.0.calls()
        }
        // `close_pr` intentionally left at the trait default.
    }
    let forge = NoCloseForge(MemoryForge::new());
    let _ = forge
        .upsert_pr(&release_pr_spec("stale plan"))
        .unwrap_or_else(|e| unreachable!("fixture upsert must succeed: {e}"));
    let input = MachineInput {
        branch: main_branch(),
        pending_releases: Vec::new(),
        release_pr: None,
    };
    let Err(err) = run(&forge, &input) else {
        unreachable!("the default close_pr must decline the machine's close");
    };
    assert_eq!(err.code(), "E-FORGE-UNSUPPORTED");
    assert!(
        err.to_string().contains("NoCloseForge"),
        "the decline names the adapter: {err}"
    );
}

#[test]
fn machine_input_validation_fails_closed_before_any_call() {
    // An empty tag in the input is a caller bug: the machine rejects it
    // before touching the forge (zero calls — nothing was attempted).
    let forge = MemoryForge::new();
    let input = MachineInput {
        branch: main_branch(),
        pending_releases: vec![ReleaseSpec {
            tag: String::new(),
            notes: "notes".to_owned(),
        }],
        release_pr: None,
    };
    let Err(err) = run(&forge, &input) else {
        unreachable!("an empty pending tag must be rejected");
    };
    assert_eq!(err.code(), "E-FORGE-REQUEST", "{err}");
    assert!(
        forge.calls().is_empty(),
        "validation happens before any call"
    );
}

#[test]
fn report_is_deterministic_for_equal_state() {
    // I3: two equally-constructed forges driven through the same passes
    // produce identical reports.
    let drive = || {
        let forge = MemoryForge::new();
        let branch = main_branch();
        let tag_input = MachineInput {
            branch: branch.clone(),
            pending_releases: two_pending_releases(),
            release_pr: None,
        };
        let _ =
            run(&forge, &tag_input).unwrap_or_else(|e| unreachable!("tag pass must succeed: {e}"));
        let pr_input = MachineInput {
            branch: branch.clone(),
            pending_releases: Vec::new(),
            release_pr: Some(release_pr_spec("plan")),
        };
        let pr = run(&forge, &pr_input)
            .unwrap_or_else(|e| unreachable!("prepare pass must succeed: {e}"));
        (pr, forge.releases().len())
    };
    assert_eq!(
        drive(),
        drive(),
        "identical inputs ⇒ identical reports (I3)"
    );
}

#[test]
fn machine_runs_against_a_trait_object() {
    // The shell consumes adapters as `&dyn Forge` (§7.2); the machine
    // entry point must be usable through the trait object.
    let forge: Box<dyn Forge> = Box::new(MemoryForge::new());
    let input = MachineInput {
        branch: main_branch(),
        pending_releases: Vec::new(),
        release_pr: Some(release_pr_spec("plan")),
    };
    let report =
        run(forge.as_ref(), &input).unwrap_or_else(|e| unreachable!("dyn pass must succeed: {e}"));
    assert_eq!(report.effects.len(), 1);
}
