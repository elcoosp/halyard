# Research: pnpm-lock.yaml grammar (VERIFY, M0.6)

**Checked:** 2026-11-12 · **Source:** pnpm docs, "Lockfile"
<https://pnpm.io/lockfile> (fetched live)

## Verified facts

- Current schema: `lockfileVersion: '9.0'`; the field "describes the schema
  of the entries inside a document, not how many documents the file
  contains".
- A lockfile may hold **two YAML documents**: an optional **boot document**
  (first; present iff the first line is `---`; carries `configDependencies`
  and `packageManagerDependencies`) and the always-present **project
  lockfile** (last; the real dependency graph). Readers must use a
  multi-document API; "each carries its own packages and snapshots maps".
- Top-level keys (exact): `lockfileVersion`, `settings` (e.g.
  `autoInstallPeers`, `excludeLinksFromLockfile`, `integrityTarget: tar`),
  `importers` (keyed by path, `"."` = root; per-importer groups
  `dependencies`, `configDependencies`, …), `packages` (keyed
  `name@version`), `snapshots`.
- A dependency entry records `specifier` (declared range) and `version`
  (resolved). A `packages:` entry carries `resolution.integrity` (SRI
  digest) and, since v12.0.0, an optional `revision:` field (registry
  revisions: "revision: 0 is the original, and a positive number is a
  replacement").
- Gotcha for release tools: when a boot document pins the pnpm version,
  `pnpm install --frozen-lockfile` fails with
  `ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE` if the pin was bumped
  without regenerating the lockfile.

## Design consequences (feeds M6/M12)

- Bump commits that touch `package.json` **must** also patch
  `pnpm-lock.yaml` (`importers.*.dependencies[name].version`/`specifier`,
  plus `packages`/`snapshots` entries of the released workspace packages),
  or every subsequent install fails the frozen-lockfile check.
- Parsing must be multi-document aware and byte-preserving (I4): pnpm's
  emitter sorts keys and quotes versions — re-serializing with a generic
  YAML library will not reproduce the original bytes.
