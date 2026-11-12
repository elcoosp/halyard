# ADR-0002: Crates enter the workspace when their milestone lands

- **Status:** accepted
- **Date:** 2026-11-12
- **Milestone:** M0
- **Deciders:** autonomous implementation agent

## Context

§7.1 sketches the full layout (19 crates, `bench/`, `fuzz/`, `action/`).
M0.1 says "workspace scaffold per §7.1", but the Definition of Done (TASK §3)
simultaneously forbids dead scaffolding, empty stubs and
`#[allow(dead_code)]`. Creating 19 empty crates would violate the DoD; the
two requirements cannot both be satisfied literally on day one.

## Decision

The workspace starts with exactly the crates that carry real content in
M0 — `halyard-core`, `halyard-config`, `halyard-test-support`, `xtask` — and
each subsequent milestone adds its crates as it implements them (e.g. M1.1
grows `halyard-core`; M2 adds `halyard-git`, `halyard-cache`; M1.10 adds
`halyard-cli`). §7.1 is treated as the *target* layout, reached
incrementally.

`xtask check-layers` errors on any workspace member without a layer rule,
so every crate arrival is a conscious table update rather than a silent
addition.

## Consequences

- The tree always compiles with zero dead scaffolding; every crate has
  content, tests and a layer rule.
- §7.1 full layout is not literally present until the milestones that own
  its crates complete — recorded per milestone in `PROGRESS.md`.
- Nightly CI jobs for future crates (fuzz, Miri) are pre-armed behind
  `hashFiles(...)` existence guards so no workflow edits are needed when a
  crate lands.

## Alternatives considered

1. **Create all 19 crates as empty skeletons** — rejected: violates the DoD
   (no dead scaffolding) and would produce `missing_docs`/`dead_code`
   pressure that incentivizes lint suppressions.
2. **Create only one mega-crate now and split later** — rejected: §7.2
   layering is load-bearing from the first line (core purity, I10 unsafe
   boundary); splitting later costs more than starting layered.
