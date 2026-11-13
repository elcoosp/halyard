# E-LOCK-NATIVE

**Code:** `E-LOCK-NATIVE` · **Area:** native lockfile refresh fallback
(M3.2, §9.2 rule 4) · **Raised by:** `CargoAdapter::refresh_lockfile_native`,
`halyard_eco_cargo::lockfile::verify_native_refresh`, `halyard apply`

## What it means

The native lockfile refresh effect step failed, or its outcome was
rejected by the post-refresh verification:

- **Unsupported** — the adapter has no sanctioned native refresh;
- **Spawn** — the tool (`cargo update --workspace --offline`) could not
  be started;
- **Failed** — the tool exited non-zero (its stderr is surfaced
  verbatim);
- **Verification** — the refresh moved rows beyond the workspace members
  (a dependency resolution change), added or removed rows, or changed
  the lockfile format header.

A `Verification` failure means the tree drifted in a way the plan cannot
express; nothing should be committed or pushed.

## Cause

- the tool is absent from `PATH` (spawn) or failed for its own reasons
  (network, invalid manifest) — its message is authoritative;
- the workspace's external requirements changed between the plan and the
  refresh (resolution change, §9.2 rule 4).

## Fix

For spawn/failed: run `cargo update --workspace --offline` yourself and
address the tool's diagnosis. For verification: re-run `halyard plan`
against the refreshed tree and review the new plan before applying.

## Command

```bash
cargo update --workspace --offline
cargo test -p halyard-eco-cargo --test differential
```
