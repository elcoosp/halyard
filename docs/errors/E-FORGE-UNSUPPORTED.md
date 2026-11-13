# E-FORGE-UNSUPPORTED

**Code:** `E-FORGE-UNSUPPORTED` · **Area:** forge seam (M4.1a, §11.4) ·
**Raised by:** the [`Forge::commit_via_api`](../../crates/halyard-forge/src/lib.rs)
trait default (the documented honest decline, mirroring the lockfile
seam's `NeedsNative` convention, §7.3)

## What it means

The adapter was asked for a capability it does not implement — today that
is the API-signed commit (`commit_mode = "api"`, §11.5) on a forge
without an API-commit endpoint. The adapter names itself in the message
instead of silently degrading.

## Cause

- the resolved `forge.commit_mode` is `"api"` (or `"auto"` with an app
  token present) but the selected forge adapter cannot mint signed
  commits.

## Fix

Switch to `commit_mode = "git"` (plain push, unsigned with a
`GITHUB_TOKEN`) in `halyard.toml`, or use a forge/credential combination
that supports API commits.

## Command

```bash
halyard doctor            # shows the resolved forge kind and commit mode
```
