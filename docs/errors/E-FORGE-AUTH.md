# E-FORGE-AUTH

**Code:** `E-FORGE-AUTH` · **Area:** forge seam (M4.1a, §11.4) ·
**Raised by:** every [`Forge`](../../crates/halyard-forge/src/lib.rs)
adapter; the reference [`MemoryForge`](../../crates/halyard-forge/src/memory.rs)
produces it through its failpoint harness

## What it means

The forge rejected the run's credentials: the token is absent, expired, or
lacks the scopes the operation needs (Release PRs need `contents: write`
and `pull-requests: write`, §16.1).

## Cause

- the credential environment variable is unset (the CLI doctor surfaces
  this before any call is made);
- the token's scopes or app permissions do not cover the operation;
- the credential was minted for a different installation or repository.

## Fix

Re-export the credential with the required scopes, or (for App installs)
re-run the minting step. Never paste a token into the command line — the
error message names the missing scope, never the credential.

## Command

```bash
halyard doctor            # reports token presence/scope-shape before the run
```
