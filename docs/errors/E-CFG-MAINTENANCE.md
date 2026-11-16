# E-CFG-MAINTENANCE

**Code:** `E-CFG-MAINTENANCE` · **Area:** config (§10.3, ADR-0027) ·
**Raised by:** `halyard_config::resolve`
(`[versioning].maintenance_branches`)

## What it means

A `[versioning].maintenance_branches` entry is not a maintenance line.
Each entry names the branch AND pins its line, so it must parse as
`MAJOR.x` or `MAJOR.MINOR.x` — `1.x`, `1.2.x`, `0.x` are valid;
`1.y`, `12x`, `x`, `1.x.y` are not.

## Cause

- a wrong wildcard spelling (`1.y`, `v1.x`, `rel-1.x` — the entry is
  the literal branch name, no glob);
- a non-numeric part (`1.2.x.y`, `1..x`);
- an entry that names a line but not a branch (or vice versa) — the
  same string is both.

## Fix

Write the branch name exactly as git tracks it, with `.x` as the
line tail, e.g. `maintenance_branches = ["1.x", "1.2.x"]`. A branch
that should not be pinned is simply absent from the list — an
unpinned branch plans free (the guard is branch-scoped, ADR-0027).

## Command

```bash
# the loader refuses the entry before any planning:
halyard plan --format json
# confirm which branch is pinned after the fix:
halyard status
```
