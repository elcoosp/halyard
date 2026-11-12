# ADR-0001: Schema-generation dependencies in `halyard-core`

- **Status:** accepted
- **Date:** 2026-11-12
- **Milestone:** M0 (M0.7)
- **Deciders:** autonomous implementation agent, per TASK §1 rule 4 (deviate → ADR)

## Context

§7.2 fixes `halyard-core`'s dependency set to `semver`, `smallvec`, `bstr`,
`blake3`, `serde`. M0.7 requires the `halyard-plan/v1` and `halyard.toml v1`
JSON Schemas to be **frozen from the Rust types** (a hand-written schema
would drift silently, defeating the drift test). Generating a JSON Schema
from the types needs `schemars` derives on the wire types, and materializing
the document needs `serde_json`.

## Decision

`halyard-core` and `halyard-config` carry **optional, feature-gated**
dependencies `schemars` and `serde_json`, enabled only by the `schema-gen`
feature:

- The feature is not in `default`; no dependent crate enables it transitively.
- Only `xtask` enables it (`halyard-core = { features = ["schema-gen"] }`),
  and only for the frozen-schema generation (`xtask schema-check`).
- `xtask check-layers` allow-lists `schemars`/`serde_json` for
  `halyard-core` **only when optional**; a non-optional use of either crate
  in `halyard-core` fails the layer check.
- `deny.toml` documents the same exception.

The §7.2 five-dependency rule therefore still holds for the **runtime
graph**: with default features, `halyard-core` links exactly the allowed
crates.

## Consequences

- The wire types carry `#[cfg_attr(feature = "schema-gen", derive(JsonSchema))]`
  — small compile-time cost, zero runtime cost for dependents.
- Schema documents are byte-deterministic (draft 2020-12, sorted keys) and
  verified by `xtask schema-check` in CI.
- If `schemars` ever leaks into a non-optional edge, CI fails via
  `check-layers` before it can become a runtime dependency.

## Alternatives considered

1. **Hand-written JSON Schemas** — rejected: drift between types and schema
   is exactly the failure mode M0.7's drift test exists to prevent.
2. **Separate `halyard-plan-schema` crate re-declaring the types** —
   rejected: duplicate type definitions are worse drift than a duplicate
   dependency edge.
3. **Unconditional `schemars`/`serde_json` in core** — rejected: violates
   §7.2 for real and drags `serde_json` into every consumer for a feature
   only the workspace tool needs.
