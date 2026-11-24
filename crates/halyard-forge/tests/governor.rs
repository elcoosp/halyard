//! The §11.5 call governor (M5.5): pacing, Retry-After honoring,
//! deterministic backoff and the per-run call budget, pinned over the
//! [`MemoryForge`] reference adapter and a scripted failing forge.
//!
//! The governor is a [`Forge`] decorator: it forwards every operation to
//! an inner adapter while enforcing the §11.5 secondary-rate-limit
//! contract the `E-FORGE-RATE-LIMIT` page promises — content-creating
//! calls are spaced, `Retry-After` is honored exactly, transport
//! failures back off exponentially, and a per-run call budget fails
//! closed (`E-FORGE-BUDGET`) instead of spending an unbounded run. Time
//! never enters the crate: the clock and sleeper are injected, so every
//! schedule here is asserted as data (I3 — the same script produces the
//! same event log).
//!
//! The failure-injection shape reuses the §15.2 convention: a failing
//! run re-run to completion reaches the uninterrupted end state with
//! zero duplicates (I2).

use std::sync::Arc;
use std::sync::Mutex;

use halyard_forge::error::ForgeError;
use halyard_forge::governor::{
    Governor, GovernorClock, GovernorEvent, GovernorPolicy, GovernorSleeper,
};
use halyard_forge::{
    ApiCommit, BranchName, CallRecord, Forge, MachineInput, MemoryForge, Pr, PrSpec, RateInfo,
    Release, ReleaseSpec, run_release_machine,
};

/// A branch name shared by the test sequences (validated once, reused).
fn main_branch() -> BranchName {
    BranchName::new("halyard/release/main")
        .unwrap_or_else(|e| unreachable!("branch fixture must be valid: {e}"))
}

/// A Release-PR spec fixture (the prepare phase's input).
fn release_pr_spec() -> PrSpec {
    PrSpec {
        branch: main_branch(),
        title: "chore: release v0.2.0".to_owned(),
        body: "## 🚀 Release plan".to_owned(),
    }
}

/// A pending-release fixture (the tag phase's input, §11.1 step 2).
fn one_pending_release() -> Vec<ReleaseSpec> {
    vec![ReleaseSpec {
        tag: "core-v0.5.0".to_owned(),
        notes: "## Features\n\n- the call governor".to_owned(),
    }]
}

/// A machine input with nothing pending (the prepare phase's shape).
fn prepare_input() -> MachineInput {
    MachineInput {
        branch: main_branch(),
        pending_releases: Vec::new(),
        release_pr: Some(release_pr_spec()),
        content_commit: None,
    }
}

/// The scripted time pair: the clock reads `now_ms`, the sleeper records
/// the requested wait and advances the clock. Tests assert on the
/// recorded sleeps — the schedule is data, never wall-clock (I3).
fn scripted_time(start_ms: u64) -> (GovernorClock, GovernorSleeper, Arc<Mutex<Script>>) {
    let script = Arc::new(Mutex::new(Script {
        now_ms: start_ms,
        sleeps: Vec::new(),
    }));
    let clock: GovernorClock = {
        let script = Arc::clone(&script);
        Arc::new(move || script.lock().ok().map_or(0, |script| script.now_ms))
    };
    let sleeper: GovernorSleeper = {
        let script = Arc::clone(&script);
        Arc::new(move |ms| {
            if let Ok(mut script) = script.lock() {
                script.sleeps.push(ms);
                script.now_ms += ms;
            }
        })
    };
    (clock, sleeper, script)
}

/// The state the scripted clock/sleeper share.
struct Script {
    now_ms: u64,
    sleeps: Vec<u64>,
}

/// A forge that fails the first `remaining` forwarded calls with a
/// template error, then delegates to a [`MemoryForge`]. Every forwarded
/// call — failed or not — is recorded in its own log (§11.5: a dropped
/// call is an observable one), so the governor's retry accounting and
/// the adapter ledger can be asserted independently.
struct FailFirstForge {
    remaining: Mutex<u32>,
    template: ForgeError,
    inner: MemoryForge,
    calls: Mutex<Vec<CallRecord>>,
}

