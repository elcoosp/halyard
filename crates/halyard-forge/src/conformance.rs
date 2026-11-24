//! The forge conformance suite (M7, §17: "forge conformance test-suite
//! shared across adapters"; gate: "same e2e matrix passes per forge").
//!
//! The suite is the machine's own contract turned inside out: instead of
//! one adapter's tests pinning its wire shapes, the matrix below runs the
//! *same* scenario list against every adapter that claims the
//! [`Forge`] contract, so the §11.1 semantics the machine relies on are
//! proven once and inherited by construction. The scenarios assert only
//! the invariants the seam requires of *every* adapter:
//!
//! * the §11.4 not-found values (`find_release_pr` → `None`,
//!   `release_exists` → `false`, `close_pr` → `false` on a missing PR);
//! * the I1 upsert echo (same branch ⇒ same PR number, the spec's
//!   title/body carried back);
//! * the §11.1 machine passes (PROPAGATE, prepare-upsert, the "0 API
//!   calls when no PR exists" no-op) and the §11.5 observable call log;
//! * the documented honest declines: the trait's optional capabilities
//!   (`close_pr`, `commit_via_api`, `upsert_pr_comment`) may answer
//!   [`ForgeError::Unsupported`] naming the adapter — a **declined**
//!   verdict, never a failure (§7.3 default-decline convention).
//!
//! Persistence nuances (GET-by-tag-first, rewrite-in-place) are real
//! adapter obligations but stay in each adapter's own contract tests:
//! the [`PreviewForge`] forecast persists nothing by design, so a
//! persistence assertion would be a forecast bug, not a conformance one.
//!
//! A conformance run is a test asset, not runtime machinery: it lives in
//! the seam crate so every adapter crate can dev-depend on it without a
//! cycle, and it performs no I/O — the adapters themselves carry their
//! transports.
//!
//! ```
//! use halyard_forge::{conformance::run_conformance_matrix, Forge, MemoryForge};
//!
//! let mut build = |_scenario| -> Box<dyn Forge> { Box::new(MemoryForge::new()) };
//! let report = run_conformance_matrix(&mut build);
//! assert!(report.is_green(), "{report:?}");
//! ```

use crate::Forge;
use crate::error::ForgeError;
use crate::spec::{BranchName, CommentSpec, PrSpec};
use crate::state::{MachineEffect, MachineInput, run_release_machine};

/// One row of the conformance matrix. The list doubles as the
/// documented "e2e matrix" the M7 gate speaks: every adapter's suite
/// names the same scenarios, in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scenario {
    /// The §11.4 not-found values: `find_release_pr` on an untouched
    /// branch is `None` and `release_exists` on an unused tag is `false`.
    FindOnEmpty,
    /// The I1 upsert echo: two `upsert_pr` calls with one spec — both
    /// succeed, both carry the spec's title/body back, both report the
    /// *same* PR number (same branch ⇒ same PR, no duplicates).
    UpsertEcho,
    /// `close_pr` on a branch with no Release PR is `Ok(false)` — the
    /// not-found value, not an error.
    CloseMissing,
    /// The §11.1 tag phase over the machine: pending releases propagate
    /// (the prepare phase is skipped) and exactly one release effect is
    /// reported (created or already-existing).
    MachinePropagate,
    /// The §11.1 prepare phase with a plan: the Release PR is opened
    /// (one `PrOpened` effect, `propagated == false`).
    MachinePrepareUpsert,
    /// The §11.1 prepare phase with an empty plan and no stale PR: zero
    /// effects ("exit (0 API calls when no PR exists)").
    MachinePrepareNoop,
    /// The §11.1 prepare phase's content step: a probe mismatch rides
    /// [`Forge::commit_via_api`] *before* the PR upsert. An adapter that
    /// declines the capability (`Unsupported`) takes the **declined**
    /// verdict.
    CommitCapability,
    /// The §11.6 check-comment upsert on the upserted Release PR. An
    /// adapter without a comment endpoint takes the **declined** verdict.
    CommentCapability,
    /// The §11.5 observability surface: after two operations the
    /// adapter's own call log is non-empty (a dropped call is an
    /// unobservable one — the log never is).
    CallsObservable,
}

