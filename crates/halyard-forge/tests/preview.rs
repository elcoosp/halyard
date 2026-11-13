//! The [`PreviewForge`] contract (M4.2b step 1): the dry-run adapter
//! behind `halyard pr --dry`. Every probe answers honestly about an
//! *empty, unobservable* forge (offline mode cannot see the remote);
//! every mutation succeeds without persisting anything; every call is
//! recorded so the §11.5 call-budget forecast is observable. The tests
//! run through `&dyn Forge` (object safety rides along) and pin the
//! machine composition — the exact pass `halyard pr --dry` performs.

use halyard_forge::{
    BranchName, Forge, MachineInput, PrSpec, PreviewForge, ReleaseSpec, run_release_machine,
};

fn main_branch() -> BranchName {
    BranchName::new("halyard/release/main")
        .unwrap_or_else(|e| unreachable!("branch fixture must be valid: {e}"))
}

fn spec(branch: BranchName) -> PrSpec {
    PrSpec {
        branch,
        title: "chore: release p1-v0.1.1".to_owned(),
        body: "## 🚀 Release plan".to_owned(),
    }
}

#[test]
fn find_returns_none_and_records_the_probe() {
    let forge = PreviewForge::new();
    let branch = main_branch();
    let dyn_forge: &dyn Forge = &forge;
    let found = dyn_forge
        .find_release_pr(&branch)
        .unwrap_or_else(|e| unreachable!("preview probes never fail: {e}"));
    assert_eq!(found, None, "offline mode cannot observe the remote");
    let calls = forge.calls();
    assert_eq!(calls.len(), 1, "the probe is observable");
    let first = calls
        .first()
        .unwrap_or_else(|| unreachable!("one recorded call"));
    assert_eq!(first.method, "find_release_pr");
    assert_eq!(first.target, "halyard/release/main");
}

#[test]
fn upsert_reports_a_preview_pr_without_storing_it() {
    let forge = PreviewForge::new();
    let branch = main_branch();
    let dyn_forge: &dyn Forge = &forge;
    let pr = dyn_forge
        .upsert_pr(&spec(branch.clone()))
        .unwrap_or_else(|e| unreachable!("preview mutations never fail: {e}"));
    assert_eq!(pr.number, 0, "0 marks a preview PR by shape");
    assert_eq!(pr.branch, "halyard/release/main");
    assert_eq!(pr.title, "chore: release p1-v0.1.1");
    assert!(
        pr.url.starts_with("preview://"),
        "preview URLs are marked: {}",
        pr.url
    );
    // Nothing persisted: the re-run's probe still sees an empty forge,
    // and the second forecast is as fresh as the first (I1 trivially —
    // a dry run cannot mutate state).
    let again = dyn_forge
        .find_release_pr(&branch)
        .unwrap_or_else(|e| unreachable!("preview probes never fail: {e}"));
    assert_eq!(again, None);
    let second = dyn_forge
        .upsert_pr(&spec(branch))
        .unwrap_or_else(|e| unreachable!("preview mutations never fail: {e}"));
    assert_eq!(second.number, 0);
}

#[test]
fn release_creation_previews_created_true_without_a_row() {
    let forge = PreviewForge::new();
    let dyn_forge: &dyn Forge = &forge;
    let release = dyn_forge
        .create_release(&ReleaseSpec {
            tag: "p1-v0.1.1".to_owned(),
            notes: "## Features".to_owned(),
        })
        .unwrap_or_else(|e| unreachable!("preview mutations never fail: {e}"));
    assert!(release.created, "the preview always reports the fresh path");
    assert!(release.url.starts_with("preview://"), "{}", release.url);
    let exists = dyn_forge
        .release_exists("p1-v0.1.1")
        .unwrap_or_else(|e| unreachable!("preview probes never fail: {e}"));
    assert!(!exists, "nothing was stored");
}

#[test]
fn close_pr_is_a_value_false() {
    let forge = PreviewForge::new();
    let dyn_forge: &dyn Forge = &forge;
    let closed = dyn_forge
        .close_pr(&main_branch())
        .unwrap_or_else(|e| unreachable!("preview probes never fail: {e}"));
    assert!(!closed, "nothing stored ⇒ nothing to close (§11.4: value)");
}

#[test]
fn commit_via_api_declines_like_the_seam_default() {
    let forge = PreviewForge::new();
    let dyn_forge: &dyn Forge = &forge;
    let branch = main_branch();
    let error = dyn_forge
        .commit_via_api(&halyard_forge::ApiCommit {
            branch,
            message: "chore: release".to_owned(),
            files: Vec::new(),
        })
        .unwrap_err();
    assert_eq!(error.code(), "E-FORGE-UNSUPPORTED");
}

#[test]
fn machine_pass_over_the_preview_forge_is_the_open_forecast() {
    let forge = PreviewForge::new();
    let branch = main_branch();
    let report = run_release_machine(
        &forge,
        &MachineInput {
            branch: branch.clone(),
            pending_releases: Vec::new(),
            release_pr: Some(spec(branch)),
        },
    )
    .unwrap_or_else(|e| unreachable!("preview passes never fail: {e}"));
    assert!(!report.propagated, "no pending releases ⇒ prepare runs");
    assert_eq!(
        report.effects,
        vec![halyard_forge::MachineEffect::PrOpened { number: 0 }],
        "an empty forge ⇒ the open path"
    );
    assert_eq!(report.calls, 2, "find + upsert (§11.5 update-path budget)");
}

#[test]
fn machine_pass_with_an_empty_plan_spends_one_probe() {
    let forge = PreviewForge::new();
    let report = run_release_machine(
        &forge,
        &MachineInput {
            branch: main_branch(),
            pending_releases: Vec::new(),
            release_pr: None,
        },
    )
    .unwrap_or_else(|e| unreachable!("preview passes never fail: {e}"));
    assert!(
        report.effects.is_empty(),
        "no PR observable ⇒ nothing to close"
    );
    assert_eq!(report.calls, 1, "the find probe and nothing else");
}

#[test]
fn machine_tag_phase_forecasts_the_release_creations() {
    let forge = PreviewForge::new();
    let report = run_release_machine(
        &forge,
        &MachineInput {
            branch: main_branch(),
            pending_releases: vec![ReleaseSpec {
                tag: "p1-v0.1.1".to_owned(),
                notes: "## Features".to_owned(),
            }],
            release_pr: None,
        },
    )
    .unwrap_or_else(|e| unreachable!("preview passes never fail: {e}"));
    assert!(
        report.propagated,
        "pending releases ⇒ prepare skipped (§11.1)"
    );
    assert_eq!(
        report.effects,
        vec![halyard_forge::MachineEffect::ReleaseCreated {
            tag: "p1-v0.1.1".to_owned(),
            url: "preview://p1-v0.1.1".to_owned(),
        }]
    );
    assert_eq!(report.calls, 1);
}

#[test]
fn preview_forge_is_object_safe_and_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<PreviewForge>();
    let forge: Box<dyn Forge> = Box::new(PreviewForge::new());
    let branch = main_branch();
    let found = forge
        .find_release_pr(&branch)
        .unwrap_or_else(|e| unreachable!("preview probes never fail: {e}"));
    assert_eq!(found, None);
}
