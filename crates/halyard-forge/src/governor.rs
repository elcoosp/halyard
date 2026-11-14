//! The §11.5 call governor (M5.5): the [`Forge`] decorator that
//! enforces the secondary-rate-limit contract the `E-FORGE-RATE-LIMIT`
//! page promises — content-creating calls are serialized with pacing,
//! `Retry-After` is honored exactly, transport failures back off, and a
//! per-run call budget fails closed (`E-FORGE-BUDGET`) instead of
//! spending an unbounded run.
//!
//! The governor composes with everything already built on the seam: it
//! *is* a [`Forge`], so the §11.1 state machine, the shell's live
//! commands and any other consumer run over it unchanged, and
//! [`Forge::calls`] keeps delegating to the inner adapter so the
//! machine's §11.5 call accounting never notices the wrapper.
//!
//! Time never enters the seam types: the clock and the sleeper are
//! injected ([`GovernorClock`]/[`GovernorSleeper`], the ADR-0014
//! convention), production uses [`system_time`], and tests script both —
//! every wait schedule is asserted as data (I3).
//!
//! Pinned semantics (the contract the test suite holds):
//!
//! * **Pacing** spaces *content-creating* calls (`upsert_pr`,
//!   `create_release`, `close_pr`, `commit_via_api`) start-to-start at
//!   [`GovernorPolicy::min_spacing_ms`]; probes (`find_release_pr`,
//!   `release_exists`) and the [`Forge::rate_limit`] read are unpaced —
//!   §11.5 paces content creation, not reads.
//! * **Retry-After** ([`ForgeError::RateLimited`]) is honored exactly:
//!   the suggested wait in seconds, once per retry attempt.
//! * **Backoff** ([`ForgeError::Transport`]) is a pure function of the
//!   attempt — `backoff_base_ms × 2^attempt`, capped at
//!   `backoff_cap_ms`. Deliberately *no jitter*: the schedule must stay
//!   byte-predictable (I3 outranks the seed's "with jitter" sketch; see
//!   ADR-0015).
//! * **Budget** ([`GovernorPolicy::max_calls`]) is a ledger of forwarded
//!   attempts, not of successes — an injected failure or a retry is
//!   observable spend (§11.5: "a dropped call is an observable one"),
//!   and the refusal happens *before* the call reaches the forge.
//! * **Non-retryable errors** (`Auth`, `Conflict`, `Request`,
//!   `Unsupported`, `Budget`) pass through verbatim with no wait and no
//!   event; exhausted retries return the last error verbatim — the
//!   §15.4 code is never swallowed.
//!
//! ```no_run
//! use halyard_forge::governor::{Governor, GovernorPolicy, system_time};
//! use halyard_forge::{BranchName, Forge, MemoryForge, PrSpec};
//!
//! let forge = MemoryForge::new();
//! let policy = GovernorPolicy {
//!     min_spacing_ms: 1_000,
//!     max_retries: 3,
//!     ..GovernorPolicy::default()
//! };
//! let (clock, sleeper) = system_time();
//! let governor = Governor::new(&forge, policy, clock, sleeper);
//! let branch = BranchName::new("halyard/release/main").unwrap();
//! let pr = governor.upsert_pr(&PrSpec {
//!     branch,
//!     title: "chore: release v0.1.0".to_owned(),
//!     body: "## 🚀 Release plan".to_owned(),
//! }).unwrap();
//! // The §11.5 observability surface: what was waited, what was spent.
//! assert_eq!(governor.spent(), 1);
//! assert!(governor.events().is_empty(), "the first content call is unpaced");
//! ```

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;
use std::thread;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use crate::Forge;
use crate::error::ForgeError;
use crate::memory::CallRecord;
use crate::spec::{ApiCommit, BranchName, Pr, PrSpec, RateInfo, Release, ReleaseSpec};

