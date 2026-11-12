# E-EDIT-UNREWRITABLE

**Code:** `E-EDIT-UNREWRITABLE` · **Area:** version (M1.3, §A.2) ·
**Raised by:** `halyard_version::rewrite_requirement`

## What it means

A dependent's requirement on a bumped package cannot be rewritten so it
admits the new version while preserving its meaning. The rewriter handles
bare versions, `=`, `^`, `~`, `>=`, wildcards (`1.*`, `1.2.*`) and the
`>=X, <Y` window under the widen policy. Closed upper bounds (`<=`),
strict lower bounds (`>`) and mixed multi-comparator sets are not
rewritable.

## Cause

- the requirement shape is outside the supported matrix (e.g. `<=2`
  crossed by `2.0.0`);
- the requirement text is not a valid semver requirement at all;
- the `Error` upper-bound policy was configured (appendix A.2 makes the
  `>=1.0, <2` + `2.0.0` case configurable).

## Fix

Either widen the requirement by hand in the manifest (the message shows
the requirement and the new version) or rely on the lockfile refresh
fallback (`NeedsNative`, §9.2) — the planner records the cascade either
way.

## Command

```bash
cargo run -p halyard-version --example bump_algebra
cargo test -p halyard-version
```
