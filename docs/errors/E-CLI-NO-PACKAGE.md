# E-CLI-NO-PACKAGE

**Code:** `E-CLI-NO-PACKAGE` · **Area:** CLI (M1.10) · **Raised by:**
`halyard explain <pkg>`

## What it means

No pending release exists for the requested package: it is either not in
the workspace or skipped in the current plan.

## Cause

- a typo in the package name (`explain` matches exact package names);
- the package has no unreleased commits (nothing pending);
- the package is skipped (release disabled / not publishable).

## Fix

Run `halyard status` to see every package and its pending state, then
explain an existing name. Skipped packages appear in `plan.skipped`.

## Command

```bash
halyard status
halyard plan --json | jq -r '.skipped[].package'
```