/// Milliseconds since the Unix epoch — the injected governor clock
/// (ADR-0014: no system-time read inside the seam; tests script this).
pub type GovernorClock = Arc<dyn Fn() -> u64 + Send + Sync>;

/// Waits the given number of milliseconds — the injected governor
/// sleeper. Production sleeps the thread; tests record the request and
/// advance their scripted clock, so a wait schedule is assertable data.
pub type GovernorSleeper = Arc<dyn Fn(u64) + Send + Sync>;

/// The production time pair: the system wall clock (milliseconds since
/// the epoch) and a real thread sleep.
pub fn system_time() -> (GovernorClock, GovernorSleeper) {
    let clock: GovernorClock = Arc::new(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| {
                u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
            })
    });
    let sleeper: GovernorSleeper = Arc::new(|ms| thread::sleep(Duration::from_millis(ms)));
    (clock, sleeper)
}

/// The governor's policy (§11.5): pacing, budget and retry shape. All
/// fields are plain data — the policy is a value the caller constructs,
/// never read from the environment inside this crate (the shell's env
/// grammar is the config layer, §16.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GovernorPolicy {
    /// Minimum start-to-start spacing between content-creating calls,
    /// in milliseconds (§11.5 pacing). `0` disables pacing.
    pub min_spacing_ms: u64,
    /// The per-run call budget: the maximum number of forwarded
    /// attempts before the governor refuses with `E-FORGE-BUDGET`.
    /// Injected failures and retries count (observable spend). `None`
    /// is unbounded.
    pub max_calls: Option<u32>,
    /// Retry attempts per operation after the initial one (`0` = a
    /// single attempt, fail on the first error).
    pub max_retries: u32,
    /// Transport backoff base, in milliseconds: the first transport
    /// retry waits `backoff_base_ms`, the next `2 × backoff_base_ms`,
    /// and so on (deterministic, capped — no jitter, ADR-0015).
    pub backoff_base_ms: u64,
    /// Upper bound of one transport backoff wait, in milliseconds.
    pub backoff_cap_ms: u64,
}

impl Default for GovernorPolicy {
    /// The passive policy: no pacing, no budget, no retries. The
    /// decorator is then transparent — every schedule decision is "no
    /// wait", and the wrapper exists for its observability surface only.
    fn default() -> Self {
        Self {
            min_spacing_ms: 0,
            max_calls: None,
            max_retries: 0,
            backoff_base_ms: 1_000,
            backoff_cap_ms: 30_000,
        }
    }
}

impl GovernorPolicy {
    /// The §11.5 production shape the shell defaults to: one-second
    /// pacing between content-creating calls, three retries with a
    /// deterministic capped backoff, and no call budget (the ≤ 2-calls
    /// update path is enforced by design, not by a cap — arming a budget
    /// is an operator decision via the shell's env knobs).
    pub fn shell_default() -> Self {
        Self {
            min_spacing_ms: 1_000,
            max_calls: None,
            max_retries: 3,
            backoff_base_ms: 1_000,
            backoff_cap_ms: 30_000,
        }
    }
}

/// One governor decision or wait, oldest first in
/// [`Governor::events`]. Deterministic data — the log is comparable
/// across runs (I3), which is what makes the shell's end-of-run usage
/// summary (§11.5) a stable render.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GovernorEvent {
    /// A content-creating call waited `waited_ms` to honor the pacing
    /// spacing before being forwarded.
    Paced {
        /// The operation that was paced (`"create_release"`, …).
        op: &'static str,
        /// The wait the pacing decision produced, in milliseconds.
        waited_ms: u64,
    },
    /// A `Retry-After` rate limit was honored: attempt `attempt` failed
    /// with `E-FORGE-RATE-LIMIT` and the retry waited `waited_ms`.
    RateLimitRetry {
        /// The retried operation.
        op: &'static str,
        /// Zero-based index of the failed attempt.
        attempt: u32,
        /// The honored `Retry-After` wait, in milliseconds.
        waited_ms: u64,
    },
    /// A transport failure was retried with deterministic backoff:
    /// attempt `attempt` failed with `E-FORGE-TRANSPORT` and the retry
    /// waited `waited_ms`.
    TransportRetry {
        /// The retried operation.
        op: &'static str,
        /// Zero-based index of the failed attempt.
        attempt: u32,
        /// The backoff wait, in milliseconds.
        waited_ms: u64,
    },
    /// The per-run call budget refused the operation before it reached
    /// the forge (`E-FORGE-BUDGET`, fail-closed).
    BudgetBlocked {
        /// The refused operation.
        op: &'static str,
        /// The spend already on the ledger when the call was refused.
        spent: u32,
    },
}

