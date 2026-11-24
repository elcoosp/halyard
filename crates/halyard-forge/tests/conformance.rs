//! The forge conformance suite over the two in-crate reference adapters
//! (M7, §17 "forge conformance test-suite shared across adapters"):
//! [`MemoryForge`] (the stateful reference) and [`PreviewForge`] (the
//! dry-run forecast) prove the runner's semantics. The REST adapters run
//! the same matrix in their own crates' suites (`halyard-forge-github`,
//! `halyard-forge-gitlab`) — the M7 gate is "same e2e matrix passes per
//! forge".
//!
//! The matrix asserts only the invariants the seam contract requires of
//! *every* adapter: the §11.4 not-found values, the I1 upsert echo
//! (same branch ⇒ same PR number), the §11.1 machine passes and the
//! documented honest declines (the trait defaults). Persistence
//! nuances (GET-by-tag-first, rewrite-in-place) stay in each adapter's
//! own contract tests — [`PreviewForge`] deliberately persists nothing.

use halyard_core::reason::Oid;
use halyard_forge::conformance::{Scenario, Verdict, run_conformance_matrix};
use halyard_forge::{
    BranchName, CallRecord, Forge, ForgeError, MachineInput, MemoryForge, PrSpec, PreviewForge,
    RateInfo, ReleaseSpec, run_release_machine,
};

const BRANCH: &str = "halyard/release/main";

fn branch() -> BranchName {
    BranchName::new(BRANCH).unwrap_or_else(|error| unreachable!("static branch is valid: {error}"))
}

fn spec() -> PrSpec {
    PrSpec {
        branch: branch(),
        title: "chore: release v0.1.0".to_owned(),
        body: "## 🚀 Release plan".to_owned(),
    }
}

/// The content commit the machine's §11.1 step-3 content step carries
/// (a mismatched probe ⇒ the `commit_via_api` path).
fn content() -> halyard_forge::ContentCommit {
    halyard_forge::ContentCommit {
        message: "chore: release v0.1.0".to_owned(),
        files: vec![halyard_forge::ApiFile {
            path: "crates/core/Cargo.toml".to_owned(),
            content: b"[package]\nversion = \"0.1.0\"".to_vec(),
        }],
        tree_oid: Oid::from_hex(
            b"1111111111111111111111111111111111111111111111111111111111111111",
        )
        .unwrap_or_else(|| unreachable!("64 hex digits are a valid oid")),
        remote_tree_oid: None,
    }
}

/// A stateful adapter that declines every optional capability through
/// the trait defaults (`close_pr`, `commit_via_api`,
/// `upsert_pr_comment`): the Declined verdict path of the matrix.
#[derive(Debug, Default)]
struct PartialForge {
    inner: MemoryForge,
}

impl Forge for PartialForge {
    fn find_release_pr(
        &self,
        branch: &BranchName,
    ) -> Result<Option<halyard_forge::Pr>, ForgeError> {
        self.inner.find_release_pr(branch)
    }
    fn upsert_pr(&self, spec: &PrSpec) -> Result<halyard_forge::Pr, ForgeError> {
        self.inner.upsert_pr(spec)
    }
    fn release_exists(&self, tag: &str) -> Result<bool, ForgeError> {
        self.inner.release_exists(tag)
    }
    fn create_release(&self, spec: &ReleaseSpec) -> Result<halyard_forge::Release, ForgeError> {
        self.inner.create_release(spec)
    }
    fn rate_limit(&self) -> RateInfo {
        self.inner.rate_limit()
    }
    fn calls(&self) -> Vec<CallRecord> {
        self.inner.calls()
    }
}

/// A broken adapter: every probe fails with an auth error — the Failed
/// verdict path (the report names the scenario and the reason).
#[derive(Debug, Default)]
struct BrokenForge;

