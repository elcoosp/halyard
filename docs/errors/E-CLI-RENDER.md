# E-CLI-RENDER

**Code:** `E-CLI-RENDER` · **Area:** CLI (M1.10) · **Raised by:**
stable-format output paths (`--format json|gha`)

## What it means

Serializing the plan/report into the requested stable output format
failed (JSON/serde error, or the gha renderer failed on an invariant).

## Cause

- an internal invariant violation while rendering (please report);
- output stream closed early (`| head` cutting a pipe).

## Fix

Re-run with `--format human` to get the data anyway; if it reproduces,
file a bug with the command line — planning itself succeeded.

## Command

```bash
halyard plan --format human          # same data, human renderer
halyard plan --json > plan.json && jq empty plan.json   # validate JSON
```
