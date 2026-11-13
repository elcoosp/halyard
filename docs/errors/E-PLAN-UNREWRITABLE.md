# E-PLAN-UNREWRITABLE

**Code:** `E-PLAN-UNREWRITABLE` · **Area:** planner cascade (M1.9,
appendix A.2 error path) · **Raised by:**
`halyard_core::planner::plan`

## What it means

A dependent's requirement on an upstream package cannot be rewritten so
it admits the upstream's new version. The planner fails the plan instead
of emitting an edit that would leave the requirement stale (or widen a
bound it was configured not to widen).

## Cause

- a requirement shape outside the rewriter grammar (e.g. `>=8 <10`
  comparators it cannot move, A.2);
- `UpperBoundPolicy::Error` configured and the cascade would have to
  widen an upper bound (`<2` with a move to 3.0.0).

## Fix

Adjust the requirement in the named manifest to a shape the rewriter
handles (`^x.y`, `~x.y`, `=x.y.z`, plain `x.y`), or set
`[versioning] cascade = "never"` in `halyard.toml` if the edge should
not cascade at all.

## Command

```bash
cargo run -p halyard -- explain <package>
cargo test -p halyard-core planner
```
