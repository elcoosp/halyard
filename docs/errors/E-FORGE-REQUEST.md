# E-FORGE-REQUEST

**Code:** `E-FORGE-REQUEST` · **Area:** forge seam (M4.1a, §11.4) ·
**Raised by:** the seam's validation constructors
(`BranchName::new`, `MemoryForge::create_release`'s tag check) and every
adapter that must fail closed on a spec the forge would reject

## What it means

The caller sent a malformed request: an empty or whitespace-bearing
branch name, an empty release tag, or (in real adapters) a spec field the
forge would reject outright. This is a caller bug — fail-closed, never
retried.

## Cause

- a misconfigured `base_branch` or tag template produced an invalid name
  (`halyard.toml` keys `repo.base_branch` / `repo.tag_style`);
- a bug in the caller passed an unvalidated value across the seam.

## Fix

Fix the configuration (or the caller). The message names the exact field
and why it was rejected; nothing was sent to the forge.

## Command

```bash
halyard doctor            # lints the resolved config, including tag templates
```
