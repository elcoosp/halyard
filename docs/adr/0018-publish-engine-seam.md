# ADR-0018: the publish engine seam (M5.2 step 1, §13)

- **Status:** accepted
- **Date:** 2026-11-14
- **Milestone:** M5.2 step 1 — the `halyard-publish` crate: plan
  validation + topological layers (§13 step 1), the resume loop over the
  `Registry` seam (§13 step 2), the `PublishEffect` tool seam and the
  §13 step-3 visibility wait, with the §15.2 failure-injection matrix;
  the real tool spawn joins with the CLI shell (step 2)
- **Deciders:** autonomous implementation agent (§13.1's "layers",
  §13.7's failure model and the gating semantics were underdetermined;
  settled per §0 rule 3 against the existing seam conventions — no new
  external facts required, no VERIFY items opened)

## Context

§13 specifies the publish engine as five properties: topological order
from the dependency graph (step 1), idempotent resume through registry
existence probes (step 2), the visibility wait before dependents publish
(step 3), the tool spawn as an effect the engine only *orchestrates*
(step 4), and the per-package outcome model with Sampo-style continue-
from-the-failure re-runs (§13.7). §17 M5.2 names its acceptance:
"failure-injection after each package; no duplicates". PROGRESS.md's
declared start point after the M5.1 registry slice was exactly this:
"the M5.2 publisher over the `Registry` seam (topo layers, resume,
visibility wait — all testable against `MemoryTransport`)".

The M5.1 seam covers steps 2–3's *reads*; what was missing is the
orchestration: who validates the plan (I8), who computes the layers,
what happens when a package fails mid-run, and where the spawn lives.

Four decisions had to be settled:

1. **Crate placement.** §7.1 reserved `halyard-publish/` from the start
   ("topological publisher, retry/resume, provenance"); the registry
   crate's layer note explicitly deferred "the workspace vocabulary
   joins with the M5.2 publisher" — a new crate consuming both `core`
   (the `Plan`/`Workspace` vocabulary) and `registry` (the probes) is
   the pre-agreed shape, not a new one.
2. **Where the spawn lives.** §13.4 shells out to `cargo publish` /
   `pnpm publish`, but the engine must stay offline-testable (I5) and
   the house effect-seam pattern (`TagWriter`, `TagPusher`,
   `HttpTransport`, `refresh_lockfile_native`) always injects the
   effect and scripts it in tests. The spawn is an injected
   `PublishEffect`; the *real* spawn (argv tables per ecosystem, the
   §16.2 `publish` command) belongs to the shell, like every other
   live effect.
3. **Failure semantics.** §13.7: each package outcome ∈ `{Published,
   AlreadyPublished, Skipped, Failed{retryable}}` and "a rerun after
   failure of package *k* continues from *k*". A tool failure must not
   abort the run — but it must also not let *dependents* publish: a
   released artifact referencing a version the registry does not carry
   is a broken release. Gating (failed/invisible/skipped upstream ⇒
   skipped dependents) is the honest middle.
4. **Thread safety.** §13.1: "packages within a layer publish
   **concurrently** (bounded)". Shared `&dyn` across scoped threads
   requires `Send + Sync` on the injected seams.

## Decision

1. **New crate `halyard-publish`** with the layer rule `publish →
   [core, registry]` (§7.2; `registry → forge` already holds, so the
   transport vocabulary arrives transitively). No external deps: the
   engine speaks wire-format version strings (the registries' grammar
   is the registries'), so even `semver` stays out of the runtime
   graph (`criterion`/`semver` are dev-only).
2. **`plan_publication(&Plan, &Workspace)`** — the pure §13 step 1:
   recompute the plan hash and refuse a mismatch (`E-PUB-PLAN`, I8 —
   publish consumes a verified plan only), join publish-enabled
   releases with the workspace (unknown package / unpublishable member
   / target outside `plan.order` / duplicate release names ⇒
   `E-PUB-PLAN`), layer the targets over `WorkspaceGraph`'s non-dev
   edges (dependencies first, name-sorted — I3), refuse a cycle among
   targets with the rendered path (`E-PUB-ORDER`; dev-only cycles stay
   legal per §6.6), and report the not-targets (`publish_disabled`,
   `no_publish_contract`) for §16.3 observability.
3. **`run_publication`** — the resume loop (§13 steps 2–3): registry
   routing resolved **before** any call (`RegistryRouter`; a missing
   client is `E-PUB-REGISTRY` with zero side effects — validate-then-
   act), one resume probe per target (`AlreadyPublished` = the I1
   no-op), the injected `PublishEffect` for the spawn, then the §13
   step-3 visibility wait (`wait_until_visible`, the ADR-0017
   deterministic backoff). Outcomes are rows, never run aborts
   (§13.7); dependents of a `Failed` / invisible / skipped upstream
   are skipped with the deterministic reason
   (`upstream_failed:` / `upstream_invisible:` / `upstream_skipped:`,
   names sorted, comma-joined). Counters are derived from rows (the
   house tallies convention).
4. **Bounded within-layer parallelism** — `PublishPolicy::max_parallel`
   (default `1`, `0` normalizes to `1`): one layer's targets are
   independent by construction, so scheduling order cannot change
   outcomes; the report stays layer-ordered and name-sorted (I3).
   Scoped `std::thread`s (§6.2's "scoped std threads until budgets
   demand more").
5. **`Send + Sync` on the injected seams** — `PublishEffect` and
   `RegistryRouter` carry the supertraits; `halyard_registry`'s
   `Registry` and `RegistrySleeper` gained them (a widening, not a
   change: every shipped impl already satisfied the bound — the
   clients hold `Arc<dyn HttpTransport>`, which is `Send + Sync`).
   Recorded here because it widens another crate's trait contract.
6. **The error family** — `E-PUB-PLAN`, `E-PUB-ORDER`,
   `E-PUB-REGISTRY`, `E-PUB-EFFECT` (the §15.4-reserved `E-PUB-*`
   family's first pages). Row-level failures carry the *underlying*
   stable code (`E-PUB-EFFECT` or the probe's own `E-REG-*`), never a
   wrapper's; the run-level `E-PUB-REGISTRY` is reserved for the
   routing gap.
7. **The reference fakes ship with the seam** (`MemoryRegistry`,
   `MemoryPublishEffect`, `MapRouter`) — the `MemoryForge`/
   `MemoryTransport` convention: the scripted harness is part of the
   seam's public API, so downstream tests (the CLI shell's, M5.3's)
   script the same behavior without a new test-support rule.

## Consequences

- The §17 M5.2 acceptance's "failure-injection after each package; no
  duplicates" is demonstrated offline: `tests/publish.rs` injects a
  failure at every step of a chain and asserts the re-run converges to
  the uninterrupted state with exactly one successful publish per
  package (the `successes()` log is the zero-duplicates witness).
- The real spawn (`cargo publish`/`pnpm publish` argv tables, the
  PATH-shim-tested spawn, the §16.2 `publish [--dry]` command with
  json/human/gha renders) is step 2 — the shell work, exactly like the
  tag phase's forecast → live split (ADR-0012).
- OIDC trusted publishing, provenance and secret scrubbing stay M5.3+
  (their own VERIFY research per §0.4); the engine's `PublishRequest`
  vocabulary is spawn-agnostic and needs no change.
- The engine adds ≈200 µs of pure layering and ≈84 µs of orchestration
  per 40-target plan (`benches/publish.rs`) — noise against one
  registry round trip; no §14.1 budget is touched.
