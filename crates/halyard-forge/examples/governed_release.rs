//! The §11.5 call governor consumer demo (M5.5, ADR-0015): one §11.1
//! machine pass over a *governed* [`MemoryForge`] — content-call pacing
//! visible in the event log, an injected `E-FORGE-RATE-LIMIT` honored
//! through the suggested `Retry-After`, the pass aborting fail-closed
//! when the retries run out — and then the I2 re-run that reaches the
//! uninterrupted end state over the same governor.
//!
//! The demo uses a scripted clock/sleeper (the ADR-0014 convention), so
//! it is deterministic and instant: every wait the governor requests is
//! printed as data instead of slept.
//!
//! Run: `cargo run -p halyard-forge --example governed_release`

use std::sync::Arc;
use std::sync::Mutex;

use halyard_forge::governor::{Governor, GovernorClock, GovernorPolicy, GovernorSleeper};
use halyard_forge::{
    BranchName, Forge, ForgeError, MachineInput, MemoryForge, PrSpec, ReleaseSpec,
    run_release_machine,
};

/// The scripted time state the clock reads and the sleeper advances.
#[derive(Default)]
struct Script {
    now_ms: u64,
    sleeps: Vec<u64>,
}

/// Builds the injected time pair: the clock reads `script.now_ms`, the
/// sleeper records the requested wait and advances the clock.
fn scripted_time() -> (GovernorClock, GovernorSleeper, Arc<Mutex<Script>>) {
    let script = Arc::new(Mutex::new(Script::default()));
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let forge = MemoryForge::new();
    let (clock, sleeper, script) = scripted_time();
    // The §11.5 production shape: one-second content-call pacing, two
    // retries (a third attempt total), deterministic capped backoff.
    let policy = GovernorPolicy {
        max_retries: 2,
        ..GovernorPolicy::shell_default()
    };
    let governor = Governor::new(&forge, policy, clock, sleeper);

    // Pass 1 — the forge is throttling: every call fails with a
    // Retry-After of 10 s, so the governor honors the wait twice, still
    // sees the limit, and aborts the pass fail-closed with the
    // adapter's own error verbatim (§15.4).
    forge.set_failure(Some(ForgeError::rate_limited(10)));
    let branch = BranchName::new("halyard/release/main")?;
    let input = MachineInput {
        branch: branch.clone(),
        pending_releases: vec![ReleaseSpec {
            tag: "core-v0.5.0".to_owned(),
            notes: "## Features\n\n- the call governor".to_owned(),
        }],
        release_pr: None,
    };
    let first = run_release_machine(&governor, &input);
    match first {
        Ok(report) => unreachable!("the throttled pass must abort, got: {report:?}"),
        Err(error) => println!("pass 1 (throttled): {error}"),
    }
    let waits = script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .sleeps
        .clone();
    println!("pass 1 waits: {waits:?} ms");
    println!("pass 1 events: {:#?}", governor.events());
    println!("pass 1 spend: {} forwarded attempts", governor.spent());

    // Pass 2 — the throttle lifted (failpoint disarmed): the same
    // governor re-runs the pass to completion — one release created,
    // zero duplicates (I2) — and the event log carries the whole run's
    // §11.5 observability surface.
    forge.set_failure(None);
    let second = run_release_machine(&governor, &input)?;
    println!("\npass 2 (healed): {second:#?}");
    println!("pass 2 events: {:#?}", governor.events());
    println!(
        "pass 2 forge ledger: {} create_release calls (attempts + the created one)",
        forge.call_count("create_release")
    );

    // Pass 3 — the prepare phase over the same governed forge, now with
    // pacing visible: the Release PR upsert is a content call, so it
    // spaces itself after the tag phase's last content call.
    let prepare = MachineInput {
        branch,
        pending_releases: Vec::new(),
        release_pr: Some(PrSpec {
            branch: BranchName::new("halyard/release/main")?,
            title: "chore: release v0.5.0".to_owned(),
            body: "## 🚀 Release plan".to_owned(),
        }),
    };
    let third = run_release_machine(&governor, &prepare)?;
    println!("\npass 3 (prepare): {third:#?}");
    println!("pass 3 events: {:#?}", governor.events());
    let waits = script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .sleeps
        .clone();
    println!("total waits: {waits:?} ms");
    Ok(())
}
