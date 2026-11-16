# E-CFG-CHANNEL

**Code:** `E-CFG-CHANNEL` · **Area:** config (§10.3, ADR-0025) ·
**Raised by:** `halyard_config::resolve` (`[versioning].channel`)

## What it means

The `[versioning].channel` value is not a valid prerelease channel
name. A channel is **one semver identifier**: alphanumerics and
hyphens, non-empty, no leading or trailing hyphen (`rc`, `beta`,
`next-1` are valid; `rc.`, `-rc`, `rc-`, `` are not).

## Cause

- a dotted or otherwise multi-identifier value (`rc.1`, `pre rc`);
- a leading or trailing hyphen (`-rc`, `rc-`);
- an empty string (`channel = ""` — remove the key instead, which is
  the documented promotion act).

## Fix

Set a single identifier, e.g. `channel = "rc"`. Removing the
`[versioning].channel` key entirely plans stable releases and promotes
a leftover prerelease manifest to its own stable triple (ADR-0025).

## Command

```bash
# the loader refuses the value before any planning:
halyard plan --format json
```