impl FailFirstForge {
    fn new(remaining: u32, template: ForgeError) -> Self {
        Self {
            remaining: Mutex::new(remaining),
            template,
            inner: MemoryForge::new(),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// The inner forge (state assertions after the run).
    fn inner(&self) -> &MemoryForge {
        &self.inner
    }

    /// `true` when this call still fails: consumes one remaining failure.
    fn take_failure(&self) -> bool {
        let mut remaining = self
            .remaining
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *remaining > 0 {
            *remaining -= 1;
            true
        } else {
            false
        }
    }

    /// Records the call, then fails or delegates.
    fn op<T>(
        &self,
        method: &'static str,
        target: &str,
        body: impl FnOnce(&MemoryForge) -> Result<T, ForgeError>,
    ) -> Result<T, ForgeError> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(CallRecord {
                method,
                target: target.to_owned(),
            });
        if self.take_failure() {
            return Err(self.template.clone());
        }
        body(&self.inner)
    }
}

impl Forge for FailFirstForge {
    fn find_release_pr(&self, branch: &BranchName) -> Result<Option<Pr>, ForgeError> {
        self.op("find_release_pr", branch.as_str(), |inner| {
            inner.find_release_pr(branch)
        })
    }

    fn upsert_pr(&self, spec: &PrSpec) -> Result<Pr, ForgeError> {
        self.op("upsert_pr", spec.branch.as_str(), |inner| {
            inner.upsert_pr(spec)
        })
    }

    fn release_exists(&self, tag: &str) -> Result<bool, ForgeError> {
        self.op("release_exists", tag, |inner| inner.release_exists(tag))
    }

    fn create_release(&self, spec: &ReleaseSpec) -> Result<Release, ForgeError> {
        self.op("create_release", &spec.tag, |inner| {
            inner.create_release(spec)
        })
    }

    fn close_pr(&self, branch: &BranchName) -> Result<bool, ForgeError> {
        self.op("close_pr", branch.as_str(), |inner| inner.close_pr(branch))
    }

    fn commit_via_api(&self, spec: &ApiCommit) -> Result<halyard_core::reason::Oid, ForgeError> {
        self.op("commit_via_api", spec.branch.as_str(), |inner| {
            inner.commit_via_api(spec)
        })
    }

    fn rate_limit(&self) -> RateInfo {
        self.inner.rate_limit()
    }

