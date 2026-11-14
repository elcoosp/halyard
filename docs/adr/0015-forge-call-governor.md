# ADR-0015: the forge call governor and its deterministic retry schedule

- **Status:** accepted
- **Date:** 2026-11-14
- **Milestone:** M5.5 (§11.5 "Secondary rate limits: serialize
  content-creating calls with pacing, honor `Retry-After`, exponential
  backoff with jitter, per-run call budget with an end-of-run API usage
  summary")
- **Deciders:** autonomous implementation agent (the deviation item —
  no jitter — is documented here per §0 rule 3)

## Context

The forge seam shipped with the §11.5 budget *accounted* but not
*enforced*: `MemoryForge` and the REST adapter keep the call log the
machine reports (`MachineReport.calls`), and the `E-FORGE-RATE-LIMIT`
page promises that "real adapters are expected to honour `Retry-After`,
pace content-creating calls, and keep the run under the per-run call
budget" — but no code does any of it. A throttled live run surfaces
`E-FORGE-RATE-LIMIT` and dies; nothing spaces content-creating calls;
nothing bounds a runaway run.

Enforcing this inside each adapter would duplicate the policy per
backend and leave the mock adapters unguarded. Enforcing it in the §11.1
machine would mix policy with the effect executor it reports on.

## Decision

1. **A decorator, not a rewrite.** `halyard_forge::governor::Governor`
   wraps any `&dyn Forge` and *is* a `Forge`: the §11.1 machine, the
   shell and every other consumer run over it unchanged.
   `Forge::calls` keeps delegating to the inner adapter, so the
   machine's §11.5 accounting and the ≤ 2-calls budget assertion are
   untouched by the wrapper.
2. **Injected time (the ADR-0014 convention).** The clock and sleeper
   are injected (`GovernorClock`/`GovernorSleeper`); the crate's types
   stay clock-free and the production pair is `governor::system_time()`
   (wall clock + `thread::sleep`) — the sanctioned effect, mirroring how
   `UreqTransport` is the sanctioned HTTP effect.
3. **Pacing is for content creation only.** `upsert_pr`,
   `create_release`, `close_pr` and `commit_via_api` are spaced
   start-to-start at `min_spacing_ms`; `find_release_pr`,
   `release_exists` and the `rate_limit()` read are unpaced (§11.5
   paces content creation, not reads).
4. **The budget is a ledger of forwarded attempts** — injected failures
   and retries count (§11.5: "a dropped call is an observable one") —
   and the refusal (`ForgeError::Budget`, `E-FORGE-BUDGET`, new docs
   page) happens *before* the call reaches the forge, re-checked per
   retry attempt.
5. **`Retry-After` is honored exactly** (`retry_after_secs × 1000 ms`
   per retry); **transport failures back off deterministically** —
   `backoff_base_ms × 2^attempt`, capped at `backoff_cap_ms`.
6. **No jitter, deliberately.** §11.5's sketch says "exponential backoff
   with jitter"; invariant I3 ("no randomness in any output path") and
   the testability bar the codebase holds (every schedule asserted as
   data in `tests/governor.rs`) win: the retry schedule is a pure
   function of the attempt number. Jitter is a thundering-herd
   mitigation for many concurrent clients; halyard's design goal is the
   opposite (≤ 2 calls on the update path), so the herd scenario this
   mitigates is the one §11.5's minimal-call design prevents.
7. **Non-retryable errors never wait**: `Auth`, `Conflict`, `Request`,
   `Unsupported` and `Budget` pass through verbatim; exhausted retries
   return the last error verbatim (the §15.4 code is never swallowed).

## Consequences

- The shell wraps its three live forge sites in the governor (M5.5
  wire-up) with `GovernorPolicy::shell_default()` and `HALYARD_FORGE_*`
  env knobs; the dry-run paths (`PreviewForge`) stay ungoverned — a
  forecast spends no calls.
- The event log (`Governor::events`) is deterministic, so the
  end-of-run API usage summary renders byte-stable JSON (I3).
- Retry behavior changes for live runs: a throttled command now waits
  and retries instead of failing on the first `E-FORGE-RATE-LIMIT`;
  the resumability contract (I2) is unchanged — the re-run after an
  exhausted retry budget reaches the uninterrupted end state with zero
  duplicates (pinned by the machine-composition test).
