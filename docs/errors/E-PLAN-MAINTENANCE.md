# E-PLAN-MAINTENANCE

**Code:** `E-PLAN-MAINTENANCE` · **Area:** planner (§10.3, ADR-0027) ·
**Raised by:** `halyard_core::planner::plan` (the maintenance-line
guard over the final targets)

## What it means

The checked-out branch is pinned to a maintenance line
(`[versioning].maintenance_branches`, e.g. `1.x` or `1.2.x`), and a
planned release would leave that line — the message names the package,
the pinned line and the offending target (`app would release 2.0.0 on
the 1.x maintenance branch`).

## Cause

- a breaking commit landed on the maintenance branch (the §10.1 rule
  maps it to `Major`, which crosses the line);
- a minor bump on a `MAJOR.MINOR.x` pin (`1.2.x` refuses `1.3.0`);
- the §10.2 pre-major remap applies first and the line guard is the
  last gate: on `0.1.x` a breaking commit remaps to `minor` (`0.2.0`),
  which still leaves the line and refuses;
- a channelized train target carries the prerelease suffix — its
  stable triple is what must stay in line.

## Fix

Land the breaking work on the default branch and backport only fixes
to the maintenance branch, or force the one release you really want
with a `Release-As` footer — the explicit escape hatch is exempt from
the guard (the ADR-0025 exemption reasoning). Rebinding the line is a
config change (`[versioning].maintenance_branches`), never a guess.

## Command

```bash
# the guard refuses before any manifest edit is produced:
halyard plan --format json
# what the branch pins:
halyard status
```
