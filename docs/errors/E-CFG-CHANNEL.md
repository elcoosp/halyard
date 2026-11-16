# E-CFG-CHANNEL

**Code:** `E-CFG-CHANNEL` · **Area:** config (§10.3, ADR-0025/ADR-0026) ·
**Raised by:** `halyard_config::resolve` (`[versioning].channel`,
`channel_branches` values, `[[package]] channel` overrides)

## What it means

The `[versioning].channel` value is not a valid prerelease channel
name. A channel is **one semver identifier**: alphanumerics and
hyphens, non-empty, no leading or trailing hyphen (`rc`, `beta`,
`next-1` are valid; `rc.`, `-rc`, `rc-`, `` are not).

## Cause

- a dotted or otherwise multi-identifier value (`rc.1`, `pre rc`);
- a leading or trailing hyphen (`-rc`, `rc-`);
- an empty string (`channel = ""` — remove the key instead, which is
  the documented promotion act);
- the same grammar applied to the step-2 branch bindings
  (`channel_branches = { next = "rc." }` — the error names the branch)
  and the step-3 per-package overrides (`[[package]] channel = "rc."`
  — the error names the package; an override without a `name` is also
  refused, since a path-glob override cannot carry a train).

## Fix

Set a single identifier, e.g. `channel = "rc"`. Removing the
`[versioning].channel` key entirely plans stable releases and promotes
a leftover prerelease manifest to its own stable triple (ADR-0025).
For a `[[package]] channel` override (ADR-0026), set the fix on the
named package's entry and give the entry a `name`.

## Command

```bash
# the loader refuses the value before any planning:
halyard plan --format json
```
