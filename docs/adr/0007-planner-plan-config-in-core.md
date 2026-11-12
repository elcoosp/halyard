# ADR-0007: The planner consumes a pure `PlanConfig` owned by core

- **Status:** accepted
- **Date:** 2026-11-12
- **Milestone:** M1 (slice C: M1.9)
- **Deciders:** autonomous implementation agent (deviation from §4's
  `plan(snapshot, cfg: &ResolvedConfig)` signature → documented here per
  §0 rule 3)

## Context

§4 gives the planner signature `pub fn plan(snapshot: &Snapshot, cfg:
&ResolvedConfig) -> Result<Plan, PlanError>` with the planner living in
`halyard-core::planner`. `ResolvedConfig` lives in `halyard-config`, which
depends on `halyard-core` (for `TagTemplate`). A literal reading makes
`halyard-core` depend on `halyard-config` — a workspace dependency cycle,
and a violation of the §7.2 layer rule that `xtask check-layers` enforces
(`halyard-core` has **no** workspace dependencies).

## Decision

The planning knobs are defined once, in core, as a plain-data struct
`halyard_core::planner::PlanConfig`: the commit-type → bump mapping, the
pre-major policy, the cascade policy, tag-style resolution inputs,
`initial_max_commits`, the changelog style/hidden types, the publish
master switch and registry names, and the per-package release overrides.
`plan()` takes `cfg: &PlanConfig`.

`halyard-config` grows the additive conversion
`ResolvedConfig::plan_config(&self) -> PlanConfig` (plus a
`PlanConfig::from_resolved` alias), so every caller that holds a resolved
configuration reaches the §4 shape with one call:

```rust
let plan = halyard_core::planner::plan(&snapshot, &resolved.plan_config())?;
```

`PlanConfig` derives `Serialize`/`Deserialize`/`PartialEq` so golden plans
can pin the exact configuration that produced them, and so the
`config_hash` chain stays intact (the mapping is deterministic).

## Consequences

- The §4 signature is realized as `plan(&Snapshot, &PlanConfig)` in core;
  the *capability* (`plan(snapshot, resolved-config)`) is unchanged and one
  conversion call away. The deviation is mechanical: layering, not design.
- `halyard-config` stays the only place that knows `ResolvedConfig`; core
  never sees config-layer types.
- Tests that exercise the planner directly construct `PlanConfig` (or use
  the config crate's conversion, dev-dependency of the core tests).

## Alternatives considered

1. **Move `ResolvedConfig` into core** — rejected: `halyard-config` owns
   `halyard.toml` parsing, env layering and `config_hash`; dragging the
   TOML loader into the pure core violates §4's "no I/O" more than a
   plain-data knob struct does.
2. **`plan()` in a new crate between config and core** — rejected: §4 and
   §17 (M1.9) fix the planner's home as `halyard-core::planner`; the plan
   hash (I8) is a core contract.
3. **Function-pointer knobs** (pass `apply_bump`/`rewrite` closures) —
   rejected: the planner's determinism contract (I3) is easiest to defend
   when every knob is data.