impl Scenario {
    /// Every scenario, in execution order — the documented matrix shape.
    pub const ALL: &'static [Scenario] = &[
        Scenario::FindOnEmpty,
        Scenario::UpsertEcho,
        Scenario::CloseMissing,
        Scenario::MachinePropagate,
        Scenario::MachinePrepareUpsert,
        Scenario::MachinePrepareNoop,
        Scenario::CommitCapability,
        Scenario::CommentCapability,
        Scenario::CallsObservable,
    ];

    /// The stable scenario name (the suite's report and every adapter's
    /// scripted factory key on it).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Scenario::FindOnEmpty => "find_on_empty_is_a_value",
            Scenario::UpsertEcho => "upsert_echoes_and_is_idempotent",
            Scenario::CloseMissing => "close_missing_is_false",
            Scenario::MachinePropagate => "machine_propagate_pass",
            Scenario::MachinePrepareUpsert => "machine_prepare_upsert_pass",
            Scenario::MachinePrepareNoop => "machine_prepare_noop_pass",
            Scenario::CommitCapability => "commit_via_api_capability",
            Scenario::CommentCapability => "upsert_pr_comment_capability",
            Scenario::CallsObservable => "calls_log_observable",
        }
    }
}

/// The outcome of one scenario against one adapter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioOutcome {
    /// The scenario that ran.
    pub scenario: Scenario,
    /// The adapter's verdict.
    pub verdict: Verdict,
}

/// A scenario verdict: passed, honestly declined (the trait's optional
/// capabilities), or failed with the reason the adapter produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The scenario's assertions held.
    Passed,
    /// The adapter declined a capability through the documented
    /// [`ForgeError::Unsupported`] default, naming itself.
    Declined {
        /// The adapter name the decline carried.
        adapter: String,
    },
    /// An assertion failed, or the adapter errored with anything other
    /// than the documented decline.
    Failed {
        /// What the scenario saw (the adapter's own error, or the
        /// assertion that did not hold).
        reason: String,
    },
}

impl Verdict {
    /// The human-readable diagnosis (empty for a pass; the adapter name
    /// for a decline; the failure reason otherwise).
    #[must_use]
    pub fn reason(&self) -> &str {
        match self {
            Verdict::Passed => "",
            Verdict::Declined { adapter } => adapter,
            Verdict::Failed { reason } => reason,
        }
    }
}

/// The full matrix result: one [`ScenarioOutcome`] per
/// [`Scenario::ALL`], oldest first. Plain data (I3) — comparable in
/// tests, printable in CI logs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConformanceReport {
    outcomes: Vec<ScenarioOutcome>,
}

impl ConformanceReport {
    /// `true` when every scenario passed *or* was honestly declined.
    #[must_use]
    pub fn is_green(&self) -> bool {
        self.outcomes
            .iter()
            .all(|outcome| !matches!(outcome.verdict, Verdict::Failed { .. }))
    }

    /// The failed scenarios (empty on a green report).
    #[must_use]
    pub fn failures(&self) -> Vec<&ScenarioOutcome> {
        self.outcomes
            .iter()
            .filter(|outcome| matches!(outcome.verdict, Verdict::Failed { .. }))
            .collect()
    }

    /// Every outcome, in [`Scenario::ALL`] order.
    #[must_use]
    pub fn outcomes(&self) -> &[ScenarioOutcome] {
        &self.outcomes
    }
}

