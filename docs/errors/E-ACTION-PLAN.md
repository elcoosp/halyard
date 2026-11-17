# E-ACTION-PLAN

**Code:** `E-ACTION-PLAN` · **Area:** action-output seam (§12 step 1,
ADR-0037) · **Raised by:** the `halyard action` plan-file write
(`halyard-action/src/lib.rs`: the canonical plan document)

## What it means

The canonical plan file — the document the `plan_json` output names —
could not be written: the serialization failed or the path is not
writable.

## Cause

- the `--plan-out` path names a parent directory that does not exist
  (the caller's contract is never invented — halyard does not
  `mkdir -p` behind the flag);
- a permission denial or a read-only filesystem on the target path;
- a plan serialization failure (a corrupted plan document — not
  reachable through the CLI pipeline).

## Fix

Create the target's parent directory first (`mkdir -p
$(dirname <path>)`) or choose a path that exists; the default
(`halyard-plan.json` in the working directory) is always writable in
a normal checkout.

## Command

```bash
# Land the plan file in an existing directory:
cargo run -p halyard-cli -- action --plan-out ./halyard-plan.json
```
