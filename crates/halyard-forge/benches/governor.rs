//! The §11.5 call governor benches (M5.5): the decorator overhead on the
//! machine's update path (passive policy — the wrapper must be free),
//! the pacing decision at the shell's default spacing, and the budget
//! ledger's refusal path.
//!
//! Run: `cargo bench -p halyard-forge`

use std::fmt::Write as _;
use std::sync::Arc;

use criterion::Criterion;
use halyard_forge::governor::{Governor, GovernorClock, GovernorPolicy, GovernorSleeper};
use halyard_forge::{
    BranchName, Forge, ForgeError, MachineInput, MemoryForge, PrSpec, ReleaseSpec,
    run_release_machine,
};

/// Branch fixture shared by the bench groups.
fn main_branch() -> BranchName {
    BranchName::new("halyard/release/main").unwrap_or_else(|e| unreachable!("valid branch: {e}"))
}

/// PR spec fixture: a plausible Release-PR body size (~1 KiB).
fn pr_spec() -> PrSpec {
    let mut body = String::with_capacity(1024);
    body.push_str("## 🚀 Release plan\n\n| Package | Current | Next | Bump | Why |\n");
    for i in 0..12 {
        let _ = writeln!(body, "| `pkg-{i}` | 0.1.{i} | 0.2.0 | minor | `feat: …` |");
    }
    PrSpec {
        branch: main_branch(),
        title: "chore: release v0.2.0".to_owned(),
        body,
    }
}

/// The governed update path over a passive policy: the machine's find +
/// upsert with the wrapper in between — the decorator overhead the shell
/// accepts on every live run.
fn bench_governor_passthrough(c: &mut Criterion) {
    c.bench_function("governor/passthrough_update_path", |b| {
        b.iter_with_large_drop(|| {
            let forge = MemoryForge::new();
            let policy = GovernorPolicy::default();
            let (clock, sleeper) = halyard_forge::governor::system_time();
            let governor = Governor::new(&forge, policy, clock, sleeper);
            let report = run_release_machine(
                &governor,
                &MachineInput {
                    branch: main_branch(),
                    pending_releases: Vec::new(),
                    release_pr: Some(pr_spec()),
                },
            )
            .unwrap_or_else(|e| unreachable!("update path must succeed: {e}"));
            assert_eq!(report.calls, 2, "find + upsert (§11.5 budget)");
        });
    });
}

/// The pacing decision: 16 content calls at the shell's one-second
/// policy, over a frozen scripted clock and a no-op sleeper — the
/// decision + event log cost per paced call. The waits themselves are
/// the point, not the cost; the schedule is asserted by the test suite.
fn bench_governor_pacing(c: &mut Criterion) {
    c.bench_function("governor/pacing_decision_16", |b| {
        b.iter(|| {
            let forge = MemoryForge::new();
            let policy = GovernorPolicy {
                min_spacing_ms: 1_000,
                ..GovernorPolicy::default()
            };
            // The scripted pair never advances the clock and never
            // sleeps: the pacing decision re-fires deterministically on
            // every call, and the bench measures exactly that decision.
            let clock: GovernorClock = Arc::new(|| 0_u64);
            let sleeper: GovernorSleeper = Arc::new(|_ms| {});
            let governor = Governor::new(&forge, policy, clock, sleeper);
            for i in 0..16 {
                let spec = ReleaseSpec {
                    tag: format!("pkg-{i}-v0.1.0"),
                    notes: "n".to_owned(),
                };
                governor
                    .create_release(&spec)
                    .unwrap_or_else(|e| unreachable!("paced call must succeed: {e}"));
            }
            assert_eq!(governor.events().len(), 15, "first call unpaced");
        });
    });
}

/// The budget ledger: the refusal path must cost the same order as the
/// forward path (fail-closed, not fail-slow) — one under-budget probe,
/// then the blocked upsert.
fn bench_governor_budget(c: &mut Criterion) {
    c.bench_function("governor/budget_block", |b| {
        b.iter(|| {
            let forge = MemoryForge::new();
            let policy = GovernorPolicy {
                max_calls: Some(1),
                ..GovernorPolicy::default()
            };
            let (clock, sleeper) = halyard_forge::governor::system_time();
            let governor = Governor::new(&forge, policy, clock, sleeper);
            governor
                .find_release_pr(&main_branch())
                .unwrap_or_else(|e| unreachable!("first call is under budget: {e}"));
            let refused = governor.upsert_pr(&pr_spec()).unwrap_err_or_else_refused();
            assert!(
                matches!(refused, ForgeError::Budget { .. }),
                "the second call is refused"
            );
        });
    });
}

/// The error side of a `Result` the bench asserts is an error (the
/// house style: no `unwrap`).
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

/// Criterion entry point (hand-written instead of `criterion_group!` so the
/// generated function keeps its doc comment under `missing_docs`).
fn main() {
    let mut c = Criterion::default().configure_from_args();
    bench_governor_passthrough(&mut c);
    bench_governor_pacing(&mut c);
    bench_governor_budget(&mut c);
}
