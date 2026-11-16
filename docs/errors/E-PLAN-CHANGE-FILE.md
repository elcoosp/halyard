# E-PLAN-CHANGE-FILE

**Code:** `E-PLAN-CHANGE-FILE` · **Area:** planner change-file input
(§9.4, ADR-0028) · **Raised by:** `halyard_core::planner::plan` (the
unknown-package validation over the parsed change files)

## What it means

A change file parsed cleanly but names a package the workspace does
not know — the message names the change file and the unknown name
(`change file '.halyard/changes/next.md' names 'ap', which this
workspace does not know`). The strict input discipline of
`E-CFG-UNKNOWN-KEY` applied to the change-file layer: a typo'd name
refuses the plan instead of silently dropping the file's bump and
note. Nothing was mutated.

## Cause

- a typo in the `packages` table (`ap` instead of `app`);
- a package that lives in another workspace root than the one the
  command ran against;
- a package that was renamed or removed while its change file is
  still around.

## Fix

Correct the name in the change file to a discovered package (check
`halyard plan --format json`'s release and skip rows, or `halyard
graph`) — or delete the consumed change file, which is the normal
post-release cleanup anyway (ADR-0028: consumed-on-release is the
maintainer's contract; a leftover file keeps proposing its bump until
it is removed). A package that exists but must not release
(`[[package]] release = false`) does NOT refuse — the skip wins over
every bump source, silently by design.

## Command

```bash
# the plan refuses before any edit is produced:
halyard plan --format json
# what the workspace knows:
halyard graph
```