/// The adapter factory: one *fresh* forge per scenario, so the matrix
/// scenarios cannot leak state into each other (I1 assertions run on a
/// forge whose only history is the scenario's own). The `Scenario`
/// argument lets a scripted adapter pre-load exactly the responses the
/// scenario's conversation produces — the mapping is the adapter's
/// conformance wiring (see the adapter crates' `tests/conformance.rs`).
pub type ForgeFactory<'a> = &'a mut dyn FnMut(Scenario) -> Box<dyn Forge>;

/// Runs the full matrix against the factory: one fresh adapter per
/// scenario, every scenario runs (a failure never short-circuits the
/// report), and the report carries each verdict with its reason.
#[must_use]
pub fn run_conformance_matrix(build: ForgeFactory<'_>) -> ConformanceReport {
    let mut outcomes = Vec::with_capacity(Scenario::ALL.len());
    for scenario in Scenario::ALL {
        let forge = build(*scenario);
        let verdict = run_scenario(*scenario, forge.as_ref());
        outcomes.push(ScenarioOutcome {
            scenario: *scenario,
            verdict,
        });
    }
    ConformanceReport { outcomes }
}

/// Maps an adapter error onto a [`Verdict`]: the documented decline
/// (`ForgeError::Unsupported` naming the adapter) is a value, every
/// other error is a failure carrying the adapter's own diagnosis.
fn decline_or_error(error: ForgeError) -> Verdict {
    match error {
        ForgeError::Unsupported { adapter } => Verdict::Declined { adapter },
        other => Verdict::Failed {
            reason: other.to_string(),
        },
    }
}

fn fail(reason: impl Into<String>) -> Verdict {
    Verdict::Failed {
        reason: reason.into(),
    }
}

/// Runs one scenario against one adapter — the dispatcher over the
/// per-scenario functions below.
fn run_scenario(scenario: Scenario, forge: &dyn Forge) -> Verdict {
    match scenario {
        Scenario::FindOnEmpty => scenario_find_on_empty(forge),
        Scenario::UpsertEcho => scenario_upsert_echo(forge),
        Scenario::CloseMissing => scenario_close_missing(forge),
        Scenario::MachinePropagate => scenario_machine_propagate(forge),
        Scenario::MachinePrepareUpsert => scenario_machine_prepare_upsert(forge),
        Scenario::MachinePrepareNoop => scenario_machine_prepare_noop(forge),
        Scenario::CommitCapability => scenario_commit_capability(forge),
        Scenario::CommentCapability => scenario_comment_capability(forge),
        Scenario::CallsObservable => scenario_calls_observable(forge),
    }
}

/// The §11.4 not-found values: `find_release_pr` on an untouched branch
/// is `None` and `release_exists` on an unused tag is `false`.
fn scenario_find_on_empty(forge: &dyn Forge) -> Verdict {
    let branch = match BranchName::new("halyard/release/conformance-empty") {
        Ok(branch) => branch,
        Err(error) => return fail(error.to_string()),
    };
    match forge.find_release_pr(&branch) {
        Ok(None) => {}
        Ok(Some(pr)) => {
            return fail(format!(
                "find_release_pr on an untouched branch returned PR {}",
                pr.number
            ));
        }
        Err(error) => return decline_or_error(error),
    }
    match forge.release_exists("conformance-absent-v0.0.0") {
        Ok(false) => Verdict::Passed,
        Ok(true) => fail("release_exists on an unused tag returned true"),
        Err(error) => decline_or_error(error),
    }
}

/// The I1 upsert echo: two `upsert_pr` calls with one spec — both
/// succeed, both carry the spec's title/body back, both report the
/// *same* PR number.
fn scenario_upsert_echo(forge: &dyn Forge) -> Verdict {
    let spec = upsert_spec();
    let first = match forge.upsert_pr(&spec) {
        Ok(pr) => pr,
        Err(error) => return decline_or_error(error),
    };
    if first.title != spec.title || first.body != spec.body {
        return fail("the first upsert did not carry the spec's title/body back");
    }
    match forge.upsert_pr(&spec) {
        Ok(second) if second.number == first.number => Verdict::Passed,
        Ok(second) => fail(format!(
            "the same branch upserted twice produced PR {} then PR {} (I1: same branch \
             ⇒ same PR)",
            first.number, second.number
        )),
        Err(error) => decline_or_error(error),
    }
}

