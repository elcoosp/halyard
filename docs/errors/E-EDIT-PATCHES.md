# E-EDIT-PATCHES

**Code:** `E-EDIT-PATCHES` · **Area:** edit (M1.4) · **Raised by:**
`halyard_edit::apply`, `Patch::new`, `EditSet::add`,
`EditSet::from_plan_edits`

## What it means

A patch set is not applicable: an inverted range (`start > end`), a
missing file, an overlap between two patches, an unsorted set, or a range
outside the file.

## Cause

- stale byte spans: the manifest changed after the plan was computed
  (re-plan before applying — invariant I8);
- a bug in a patch author (adapter emitting overlapping spans).

## Fix

Re-run `halyard plan` to regenerate the edit set against the current
bytes. The message names the offending range and the file length.

## Command

```bash
cargo run -p halyard-edit --example apply_patch
cargo test -p halyard-edit
```
