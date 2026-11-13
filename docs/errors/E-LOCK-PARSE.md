# E-LOCK-PARSE

**Code:** `E-LOCK-PARSE` · **Area:** lockfile patch seam (M3.2, §9.2 ·
M3.4, §9.3) · **Raised by:** `halyard_eco_cargo::lockfile::patch_lockfile`,
`halyard_eco_npm::lockfile::patch_pnpm_lockfile`,
`CargoAdapter::patch_lockfile`, `NpmAdapter::patch_lockfile`,
`halyard_edit::apply_plan` (lockfile stages)

## What it means

A lockfile patch pass failed outright and mutated nothing: the lockfile
is not valid UTF-8, does not parse as its lockfile grammar (TOML for
`Cargo.lock`, the restricted importer grammar for `pnpm-lock.yaml`), or a
planned change is not a usable exact version (empty package name,
non-semver `from`/`to`, or `from == to`).

## Cause

- the `Cargo.lock` on disk is corrupted or hand-edited into invalid TOML;
- the `pnpm-lock.yaml` on disk is corrupted or carries a shape the
  restricted scanner cannot admit (unexpectedly indented lines, tab
  indentation, duplicate importer rows);
- the plan carries a change the planner could not have produced (a plan
  bug or a hand-assembled plan JSON).

## Fix

Re-run `halyard plan` to regenerate the plan against the current tree.
If the lockfile itself is corrupt, regenerate it with the ecosystem's own
tool (`cargo update --workspace --offline`,
`pnpm install --lockfile-only --offline`) before re-planning. Nothing was
mutated by the failed pass (§9.1 apply protocol: all or none).

## Command

```bash
cargo run -p halyard-eco-cargo --example patch_lockfile
cargo run -p halyard-eco-npm --example patch_pnpm_lockfile
cargo test -p halyard-eco-cargo --test differential
cargo test -p halyard-eco-npm --test differential
```
