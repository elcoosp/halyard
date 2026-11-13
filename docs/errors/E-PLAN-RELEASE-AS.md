# E-PLAN-RELEASE-AS

**Code:** `E-PLAN-RELEASE-AS` · **Area:** planner (M1.9, §10.1) ·
**Raised by:** `halyard_core::planner::plan`

## What it means

A commit carries a `Release-As: <version>` footer whose version is
unusable: it does not parse as semver, or it is older than the package's
current manifest version. The plan refuses the footer instead of moving
a package backwards.

## Cause

- a typo in the footer (`Release-As: 1..0`);
- the footer sits on a commit that predates the current release line.

## Fix

Fix the footer to a valid semver version newer than the manifest
version, or remove it. Only footers on commits already reachable from
the protected base branch are honoured (invariant I7).

## Command

```bash
git log --format='%H %s' --grep 'Release-As' --all
cargo run -p halyard -- explain <package>
```
