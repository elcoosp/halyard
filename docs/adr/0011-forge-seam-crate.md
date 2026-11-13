# ADR-0011: the Forge seam crate and the `MemoryForge` placement

- **Status:** accepted
- **Date:** 2026-11-13
- **Milestone:** M4.1a (forge seam; the §17 M4.1 slice split as-built)
- **Deciders:** autonomous implementation agent (deviation from the §7.1
  one-line layout note, documented here per §0 rule 3)

## Context

§11.4 fixes the `Forge` trait and §11.1 the Release-PR state machine that
consumes it. The M4.1 slice as written bundles the trait with the GitHub
REST adapter (ETag conditionals, backoff, App-JWT auth, wiremock suites).
Shipping the trait *only together with* the HTTP adapter leaves the
state-machine work (M4.2) without a consumable seam, and leaves the §15.1
E2E lane ("mock forge server + real git") without its mock. Separately,
the trait's own failure taxonomy (`E-FORGE-*`, §15.4) and observability
surface (the §11.5 call budget) can be designed and pinned without any
network code.

§7.1 sketches `halyard-test-support/` as carrying the "mock forge". The
mock must implement the `Forge` trait; if the trait lives in a seam crate
and the reference impl in test-support, the seam crate ships with *no*
public implementation at all — every test of the trait's own contract
would have to grow a private harness.

## Decision

1. New crate **`crates/halyard-forge/`**: the [`Forge`] trait, its data
   vocabulary (`Pr`, `PrSpec`, `Release`, `ReleaseSpec`, `ApiCommit`,
   `ApiFile`, `BranchName`, `RateInfo`) and the `E-FORGE-*` taxonomy.
   Layer rule: `halyard-forge → halyard-core` only (§7.2: "forge …
   depend on core only" — the layout's own rule already places it).
2. The **`MemoryForge`** deterministic reference adapter lives in
   `halyard-forge` next to the trait: it pins the idempotence semantics
   (find-by-branch upsert, GET-by-tag-first release creation, synthetic
   `fe`-prefixed commit ids) in the same crate where the contract is
   defined, and it carries the two observability surfaces — the
   per-adapter call log (trait method `Forge::calls`, the §11.5
   "≤ 2 calls on the update path" assertion surface) and the failpoint
   harness (`MemoryForge::set_failure`, the §15.2 failure injector for
   the M4.2 matrix).
3. `halyard-test-support` **re-exports** `Forge`, `MemoryForge` and the
   spec types as the §7.1 "mock forge" home — E2E suites import git
   fixtures and the forge from one crate — but the definition and its
   contract tests stay in the seam crate.
4. The M4.1 split as-built: **M4.1a** (this slice) = seam + reference
   adapter + taxonomy + docs; **M4.1b** (remaining) = the GitHub REST
   adapter (ETag, backoff, App-JWT, wiremock suites, ≤ 2-call budget
   asserted over real HTTP). The nightly `forge-smoke` job stays latent
   until M4.1b lands.

## Consequences

- The trait is object-safe and consumed as `&dyn Forge`; the state
  machine (M4.2) can be developed against `MemoryForge` end-to-end and
  swapped to the REST adapter without signature churn.
- The `E-FORGE-*` pages exist before the first real adapter, so M4.1b
  inherits a fixed error vocabulary (no retrofitting).
- `Forge::commit_via_api` ships with a default honest decline
  (`E-FORGE-UNSUPPORTED`), mirroring `refresh_lockfile_native`'s default
  (§7.3) — an adapter without the capability names itself instead of
  lying.
- The layer table (`xtask/src/layers.rs`) gains `halyard-forge → core`
  and widens `halyard-test-support` to `core + forge`; both are enforced
  by `xtask check-layers`.

## Alternatives considered

1. **Reference impl in `halyard-test-support`** — rejected: the trait's
   own contract tests would need a private harness; the mock's semantics
   (idempotence, call budget) are contract, not fixture convenience.
2. **Trait inside `halyard-core`** — rejected: core is the §7.2-pinned
   pure planning crate; a forge contract is shell vocabulary (I9 says
   GitHub is an adapter, not that the core grows one).
3. **Ship the trait + REST adapter together (full M4.1 in one slice)** —
   deferred honestly: the adapter's auth/ETag/backoff surface needs
   `VERIFY`-tagged research (§0 rule 4) and a wiremock suite; bundling it
   would either stall the seam or invite a stub.
