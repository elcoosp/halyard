# E-PLAN-CYCLE

**Code:** `E-PLAN-CYCLE` · **Area:** planner (M1.9, §10.4) · **Raised
by:** `halyard_core::planner::plan`

## What it means

The dependency graph contains a cycle through non-dev edges, so no
reverse-topological cascade order (and no publish order) exists. The
message renders the cycle path (`a -> b -> a`).

## Cause

- two or more packages depend on each other through normal, build or
  peer edges (dev-dependency cycles are allowed and exempt, §6.6);
- a `[[link]]` configuration that closes a loop with real edges.

## Fix

Break the cycle in the manifests: move one direction of the dependency
to `dev-dependencies`, extract the shared code into a third package, or
(experimentally) configure `cascade = "never"`. The rendered path names
the exact members to change.

## Command

```bash
cargo run -p halyard -- graph --format mermaid
cargo test -p halyard-core planner
```