/// `close_pr` on a branch with no Release PR is `Ok(false)` — the
/// not-found value, not an error.
fn scenario_close_missing(forge: &dyn Forge) -> Verdict {
    match forge.close_pr(&upsert_spec().branch) {
        Ok(false) => Verdict::Passed,
        Ok(true) => fail(
            "close_pr on a branch with no Release PR reported a close — the §11.4 \
             not-found value is false",
        ),
        Err(error) => decline_or_error(error),
    }
}

/// The §11.1 tag phase over the machine: pending releases propagate and
/// exactly one release effect is reported; the Release PR is untouched.
fn scenario_machine_propagate(forge: &dyn Forge) -> Verdict {
    let input = MachineInput {
        branch: upsert_spec().branch,
        pending_releases: vec![release_spec("conformance-v0.1.0")],
        release_pr: Some(upsert_spec()),
        content_commit: None,
    };
    machine_verdict(forge, &input, |report| {
        let release_effects = report
            .effects
            .iter()
            .filter(|effect| {
                matches!(
                    effect,
                    MachineEffect::ReleaseCreated { .. } | MachineEffect::ReleaseExisted { .. }
                )
            })
            .count();
        let pr_touched = report.effects.iter().any(|effect| {
            matches!(
                effect,
                MachineEffect::PrOpened { .. }
                    | MachineEffect::PrUpdated { .. }
                    | MachineEffect::PrUnchanged { .. }
                    | MachineEffect::PrClosed { .. }
            )
        });
        if report.propagated && release_effects == 1 && !pr_touched {
            None
        } else {
            Some(format!(
                "propagated={} release_effects={release_effects} pr_touched={pr_touched} — \
                 pending releases gate the prepare phase (§11.1 step 2)",
                report.propagated
            ))
        }
    })
}

/// The §11.1 prepare phase with a plan: the Release PR is opened (one
/// `PrOpened` effect, `propagated == false`).
fn scenario_machine_prepare_upsert(forge: &dyn Forge) -> Verdict {
    let input = MachineInput {
        branch: upsert_spec().branch,
        pending_releases: Vec::new(),
        release_pr: Some(upsert_spec()),
        content_commit: None,
    };
    machine_verdict(forge, &input, |report| {
        let opened = report
            .effects
            .iter()
            .filter(|effect| matches!(effect, MachineEffect::PrOpened { .. }))
            .count();
        if !report.propagated && opened == 1 && report.effects.len() == 1 {
            None
        } else {
            Some(format!(
                "propagated={} effects={:?} — a plan with releases opens exactly one \
                 Release PR",
                report.propagated, report.effects
            ))
        }
    })
}

/// The §11.1 prepare phase with an empty plan and no stale PR: zero
/// effects ("exit (0 API calls when no PR exists)").
fn scenario_machine_prepare_noop(forge: &dyn Forge) -> Verdict {
    let input = MachineInput {
        branch: upsert_spec().branch,
        pending_releases: Vec::new(),
        release_pr: None,
        content_commit: None,
    };
    machine_verdict(forge, &input, |report| {
        if !report.propagated && report.effects.is_empty() {
            None
        } else {
            Some(format!(
                "propagated={} effects={:?} — an empty plan with no stale PR spends the \
                 one find probe and nothing else",
                report.propagated, report.effects
            ))
        }
    })
}