#[derive(Debug, Default)]
struct GovernorState {
    last_content_ms: Option<u64>,
    events: Vec<GovernorEvent>,
    spent: u32,
}

/// The §11.5 call governor: a [`Forge`] decorator wrapping any adapter.
/// Construct with [`Governor::new`]; hand the governor itself to the
/// §11.1 machine or any other [`Forge`] consumer.
pub struct Governor<'a> {
    forge: &'a dyn Forge,
    policy: GovernorPolicy,
    clock: GovernorClock,
    sleeper: GovernorSleeper,
    state: Mutex<GovernorState>,
}

impl<'a> Governor<'a> {
    /// Wraps `forge` under `policy`, reading time through `clock` and
    /// waiting through `sleeper`.
    pub fn new(
        forge: &'a dyn Forge,
        policy: GovernorPolicy,
        clock: GovernorClock,
        sleeper: GovernorSleeper,
    ) -> Self {
        Self {
            forge,
            policy,
            clock,
            sleeper,
            state: Mutex::new(GovernorState::default()),
        }
    }

    /// The governor's event log, oldest first — the §11.5 end-of-run
    /// usage summary's input (pacing waits, honored retries, budget
    /// refusals). Deterministic (I3).
    pub fn events(&self) -> Vec<GovernorEvent> {
        self.state().events.clone()
    }

    /// Forwarded attempts so far, injected failures and retries
    /// included — the budget ledger (§11.5: a dropped call is an
    /// observable one). Every forwarded attempt also lands in the inner
    /// adapter's own [`Forge::calls`] log, so the machine's accounting
    /// and this ledger reconcile.
    pub fn spent(&self) -> u32 {
        self.state().spent
    }

    /// The policy the governor enforces (the shell's usage summary
    /// reports it next to the events).
    pub fn policy(&self) -> &GovernorPolicy {
        &self.policy
    }

    /// Locks the state; poisoning cannot corrupt the invariants (every
    /// mutation is a total replace or append), so the guard is recovered
    /// and the panic propagates from the *original* site (the
    /// [`MemoryForge`](crate::MemoryForge) convention).
    fn state(&self) -> MutexGuard<'_, GovernorState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Spaces a content-creating call start-to-start at
    /// `min_spacing_ms`: waits the gap between the previous content
    /// call's start and now, then records this call's start. Probes and
    /// `rate_limit()` never pace.
    fn pace(&self, op: &'static str) {
        let spacing = self.policy.min_spacing_ms;
        if spacing == 0 {
            return;
        }
        let wait = {
            let state = self.state();
            let now = (self.clock)();
            match state.last_content_ms {
                Some(last) => last.saturating_add(spacing).saturating_sub(now),
                None => 0,
            }
        };
        if wait > 0 {
            (self.sleeper)(wait);
            self.state().events.push(GovernorEvent::Paced {
                op,
                waited_ms: wait,
            });
        }
        let started = (self.clock)();
        self.state().last_content_ms = Some(started);
    }

