# E-LOCK-NATIVE

**Code:** `E-LOCK-NATIVE` · **Area:** native lockfile refresh fallback
(M3.2, §9.2 rule 4 · M3.4, §9.3) · **Raised by:**
`CargoAdapter::refresh_lockfile_native`,
`NpmAdapter::refresh_lockfile_native`,
`halyard_eco_cargo::lockfile::verify_native_refresh`,
`halyard_eco_npm::lockfile::verify_native_refresh`, `halyard apply`

## What it means

The native lockfile refresh effect step failed, or its outcome was
rejected by the post-refresh verification:

- **Unsupported** — the adapter has no sanctioned native refresh;
- **Spawn** — the tool (`cargo update --workspace --offline`,
  `pnpm install --lockfile-only --offline`, `npm install
  --package-lock-only`, `yarn install --mode update-lockfile`,
  `bun install --lockfile-only`) could not be started;
- **Failed** — the tool exited non-zero (its stderr is surfaced
  verbatim), or no npm-family lockfile exists to refresh;
- **Verification** — the refresh moved rows beyond the workspace members
  (a dependency resolution change): for `Cargo.lock` a registry row
  moved, a row appeared or disappeared, or the format header changed;
  for `pnpm-lock.yaml` a `packages:`/`snapshots:` resolution key
  changed, a non-member importer row moved, or the `lockfileVersion`
  header changed.

A `Verification` failure means the tree drifted in a way the plan cannot
express; nothing should be committed or pushed.

## Cause

- the tool is absent from `PATH` (spawn) or failed for its own reasons
  (network, invalid manifest) — its message is authoritative;
- the workspace's external requirements changed between the plan and the
  refresh (resolution change, §9.2 rule 4 / §9.3).

## Fix

For spawn/failed: run the ecosystem's own lockfile command yourself and
address the tool's diagnosis. For verification: re-run `halyard plan`
against the refreshed tree and review the new plan before applying.

## Command

```bash
cargo update --workspace --offline
pnpm install --lockfile-only --offline
cargo test -p halyard-eco-cargo --test differential
cargo test -p halyard-eco-npm --test differential
```
