# ADR-0030: machine-enforced feature exclusion + the `ci-bundle` gate feature

- **Status:** accepted
- **Date:** 2026-11-16
- **Milestone:** the flagged follow-up from the change-file and
  channels sessions ("documented-exclusive but not machine-enforced, a
  candidate xtask check, not landed")
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

`halyard-cache` carries two test-lane features, `miri` and `loom`,
whose manifests and module docs have said "never combined in one build"
since M2.5: the `miri` feature's shim switches `Mmap::open` to return
`None`, so a build with both enabled silently exercises the crate
through the `read()`-backed fallback — the real-`mmap` coverage the
default lane provides would be *masked*, not merged. The contract was
comment-only. Meanwhile every CI gate ran `--all-features`, which
combines the pair in every PR build, and the loom suite rode those
lanes with no spelled home of its own. Three sessions flagged the
inconsistency; nothing enforced the contract.

Constraints that shaped the design:

1. **A comment is not a contract.** The exclusion guard is a
   `compile_error!` behind `#[cfg(all(feature = "miri",
   feature = "loom"))]` in the one crate that owns the pair — a build
   that expresses the conflict fails loudly with the fix in the
   message, instead of silently masking coverage.
2. **The gates need a conflict-free spelling.** With the guard in
   place, `--all-features` is no longer a valid gate invocation, so the
   "all stable features" set needs a name: `ci-bundle`. Every
   feature-carrying crate contributes its stable features to its own
   `ci-bundle` (`halyard-core`/`halyard-config` map it to
   `schema-gen`; `halyard-cache`'s is deliberately empty) and keeps its
   manual/nightly lanes out. Workspace-wide `--features ci-bundle`
   enables the feature on the members that declare it and skips the
   rest — one spelling for every gate.
3. **Enforcement must not cost coverage.** The loom suite previously
   compiled only because `--all-features` switched it on as a side
   effect. The CI gains an explicit loom job running the suite's own
   documented invocation (`cargo test -p halyard-cache --features loom
   --release`), so the model check runs in a spelled lane instead of as
   a feature-flag accident. The Miri lane already exists in the
   nightly workflow (the interpreter defines `cfg(miri)`; no feature
   needed).

## Decision

- `halyard-cache`: the `compile_error!` guard; the manifest feature
  notes updated to say the exclusion is machine-enforced.
- `halyard-core`, `halyard-config`, `halyard-cache`: the `ci-bundle`
  feature as the CI gate set (stable features in; the exclusive pair
  out).
- CI: the clippy, test, nextest and doc jobs run `--features
  ci-bundle`; a new `loom` job runs the documented loom invocation on
  stable.

## Consequences

- **The documented contract is now a compile error.** Any build —
  local or CI, intentional or accidental — that combines the pair
  fails with the two documented invocations in the message.
- **`--all-features` is retired as a gate spelling.** The gates say
  what they mean: `ci-bundle` names the stable set; the exclusive
  lanes are spelled where they run. Gate output is no longer
  ambiguous about which feature combination produced it.
- **The loom suite gains a real lane.** It ran as a side effect of
  `--all-features` before; now it is a named job with its documented
  command — the coverage is visible in the job list instead of hidden
  in a feature matrix.
- **A future feature-carrying crate inherits the rule.** Its manifest
  declares a `ci-bundle` (stable features) and keeps its own
  exclusive/manual lanes out — the bundle stays the one spelling the
  gates need.

## Alternatives considered

- **An `xtask feature-guard` lint.** Rejected: it inspects manifests
  at CI time but does not stop a local combined build, and the
  compile_error travels with the crate it guards.
- **Keep `--all-features` and drop the exclusion claim.** Rejected:
  the manifest's "never combined" wording encodes a real masking
  hazard (the miri shim), not stylistic preference.
- **Give `miri`/`loom` a mutual-exclusion mechanism in cargo.** There
  is none (features are additive by design); the guard is the
  established idiom.