    /// One transport backoff wait: `backoff_base_ms × 2^attempt`,
    /// capped at `backoff_cap_ms` (deterministic — no jitter, ADR-0015).
    fn backoff_ms(&self, attempt: u32) -> u64 {
        let doubling = 1_u64.checked_shl(attempt.min(62)).unwrap_or(1);
        let backoff = self.policy.backoff_base_ms.saturating_mul(doubling);
        backoff.min(self.policy.backoff_cap_ms)
    }

    /// The per-operation pipeline: budget check → attempt loop. The
    /// budget is re-checked per attempt (retries spend too), every
    /// attempt is forwarded to the inner adapter (observable spend), and
    /// retryable errors sleep through the injected sleeper exactly as
    /// the event log records.
    fn forward<T>(
        &self,
        op: &'static str,
        content: bool,
        call: impl Fn() -> Result<T, ForgeError>,
    ) -> Result<T, ForgeError> {
        if content {
            self.pace(op);
        }
        let mut attempt = 0_u32;
        loop {
            {
                let mut state = self.state();
                if let Some(budget) = self.policy.max_calls
                    && state.spent >= budget
                {
                    let spent = state.spent;
                    state
                        .events
                        .push(GovernorEvent::BudgetBlocked { op, spent });
                    return Err(ForgeError::budget(op, spent, budget));
                }
                state.spent = state.spent.saturating_add(1);
            }
            match call() {
                Ok(value) => return Ok(value),
                Err(ForgeError::RateLimited { retry_after_secs })
                    if attempt < self.policy.max_retries =>
                {
                    let waited = retry_after_secs.saturating_mul(1_000);
                    (self.sleeper)(waited);
                    self.state().events.push(GovernorEvent::RateLimitRetry {
                        op,
                        attempt,
                        waited_ms: waited,
                    });
                    attempt = attempt.saturating_add(1);
                }
                Err(ForgeError::Transport { .. }) if attempt < self.policy.max_retries => {
                    let waited = self.backoff_ms(attempt);
                    (self.sleeper)(waited);
                    self.state().events.push(GovernorEvent::TransportRetry {
                        op,
                        attempt,
                        waited_ms: waited,
                    });
                    attempt = attempt.saturating_add(1);
                }
                // Non-retryable, or the retry budget is exhausted: the
                // error passes through verbatim (the §15.4 code is never
                // swallowed into a success or a different failure).
                Err(error) => return Err(error),
            }
        }
    }
}

impl Forge for Governor<'_> {
    fn find_release_pr(&self, branch: &BranchName) -> Result<Option<Pr>, ForgeError> {
        self.forward("find_release_pr", false, || {
            self.forge.find_release_pr(branch)
        })
    }

    fn upsert_pr(&self, spec: &PrSpec) -> Result<Pr, ForgeError> {
        self.forward("upsert_pr", true, || self.forge.upsert_pr(spec))
    }

    fn release_exists(&self, tag: &str) -> Result<bool, ForgeError> {
        self.forward("release_exists", false, || self.forge.release_exists(tag))
    }

    fn create_release(&self, spec: &ReleaseSpec) -> Result<Release, ForgeError> {
        self.forward("create_release", true, || self.forge.create_release(spec))
    }

    fn close_pr(&self, branch: &BranchName) -> Result<bool, ForgeError> {
        self.forward("close_pr", true, || self.forge.close_pr(branch))
    }

    fn commit_via_api(&self, spec: &ApiCommit) -> Result<halyard_core::reason::Oid, ForgeError> {
        self.forward("commit_via_api", true, || self.forge.commit_via_api(spec))
    }

    fn rate_limit(&self) -> RateInfo {
        // A read of the adapter's budget snapshot: not paced, not
        // budgeted, not retried (§11.4 — the read-only accessor).
        self.forge.rate_limit()
    }

    fn calls(&self) -> Vec<CallRecord> {
        // The inner adapter's log is THE call ledger (§11.5): the
        // machine counts from it, so the decorator must stay invisible
        // here. The governor's own spend ledger is `spent()`.
        self.forge.calls()
    }
}
