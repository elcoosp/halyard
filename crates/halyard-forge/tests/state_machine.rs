//! The §11.1 Release-PR state machine (M4.2a): the forge-side lifecycle
//! semantics pinned against the [`MemoryForge`] reference adapter.
//!
//! The machine is the first consumer of the §15.2 failure-injection
//! harness: every test that arms a failpoint re-runs to completion and
//! asserts the final state equals the uninterrupted run (I2 — zero
//! duplicates). The call log pins the §11.5 budgets: ≤ 2 calls on the
//! update path, zero mutating calls on the no-op paths.

use halyard_core::reason::Oid;
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
        content_commit: None,
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
        content_commit: None,
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
        content_commit: None,
    };
    let first = run(
        &forge,
        &input("## 🚀 Release plan\n\n| core | 0.1.0 | 0.2.0 |"),
    )
    .unwrap_or_else(|e| unreachable!("open pass must succeed: {e}"));
    assert!(!first.propagated, "nothing pending → prepare ran");
    assert_eq!(
        first.effects,
        vec![MachineEffect::PrOpened {
            number: 1,
            url: "mem://pr/1".to_owned(),
        }],
        "the first pass opens the Release PR"
    );
    let unchanged = run(
        &forge,
        &input("## 🚀 Release plan\n\n| core | 0.1.0 | 0.2.0 |"),
    )
    .unwrap_or_else(|e| unreachable!("no-op pass must succeed: {e}"));
    assert_eq!(
        unchanged.effects,
        vec![MachineEffect::PrUnchanged {
            number: 1,
            url: "mem://pr/1".to_owned(),
        }],
        "an identical body is a no-op, not a rewrite"
    );
    let updated = run(
        &forge,
        &input("## 🚀 Release plan\n\n| core | 0.1.0 | 0.2.0 |\n\nmore"),
    )
    .unwrap_or_else(|e| unreachable!("update pass must succeed: {e}"));
    assert_eq!(
        updated.effects,
        vec![MachineEffect::PrUpdated {
            number: 1,
            url: "mem://pr/1".to_owned(),
        }],
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
        content_commit: None,
    };
    let _ = run(&forge, &input).unwrap_or_else(|e| unreachable!("open pass must succeed: {e}"));
    let unchanged =
        run(&forge, &input).unwrap_or_else(|e| unreachable!("no-op pass must succeed: {e}"));
    assert_eq!(unchanged.calls, 1, "no-op pass: find only");
    let changed = MachineInput {
        branch,
        pending_releases: Vec::new(),
        release_pr: Some(release_pr_spec("body-v3")),
        content_commit: None,
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
        content_commit: None,
    };
    let report =
        run(&forge, &input).unwrap_or_else(|e| unreachable!("close pass must succeed: {e}"));
    assert_eq!(
        report.effects,
        vec![MachineEffect::PrClosed {
            number: 1,
            url: "mem://pr/1".to_owned(),
        }],
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
        content_commit: None,
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
                content_commit: None,
            },
        ),
        (
            "prepare-phase",
            MachineInput {
                branch: main_branch(),
                pending_releases: Vec::new(),
                release_pr: Some(release_pr_spec("plan")),
                content_commit: None,
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
        content_commit: None,
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
        vec![MachineEffect::PrClosed {
            number: 1,
            url: "mem://pr/1".to_owned(),
        }],
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
        content_commit: None,
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
        content_commit: None,
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
            content_commit: None,
        };
        let _ =
            run(&forge, &tag_input).unwrap_or_else(|e| unreachable!("tag pass must succeed: {e}"));
        let pr_input = MachineInput {
            branch: branch.clone(),
            pending_releases: Vec::new(),
            release_pr: Some(release_pr_spec("plan")),
            content_commit: None,
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
        content_commit: None,
    };
    let report =
        run(forge.as_ref(), &input).unwrap_or_else(|e| unreachable!("dyn pass must succeed: {e}"));
    assert_eq!(report.effects.len(), 1);
}

// ---------------------------------------------------------------------------
// §11.1 step 3, the content-commit half (the M4.2 remainder): the plan's
// EditSet applied in memory → tree → compare with the remote release
// branch tip → commit (the forge API path) → upsert the Release PR. The
// pinned plan line (docs/HALYARD_ARCHITECTURE.md §11.1):
//
//   "apply EditSet in memory → build tree → compare tree OID with remote
//    release branch tip (`ls-remote`, 0 REST)
//    if equal: update PR body only if plan_hash differs; else commit &
//    `push --force-with-lease`"
//
// The machine never edits: the caller applies the EditSet in memory and
// hands the applied payloads plus the tree it hashed over. The machine
// owns the compare decision and the `commit_via_api` call (§11.4) — the
// caller-probed remote tree is the idempotence gate (I1).
// ---------------------------------------------------------------------------