impl Forge for BrokenForge {
    fn find_release_pr(
        &self,
        _branch: &BranchName,
    ) -> Result<Option<halyard_forge::Pr>, ForgeError> {
        Err(ForgeError::auth("broken"))
    }
    fn upsert_pr(&self, _spec: &PrSpec) -> Result<halyard_forge::Pr, ForgeError> {
        Err(ForgeError::auth("broken"))
    }
    fn release_exists(&self, _tag: &str) -> Result<bool, ForgeError> {
        Err(ForgeError::auth("broken"))
    }
    fn create_release(&self, _spec: &ReleaseSpec) -> Result<halyard_forge::Release, ForgeError> {
        Err(ForgeError::auth("broken"))
    }
    fn rate_limit(&self) -> RateInfo {
        RateInfo::MODEL_DEFAULT
    }
    fn calls(&self) -> Vec<CallRecord> {
        Vec::new()
    }
}

fn machine_input(
    pending: Vec<ReleaseSpec>,
    release_pr: Option<PrSpec>,
    content_commit: Option<halyard_forge::ContentCommit>,
) -> MachineInput {
    MachineInput {
        branch: branch(),
        pending_releases: pending,
        release_pr,
        content_commit,
    }
}

#[test]
fn matrix_is_green_over_memory_forge() {
    let mut build = |_scenario: Scenario| -> Box<dyn Forge> { Box::new(MemoryForge::new()) };
    let report = run_conformance_matrix(&mut build);
    assert!(
        report.is_green(),
        "memory forge fails the matrix: {report:?}"
    );
}

#[test]
fn matrix_is_green_over_preview_forge() {
    let mut build = |_scenario: Scenario| -> Box<dyn Forge> { Box::new(PreviewForge::new()) };
    let report = run_conformance_matrix(&mut build);
    assert!(
        report.is_green(),
        "preview forge fails the matrix: {report:?}"
    );
}

#[test]
fn matrix_declines_optional_capabilities() {
    let mut build = |_scenario: Scenario| -> Box<dyn Forge> { Box::new(PartialForge::default()) };
    let report = run_conformance_matrix(&mut build);
    let declined = report
        .outcomes()
        .iter()
        .filter(|outcome| matches!(outcome.verdict, Verdict::Declined { .. }))
        .count();
    assert_eq!(
        declined, 3,
        "the trait-default declines are close/comment/commit: {report:?}"
    );
    assert!(
        report.is_green(),
        "a documented decline is not a failure: {report:?}"
    );
}

#[test]
fn matrix_reports_failures_with_reasons() {
    let mut build = |_scenario: Scenario| -> Box<dyn Forge> { Box::new(BrokenForge) };
    let report = run_conformance_matrix(&mut build);
    assert!(!report.is_green(), "a broken adapter must fail the matrix");
    let failed = report.failures();
    assert!(
        failed
            .iter()
            .all(|outcome| outcome.verdict.reason().contains("broken")),
        "the reason carries the adapter's own error: {failed:?}"
    );
    // 9 scenarios run; the two capabilities the broken adapter leaves on
    // the trait defaults (`close_pr`, `commit_via_api`) are honest
    // declines, the other 7 surface the adapter's own auth error.
    assert_eq!(report.outcomes().len(), 9);
    assert_eq!(
        failed.len(),
        7,
        "every scenario runs, none skipped: {report:?}"
    );
}

#[test]
fn matrix_names_are_stable_and_ordered() {
    assert_eq!(Scenario::ALL.len(), 9, "the documented matrix shape");
    let expected_first = [
        "find_on_empty_is_a_value",
        "upsert_echoes_and_is_idempotent",
    ];
    for (scenario, name) in Scenario::ALL.iter().zip(expected_first) {
        assert_eq!(scenario.name(), name);
    }
}

#[test]
fn machine_content_pass_commits_over_memory() {
    // The suite's content scenario is a full machine pass: the probe
    // mismatch ⇒ commit_via_api runs *before* the PR upsert (§11.1
    // order), and the report shows both effects.
    let forge = MemoryForge::new();
    let report = run_release_machine(
        &forge,
        &machine_input(Vec::new(), Some(spec()), Some(content())),
    )
    .unwrap_or_else(|error| unreachable!("memory forge supports the content pass: {error}"));
    assert!(
        report.effects.iter().any(|effect| matches!(
            effect,
            halyard_forge::MachineEffect::ContentCommitted { .. }
        )),
        "the mismatched probe rides the forge-signed commit: {:?}",
        report.effects
    );
}