/// The §11.1 prepare phase's content step: a probe mismatch rides
/// [`Forge::commit_via_api`] *before* the PR upsert; a declined
/// capability takes the `Declined` verdict.
fn scenario_commit_capability(forge: &dyn Forge) -> Verdict {
    let input = MachineInput {
        branch: upsert_spec().branch,
        pending_releases: Vec::new(),
        release_pr: Some(upsert_spec()),
        content_commit: Some(content_commit()),
    };
    machine_verdict(forge, &input, |report| {
        if report
            .effects
            .iter()
            .any(|effect| matches!(effect, MachineEffect::ContentCommitted { .. }))
        {
            None
        } else {
            Some(format!(
                "a mismatched probe must ride commit_via_api before the PR upsert \
                 (§11.1 order): effects={:?}",
                report.effects
            ))
        }
    })
}

/// The §11.6 check-comment upsert on the upserted Release PR; a
/// forge without a comment endpoint declines (`Declined`).
fn scenario_comment_capability(forge: &dyn Forge) -> Verdict {
    let pr = match forge.upsert_pr(&upsert_spec()) {
        Ok(pr) => pr,
        Err(error) => return decline_or_error(error),
    };
    // The forecast adapter mints PR number 0 (never a real PR number);
    // the comment spec's 1-based contract then addresses the first note
    // slot — the preview accepts any number and persists nothing.
    let pr_number = pr.number.max(1);
    let spec = CommentSpec {
        pr_number,
        marker: "<!-- halyard:check v1 -->".to_owned(),
        body: "<!-- halyard:check v1 -->\n\nconformance".to_owned(),
    };
    match forge.upsert_pr_comment(&spec) {
        Ok(_) => Verdict::Passed,
        Err(error) => decline_or_error(error),
    }
}

/// The §11.5 observability surface: after two operations the adapter's
/// own call log is non-empty (a dropped call is an unobservable one —
/// the log never is).
fn scenario_calls_observable(forge: &dyn Forge) -> Verdict {
    let spec = upsert_spec();
    if let Err(error) = forge.find_release_pr(&spec.branch) {
        return decline_or_error(error);
    }
    if let Err(error) = forge.upsert_pr(&spec) {
        return decline_or_error(error);
    }
    if forge.calls().is_empty() {
        fail(
            "calls() is empty after two operations — the §11.5 log is the \
             observability surface the budget accounting rides",
        )
    } else {
        Verdict::Passed
    }
}

/// Runs one machine pass and maps its report onto a [`Verdict`] through
/// the scenario's own assertions (a `Some` reason is a failure).
fn machine_verdict(
    forge: &dyn Forge,
    input: &MachineInput,
    assert: impl Fn(&crate::state::MachineReport) -> Option<String>,
) -> Verdict {
    match run_release_machine(forge, input) {
        Ok(report) => match assert(&report) {
            None => Verdict::Passed,
            Some(reason) => fail(reason),
        },
        Err(error) => decline_or_error(error),
    }
}

fn upsert_spec() -> PrSpec {
    PrSpec {
        branch: BranchName::new("halyard/release/main")
            .unwrap_or_else(|error| unreachable!("static branch is valid: {error}")),
        title: "chore: release v0.1.0".to_owned(),
        body: "## 🚀 Release plan".to_owned(),
    }
}

fn release_spec(tag: &str) -> crate::spec::ReleaseSpec {
    crate::spec::ReleaseSpec {
        tag: tag.to_owned(),
        notes: "## Features".to_owned(),
    }
}

fn content_commit() -> crate::spec::ContentCommit {
    crate::spec::ContentCommit {
        message: "chore: release v0.1.0".to_owned(),
        files: vec![crate::spec::ApiFile {
            path: "crates/core/Cargo.toml".to_owned(),
            content: b"[package]\nversion = \"0.1.0\"".to_vec(),
        }],
        tree_oid: halyard_core::reason::Oid::from_hex(&[b'1'; 64])
            .unwrap_or_else(|| unreachable!("64 hex digits are a valid oid")),
        remote_tree_oid: None,
    }
}