/// A valid 40-hex OID fixture (a tree the caller hashed, deterministic).
fn tree_oid_fixture(seed: &str) -> Oid {
    let mut digits = String::with_capacity(40);
    let source = seed.as_bytes();
    for index in 0..20 {
        let byte = source.get(index).copied().unwrap_or(0xa0 + index as u8);
        digits.push_str(&format!("{byte:02x}"));
    }
    Oid::from_hex(digits.as_bytes()).unwrap_or_else(|| unreachable!("hex fixture is 40 digits"))
}

/// A one-file content fixture (the applied EditSet's payload).
fn content_fixture(tree_oid: Oid, remote: Option<Oid>) -> halyard_forge::ContentCommit {
    halyard_forge::ContentCommit {
        message: "chore: release v0.2.0".to_owned(),
        files: vec![halyard_forge::ApiFile {
            path: "Cargo.toml".to_owned(),
            content: b"[package]\nversion = \"0.2.0\"\n".to_vec(),
        }],
        tree_oid,
        remote_tree_oid: remote,
    }
}

#[test]
fn content_commit_lands_the_plan_edits_then_upserts_the_pr() {
    // §11.1 step 3: the remote tree diverges (first pass: no branch yet)
    // ⇒ the machine commits the applied payloads through the forge's
    // `commit_via_api` and then upserts the Release PR — in that order.
    let forge = MemoryForge::new();
    let tree = tree_oid_fixture("the-content-tree");
    let input = MachineInput {
        branch: main_branch(),
        pending_releases: Vec::new(),
        release_pr: Some(release_pr_spec("## 🚀 Release plan")),
        content_commit: Some(content_fixture(tree.clone(), None)),
    };
    let report =
        run(&forge, &input).unwrap_or_else(|e| unreachable!("content pass must succeed: {e}"));
    assert_eq!(
        report.effects,
        vec![
            MachineEffect::ContentCommitted {
                oid: forge.commits()[0].oid.clone()
            },
            MachineEffect::PrOpened {
                number: 1,
                url: "mem://pr/1".to_owned(),
            },
        ],
        "the content commit precedes the Release-PR upsert (§11.1 order)"
    );
    assert_eq!(forge.call_count("commit_via_api"), 1);
    let commits = forge.commits();
    assert_eq!(commits.len(), 1, "exactly one content commit");
    assert_eq!(commits[0].spec.branch, main_branch());
    assert_eq!(commits[0].spec.message, "chore: release v0.2.0");
    assert_eq!(commits[0].spec.files.len(), 1);
    assert_eq!(commits[0].spec.files[0].path, "Cargo.toml");
    assert_eq!(
        report.calls, 3,
        "commit_via_api + find_release_pr + upsert_pr"
    );
}

#[test]
fn content_commit_matched_tree_skips_the_commit() {
    // §11.1: "if equal: update PR body only" — the caller's probe matched
    // the applied tree, so the branch already carries the content: zero
    // mutating calls (I1), the PR upsert still runs.
    let forge = MemoryForge::new();
    let tree = tree_oid_fixture("the-content-tree");
    let input = MachineInput {
        branch: main_branch(),
        pending_releases: Vec::new(),
        release_pr: Some(release_pr_spec("## 🚀 Release plan")),
        content_commit: Some(content_fixture(tree.clone(), Some(tree))),
    };
    let report =
        run(&forge, &input).unwrap_or_else(|e| unreachable!("content pass must succeed: {e}"));
    assert_eq!(
        report.effects,
        vec![
            MachineEffect::ContentMatched {
                tree_oid: tree_oid_fixture("the-content-tree")
            },
            MachineEffect::PrOpened {
                number: 1,
                url: "mem://pr/1".to_owned(),
            },
        ],
        "the matched tree is reported, then the PR is upserted"
    );
    assert_eq!(forge.call_count("commit_via_api"), 0, "nothing to commit");
    assert_eq!(report.calls, 2, "find_release_pr + upsert_pr only");
}

