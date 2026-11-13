//! The mock-forge re-export contract (§7.1): the E2E suites (M4.2's §15.1
//! lane) drive git fixtures and the forge from this one crate. These
//! tests pin that the re-exports behave as the seam contract — idempotent
//! Release-PR lifecycle, per-pass call budget (§11.5) — through the
//! `halyard_test_support` import path E2E code will use.

use halyard_test_support::{BranchName, Forge, ForgeError, MemoryForge, PrSpec, ReleaseSpec};

/// The release branch fixture (§11.2 naming).
fn release_branch() -> BranchName {
    BranchName::new("halyard/release/main").unwrap_or_else(|e| unreachable!("valid branch: {e}"))
}

#[test]
fn release_pr_lifecycle_is_idempotent_across_passes() {
    let forge = MemoryForge::new();
    let branch = release_branch();

    // Pass 1 (§11.1 prepare phase): nothing pending ⇒ open the Release PR.
    let first = forge
        .upsert_pr(&PrSpec {
            branch: branch.clone(),
            title: "chore: release v0.1.0".to_owned(),
            body: "## 🚀 Release plan\n\n| core | 0.1.0 | 0.2.0 | minor |".to_owned(),
        })
        .unwrap_or_else(|e: ForgeError| unreachable!("pass 1 upsert: {e}"));

    // Pass 2 (the merge of the PR created new commits; nothing to tag):
    // find the existing PR, rewrite its body in place — same number (I1).
    let found = forge
        .find_release_pr(&branch)
        .unwrap_or_else(|e| unreachable!("pass 2 find: {e}"));
    let updated = forge
        .upsert_pr(&PrSpec {
            branch: branch.clone(),
            title: "chore: release v0.1.1".to_owned(),
            body: "## 🚀 Release plan\n\n| core | 0.1.1 | 0.2.0 | minor |".to_owned(),
        })
        .unwrap_or_else(|e| unreachable!("pass 2 upsert: {e}"));
    assert_eq!(found.map(|pr| pr.number), Some(first.number));
    assert_eq!(
        updated.number, first.number,
        "no duplicate PR across passes"
    );
    assert!(updated.body.contains("0.1.1"), "body rewritten in place");
    assert_eq!(forge.prs().len(), 1, "exactly one Release PR exists");

    // Tag phase (§11.1 step 2): create the release; the rerun re-finds it
    // (GET-by-tag-first, I1) instead of duplicating the artifact.
    let spec = ReleaseSpec {
        tag: "core-v0.2.0".to_owned(),
        notes: "## Features\n\n- the painter walk".to_owned(),
    };
    let created = forge
        .create_release(&spec)
        .unwrap_or_else(|e| unreachable!("create release: {e}"));
    let recreated = forge
        .create_release(&spec)
        .unwrap_or_else(|e| unreachable!("recreate release: {e}"));
    assert!(created.created);
    assert!(!recreated.created, "rerun finds the existing release");
    assert_eq!(created.url, recreated.url);
}

#[test]
fn update_pass_stays_within_the_call_budget() {
    // §11.5: a steady-state update pass is find (≤ 1 call) + upsert
    // (1 call). The mock's call log is the assertion surface; the M4.1
    // REST adapter replays the same assertion over real HTTP.
    let forge = MemoryForge::new();
    let branch = release_branch();
    let spec = PrSpec {
        branch: branch.clone(),
        title: "chore: release v0.1.0".to_owned(),
        body: "plan".to_owned(),
    };
    let _ = forge
        .upsert_pr(&spec)
        .unwrap_or_else(|e| unreachable!("seed upsert: {e}"));
    let seed_calls = forge.calls().len();
    let _ = forge
        .find_release_pr(&branch)
        .unwrap_or_else(|e| unreachable!("update find: {e}"));
    let _ = forge
        .upsert_pr(&spec)
        .unwrap_or_else(|e| unreachable!("update upsert: {e}"));
    assert_eq!(
        forge.calls().len() - seed_calls,
        2,
        "one update pass = find + upsert: {:?}",
        forge.calls()
    );
}