    fn calls(&self) -> Vec<CallRecord> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

#[test]
fn passive_governor_is_transparent() {
    // The default policy paces nothing, budgets nothing, retries
    // nothing: the machine pass over the governor produces exactly the
    // bare-adapter report, and the event log stays empty. Two
    // equally-constructed forges carry the two passes — one shared
    // forge would leak the first pass's PR into the second.
    let forge = MemoryForge::new();
    let (clock, sleeper, script) = scripted_time(0);
    let governor = Governor::new(&forge, GovernorPolicy::default(), clock, sleeper);

    let bare = MemoryForge::new();
    let direct = run_release_machine(&bare, &prepare_input())
        .unwrap_or_else(|e| unreachable!("bare pass must succeed: {e}"));
    let governed = run_release_machine(&governor, &prepare_input())
        .unwrap_or_else(|e| unreachable!("governed pass must succeed: {e}"));
    assert_eq!(direct, governed, "passive governor is transparent");
    assert!(
        governor.events().is_empty(),
        "no events: {:?}",
        governor.events()
    );
    assert_eq!(
        governor.spent(),
        u32::try_from(direct.calls).unwrap_or(u32::MAX),
        "spent = forwarded calls"
    );
    assert!(
        script
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .sleeps
            .is_empty(),
        "no waits without a policy"
    );
}

#[test]
fn pacing_spaces_content_calls_not_probes() {
    // §11.5: content-creating calls are serialized with pacing. Two
    // content calls back-to-back with min_spacing_ms = 1_000 wait one
    // spacing interval; the find probe between them is unpaced.
    let forge = MemoryForge::new();
    let (clock, sleeper, script) = scripted_time(10_000);
    let policy = GovernorPolicy {
        min_spacing_ms: 1_000,
        ..GovernorPolicy::default()
    };
    let governor = Governor::new(&forge, policy, clock, sleeper);

    governor
        .create_release(
            one_pending_release()
                .first()
                .unwrap_or_else(|| unreachable!("one pending release")),
        )
        .unwrap_or_else(|e| unreachable!("first release must succeed: {e}"));
    governor
        .find_release_pr(&main_branch())
        .unwrap_or_else(|e| unreachable!("probe must succeed: {e}"));
    governor
        .upsert_pr(&release_pr_spec())
        .unwrap_or_else(|e| unreachable!("upsert must succeed: {e}"));

    let script = script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(
        script.sleeps,
        vec![1_000],
        "one spacing wait between the two content calls; the probe is unpaced"
    );
    assert_eq!(
        governor.events(),
        vec![GovernorEvent::Paced {
            op: "upsert_pr",
            waited_ms: 1_000,
        }],
        "the second content call was paced: {:?}",
        governor.events()
    );
}

#[test]
fn budget_blocks_fail_closed_before_the_call() {
    // The per-run budget refuses the call before it reaches the forge
    // (`E-FORGE-BUDGET`, fail-closed): the third operation is spent-2
    // blocked, the forge ledger stays at two calls, and the refusal is
    // an observable event.
    let forge = MemoryForge::new();
    let (clock, sleeper, _script) = scripted_time(0);
    let policy = GovernorPolicy {
        max_calls: Some(2),
        ..GovernorPolicy::default()
    };
    let governor = Governor::new(&forge, policy, clock, sleeper);

    governor
        .find_release_pr(&main_branch())
        .unwrap_or_else(|e| unreachable!("probe 1 must succeed: {e}"));
    governor
        .release_exists("core-v0.5.0")
        .unwrap_or_else(|e| unreachable!("probe 2 must succeed: {e}"));
    let refused = governor
        .upsert_pr(&release_pr_spec())
        .unwrap_err_or_else_refused();

    match &refused {
        ForgeError::Budget { op, spent, budget } => {
            assert_eq!(op, "upsert_pr");
            assert_eq!((*spent, *budget), (2, 2));
        }
        other => unreachable!("budget refusal expected, got: {other}"),
    }
    assert_eq!(
        forge.call_count("upsert_pr"),
        0,
        "the refused call never reached the forge"
    );
    assert_eq!(
        governor.events(),
        vec![GovernorEvent::BudgetBlocked {
            op: "upsert_pr",
            spent: 2,
        }],
    );
}

#[test]
fn rate_limit_retry_honors_retry_after() {
    // §11.5: honor `Retry-After`. One injected rate limit, then success:
    // the retry sleeps exactly the suggested wait, the attempt is
    // observable in the forge ledger, and the retry is an event.
    let forge = FailFirstForge::new(1, ForgeError::rate_limited(30));
    let (clock, sleeper, script) = scripted_time(0);
    let policy = GovernorPolicy {
        max_retries: 3,
        ..GovernorPolicy::default()
    };
    let governor = Governor::new(&forge, policy, clock, sleeper);

    let release = governor
        .create_release(
            one_pending_release()
                .first()
                .unwrap_or_else(|| unreachable!("one pending release")),
        )
        .unwrap_or_else(|e| unreachable!("retry must succeed: {e}"));
    assert!(release.created, "the retried call created the release");
    let script = script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(script.sleeps, vec![30_000], "Retry-After honored exactly");
    assert_eq!(
        governor.events(),
        vec![GovernorEvent::RateLimitRetry {
            op: "create_release",
            attempt: 0,
            waited_ms: 30_000,
        }],
    );
    assert_eq!(governor.spent(), 2, "both attempts are observable spend");
    assert_eq!(
        forge.calls().len(),
        2,
        "the failed attempt is in the adapter ledger too (§11.5)"
    );
}

#[test]
fn transport_backoff_is_deterministic_exponential() {
    // §11.5: exponential backoff on transport failures. Two failures,
    // then success: the waits double from the base and are capped — a
    // pure function of the attempt (no jitter: the schedule must stay
    // byte-predictable, I3).
    let forge = FailFirstForge::new(2, ForgeError::transport("connection reset"));
    let (clock, sleeper, script) = scripted_time(0);
    let policy = GovernorPolicy {
        max_retries: 3,
        backoff_base_ms: 500,
        backoff_cap_ms: 4_000,
        ..GovernorPolicy::default()
    };
    let governor = Governor::new(&forge, policy, clock, sleeper);

    governor
        .upsert_pr(&release_pr_spec())
        .unwrap_or_else(|e| unreachable!("backoff run must succeed: {e}"));
    let script = script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(script.sleeps, vec![500, 1_000], "base, then 2×base");
    assert_eq!(
        governor.events(),
        vec![
            GovernorEvent::TransportRetry {
                op: "upsert_pr",
                attempt: 0,
                waited_ms: 500,
            },
            GovernorEvent::TransportRetry {
                op: "upsert_pr",
                attempt: 1,
                waited_ms: 1_000,
            },
        ],
    );
}

#[test]
fn backoff_is_capped_at_the_policy_cap() {
    // The doubling never exceeds `backoff_cap_ms` — a long outage waits
    // the cap, not an unbounded exponential.
    let forge = FailFirstForge::new(4, ForgeError::transport("connection reset"));
    let (clock, sleeper, script) = scripted_time(0);
    let policy = GovernorPolicy {
        max_retries: 4,
        backoff_base_ms: 2_000,
        backoff_cap_ms: 5_000,
        ..GovernorPolicy::default()
    };
    let governor = Governor::new(&forge, policy, clock, sleeper);

    governor
        .upsert_pr(&release_pr_spec())
        .unwrap_or_else(|e| unreachable!("capped run must succeed: {e}"));
    let script = script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(
        script.sleeps,
        vec![2_000, 4_000, 5_000, 5_000],
        "doubles then rides the cap"
    );
}

#[test]
fn retries_exhausted_return_the_last_error() {
    // A forge that never recovers: after `max_retries` the governor
    // gives up and returns the last error verbatim — the §15.4 code
    // passes through fail-closed, never swallowed into a success.
    let forge = FailFirstForge::new(u32::MAX, ForgeError::rate_limited(5));
    let (clock, sleeper, _script) = scripted_time(0);
    let policy = GovernorPolicy {
        max_retries: 2,
        ..GovernorPolicy::default()
    };
    let governor = Governor::new(&forge, policy, clock, sleeper);

    let error = governor
        .create_release(
            one_pending_release()
                .first()
                .unwrap_or_else(|| unreachable!("one pending release")),
        )
        .unwrap_err_or_else_refused();
    assert_eq!(
        error,
        ForgeError::rate_limited(5),
        "the last error passes through verbatim"
    );
    assert_eq!(governor.spent(), 3, "initial attempt + 2 retries");
    assert_eq!(governor.events().len(), 2, "one retry event per retry");
}

#[test]
fn non_retryable_errors_pass_through_without_waiting() {
    // Auth/Conflict/Request/Unsupported/Budget are never retried: the
    // error surfaces immediately, no sleep is recorded, no event.
    let forge = FailFirstForge::new(1, ForgeError::auth("bad token"));
    let (clock, sleeper, script) = scripted_time(0);
    let policy = GovernorPolicy {
        max_retries: 3,
        ..GovernorPolicy::default()
    };
    let governor = Governor::new(&forge, policy, clock, sleeper);

    let error = governor
        .upsert_pr(&release_pr_spec())
        .unwrap_err_or_else_refused();
    assert_eq!(error, ForgeError::auth("bad token"));
    let script = script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert!(script.sleeps.is_empty(), "no wait on a non-retryable error");
    assert!(governor.events().is_empty(), "no retry events");
    assert_eq!(governor.spent(), 1, "the failed attempt is still spend");
}

#[test]
fn budget_counts_injected_failures_and_retries() {
    // The budget is a ledger of forwarded attempts, not of successes: a
    // failed attempt (§11.5 observable spend) and its retry both count.
    let forge = FailFirstForge::new(1, ForgeError::rate_limited(1));
    let (clock, sleeper, _script) = scripted_time(0);
    let policy = GovernorPolicy {
        max_calls: Some(2),
        max_retries: 3,
        ..GovernorPolicy::default()
    };
    let governor = Governor::new(&forge, policy, clock, sleeper);

    governor
        .upsert_pr(&release_pr_spec())
        .unwrap_or_else(|e| unreachable!("one failure then success: {e}"));
    assert_eq!(governor.spent(), 2, "failure + retry");
    let refused = governor
        .find_release_pr(&main_branch())
        .unwrap_err_or_else_refused();
    match &refused {
        ForgeError::Budget { spent, .. } => assert_eq!(*spent, 2),
        other => unreachable!("budget refusal expected, got: {other}"),
    }
}

#[test]
fn machine_composition_re_run_reaches_uninterrupted_state() {
    // §15.2 / I2: a rate-limited pass aborts fail-closed; the re-run
    // over the same governor (failure disarmed) reaches the
    // uninterrupted end state — one PR, zero duplicates.
    let forge = FailFirstForge::new(1, ForgeError::rate_limited(1));
    let (clock, sleeper, _script) = scripted_time(0);
    let policy = GovernorPolicy {
        max_retries: 0,
        ..GovernorPolicy::default()
    };
    let governor = Governor::new(&forge, policy, clock, sleeper);

    let first = run_release_machine(&governor, &prepare_input());
    assert!(
        first.is_err(),
        "the armed pass aborts: {:?}",
        first.unwrap_err_or_else_refused()
    );
    // The single armed failure was consumed by the aborted pass (the
    // governor retried nothing: max_retries = 0), so the re-run goes
    // through the healed adapter.
    let second = run_release_machine(&governor, &prepare_input())
        .unwrap_or_else(|e| unreachable!("re-run must succeed: {e}"));
    assert_eq!(second.effects.len(), 1, "one PR opened, zero duplicates");
    let prs = forge.inner().prs();
    assert_eq!(prs.len(), 1, "exactly one Release PR exists (I1)");
}

#[test]
fn events_and_spent_are_deterministic() {
    // I3: the same operation script over equally-constructed forges
    // produces identical event logs and spend — the governor adds no
    // nondeterminism of its own.
    let build = |start_ms: u64| {
        let forge = FailFirstForge::new(1, ForgeError::transport("flaky"));
        let (clock, sleeper, _script) = scripted_time(start_ms);
        let policy = GovernorPolicy {
            max_retries: 1,
            backoff_base_ms: 100,
            ..GovernorPolicy::default()
        };
        let governor = Governor::new(&forge, policy, clock, sleeper);
        governor
            .upsert_pr(&release_pr_spec())
            .unwrap_or_else(|e| unreachable!("scripted run must succeed: {e}"));
        (governor.events(), governor.spent())
    };
    assert_eq!(build(0), build(7_777), "wall-clock start never leaks");
}

#[test]
fn rate_limit_probe_is_passthrough() {
    // `rate_limit()` is a read of the adapter's budget snapshot: not
    // paced, not budgeted, not retried — the §11.4 read-only accessor.
    let forge = MemoryForge::new();
    let (clock, sleeper, _script) = scripted_time(0);
    let policy = GovernorPolicy {
        max_calls: Some(0),
        ..GovernorPolicy::default()
    };
    let governor = Governor::new(&forge, policy, clock, sleeper);
    assert_eq!(governor.rate_limit(), RateInfo::MODEL_DEFAULT);
    assert_eq!(governor.spent(), 0, "the read is not spend");
    assert_eq!(governor.events(), Vec::new());
}

/// The error side of a `Result` the test asserts is an error; keeps the
/// house style (no `unwrap`) while staying readable.
trait UnwrapErrRefused<T> {
    fn unwrap_err_or_else_refused(self) -> ForgeError;
}

impl<T: std::fmt::Debug> UnwrapErrRefused<T> for Result<T, ForgeError> {
    fn unwrap_err_or_else_refused(self) -> ForgeError {
        match self {
            Ok(value) => unreachable!("expected a refusal, got: {value:?}"),
            Err(error) => error,
        }
    }
}