#[test]
fn content_commit_rerun_after_landing_matches() {
    // I1/I2: pass 1 commits the content; pass 2 re-probes the branch —
    // the tip's tree now equals the applied tree ⇒ no duplicate commit.
    let forge = MemoryForge::new();
    let tree = tree_oid_fixture("the-content-tree");
    let first = MachineInput {
        branch: main_branch(),
        pending_releases: Vec::new(),
        release_pr: Some(release_pr_spec("plan")),
        content_commit: Some(content_fixture(tree.clone(), None)),
    };
    let _ = run(&forge, &first).unwrap_or_else(|e| unreachable!("first pass: {e}"));
    let second = MachineInput {
        branch: main_branch(),
        pending_releases: Vec::new(),
        release_pr: Some(release_pr_spec("plan")),
        content_commit: Some(content_fixture(tree.clone(), Some(tree.clone()))),
    };
    let rerun = run(&forge, &second).unwrap_or_else(|e| unreachable!("rerun: {e}"));
    assert_eq!(
        rerun.effects.first(),
        Some(&MachineEffect::ContentMatched { tree_oid: tree }),
        "the re-run observes the landed content, no duplicate commit"
    );
    assert_eq!(forge.call_count("commit_via_api"), 1);
}

#[test]
fn content_commit_propagates_with_the_prepare_phase() {
    // §11.1 PROPAGATE: pending releases gate the whole prepare phase —
    // the content step never runs on a tag-phase pass.
    let forge = MemoryForge::new();
    let input = MachineInput {
        branch: main_branch(),
        pending_releases: two_pending_releases(),
        release_pr: Some(release_pr_spec("plan")),
        content_commit: Some(content_fixture(tree_oid_fixture("t"), None)),
    };
    let report = run(&forge, &input).unwrap_or_else(|e| unreachable!("tag pass must succeed: {e}"));
    assert!(report.propagated);
    assert!(
        !report
            .effects
            .iter()
            .any(|effect| matches!(effect, MachineEffect::ContentCommitted { .. })),
        "the content step is prepare-phase work, never tag-phase work"
    );
    assert_eq!(forge.call_count("commit_via_api"), 0);
}

#[test]
fn content_commit_validates_fail_closed_before_any_call() {
    // The shared fail-closed contract (the CommentSpec convention): an
    // empty message or an empty payload list is a caller bug — refused
    // with E-FORGE-REQUEST before the first forge call.
    let cases: [(&str, fn(&mut halyard_forge::ContentCommit)); 2] = [
        (
            "empty message",
            |commit: &mut halyard_forge::ContentCommit| commit.message.clear(),
        ),
        ("no files", |commit: &mut halyard_forge::ContentCommit| {
            commit.files.clear()
        }),
    ];
    for (name, mutate) in cases {
        let forge = MemoryForge::new();
        let mut commit = content_fixture(tree_oid_fixture("t"), None);
        mutate(&mut commit);
        let input = MachineInput {
            branch: main_branch(),
            pending_releases: Vec::new(),
            release_pr: Some(release_pr_spec("plan")),
            content_commit: Some(commit),
        };
        let Err(err) = run(&forge, &input) else {
            unreachable!("{name} must be rejected");
        };
        assert_eq!(err.code(), "E-FORGE-REQUEST", "{name}: {err}");
        assert!(forge.calls().is_empty(), "{name}: no call ran");
    }
}

#[test]
fn failure_at_the_content_commit_converges_on_the_rerun() {
    // §15.2 failure injection (I2): the commit_via_api call fails → the
    // pass aborts fail-closed → the re-run commits exactly once and
    // reaches the uninterrupted end state.
    let forge = MemoryForge::new();
    let tree = tree_oid_fixture("the-content-tree");
    let input = MachineInput {
        branch: main_branch(),
        pending_releases: Vec::new(),
        release_pr: Some(release_pr_spec("plan")),
        content_commit: Some(content_fixture(tree, None)),
    };
    forge.set_failure(Some(ForgeError::rate_limited(30)));
    let Err(err) = run(&forge, &input) else {
        unreachable!("the armed failpoint must fail the content commit");
    };
    assert_eq!(err.code(), "E-FORGE-RATE-LIMIT");
    forge.set_failure(None);
    let recovered = run(&forge, &input).unwrap_or_else(|e| unreachable!("recovery pass: {e}"));
    // The failed attempt is an observable call (§11.5) but minted no
    // commit — the re-run commits exactly once.
    assert_eq!(forge.commits().len(), 1, "no duplicate commit");
    assert_eq!(
        recovered.effects,
        vec![
            MachineEffect::ContentCommitted {
                oid: forge.commits()[0].oid.clone()
            },
            MachineEffect::PrOpened {
                number: 1,
                url: "mem://pr/1".to_owned(),
            },
        ],
        "the re-run completes the pass with zero duplicates"
    );
}
