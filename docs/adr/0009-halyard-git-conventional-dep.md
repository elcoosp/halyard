# ADR-0009: `halyard-git` depends on `halyard-conventional`

- **Status:** accepted
- **Date:** 2026-11-12
- **Milestone:** M1/M2 (slice C: M2.1/M2.2)
- **Deciders:** autonomous implementation agent (amendment to the
  slice-C layer plan "git → core (only)" → documented here per §0
  rule 3)

## Context

The slice-C protocol fixes the new crates' layer rules as `git → core
(only)`. The painter walk (§8.3) emits [`RelevantCommit`] rows, whose
`parsed`, `pr` and `trailers` fields are the **parsed** commit message
(§5.7). Message parsing lives in `halyard-conventional` (M1.2, winnow).
If `halyard-git` cannot call it, either the git engine re-implements the
grammar (a second parser, exactly the drift M1.2's fuzz + differential
suite exists to prevent) or every consumer of the walk must assemble
`HistoryDigest` itself (the "real provider" stops being a
`HistoryProvider` and the planner's snapshot contract degrades into
boilerplate every shell must repeat).

## Decision

`halyard-git` depends on `halyard-core` **and** `halyard-conventional`.
`xtask check-layers` carries the rule as
`git → ["halyard-core", "halyard-conventional"]`. The dependency is
harmless by construction: `halyard-conventional` is a **leaf** (zero
workspace dependencies, zero I/O), so no cycle can form and the §7.2
purity story is unchanged — the git engine stays an imperative-shell
crate that only *adds* `gix` to the graph.

## Consequences

- `GitHistory::build(path, workspace)` returns a fully materialized
  [`HistoryDigest`] (parsed commits, PR numbers, trailers, touched
  bitsets) and implements `HistoryProvider` directly — the naive
  `FixedHistory` and the gix engine are interchangeable planner inputs.
- The conventional parser's guarantees (totality proptest, spec goldens,
  fuzz) apply unchanged to every commit the walk emits.
- The layer table in `xtask/src/layers.rs` is the single source of truth;
  the slice protocol's "git → core (only)" is superseded by this ADR.

## Alternatives considered

1. **Re-implement subject parsing in `halyard-git`** — rejected: two
   conventional-commit grammars in one workspace is a standing
   correctness hazard.
2. **Assemble digests in the CLI shell** — rejected: every consumer
   (CLI, xtask gates, tests, the future Action runtime) repeats the same
   mapping; the seam leaks.
3. **Move the parser into core** — rejected: core's dependency list is
   ADR-pinned (§7.2 + ADR-0005) and `winnow` is a real compile-time cost
   for consumers that never read commit messages.
