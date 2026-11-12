# ADR-0005: `serde_json` becomes an unconditional `halyard-core` dependency

- **Status:** accepted
- **Date:** 2026-11-12
- **Milestone:** M1 (slice A: M1.1)
- **Deciders:** autonomous implementation agent (deviation from the ADR-0001
  arrangement → documented here per §0 rule 3)

## Context

§5.8 and invariant I3 make the plan hash (`blake3` over canonical JSON) part
of the core contract: the planner (M1.9, living in `halyard-core` per §4)
must be able to compute `plan_hash`, and `halyard-config` must compute the
analogous `config_hash` (§8.7). ADR-0001 arranged `serde_json` as a
feature-gated dependency ("schema-gen only"), which puts canonical-JSON
serialization behind a tool-only feature and forces either a hand-written
JSON emitter in core or a second canonicalization implementation in config.

## Decision

`serde_json` moves from the `schema-gen` feature set to an **unconditional**
`halyard-core` dependency. `halyard-core/src/canonical.rs` owns the single
canonical-JSON emitter (`halyard_core::canonical_json`), the blake3 helper
(`halyard_core::blake3_hex`) and `halyard_core::plan_hash`;
`halyard-config` reuses them for `config_hash` instead of duplicating the
format. `schemars` remains optional and feature-gated exactly as ADR-0001
specified. `xtask check-layers` now requires `schemars` to be optional and
accepts `serde_json` unconditionally for `halyard-core`.

## Consequences

- The §7.2 five-dependency rule for `halyard-core` is amended to six
  (`serde_json` joins the sanctioned set); the runtime graph stays
  dependency-lean — `serde_json` is pure (no I/O), fits the "pure core"
  constraint, and was already in every build via dev-dependencies and
  `xtask`.
- Canonical JSON is defined once: sorted object keys, no insignificant
  whitespace, RFC 8785-style string escaping; the format is pinned by an
  insta snapshot and the determinism tests.
- The schema documents regenerate with an updated `description` for
  `Plan` (doc-comment pointer to `plan_hash`); the wire format itself is
  unchanged.

## Alternatives considered

1. **Hand-written canonical JSON emitter over the plan structs** —
   rejected: escaping/ordering bugs in a bespoke serializer are exactly
   the class of defect the differential snapshot tests would have to
   protect against forever; `serde_json` is already sanctioned territory
   (ADR-0001) and the emitter over `serde_json::Value` is ~40 lines.
2. **Keep hashing behind the `schema-gen` feature** — rejected: the plan
   hash is runtime contract data (I8), not a schema tool; gating it would
   force dependents to enable a tool-only feature or re-implement hashing.
3. **Move `plan_hash` into a new `halyard-hash` crate** — rejected: an
   extra crate for one function invites the same §7.2 exception churn;
   `canonical.rs` in core keeps the contract next to the types it hashes.
