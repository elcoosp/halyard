# ADR-0004: Corpus pinned by `ls-remote` HEAD at authoring time

- **Status:** accepted
- **Date:** 2026-11-12
- **Milestone:** M0 (M0.2)
- **Deciders:** autonomous implementation agent

## Context

M0.2 requires ≥ 12 real repositories pinned "to a commit SHA" such that
`xtask corpus fetch` is reproducible. Pinning needs a concrete SHA per repo
at authoring time; the natural choice is each repository's default-branch
HEAD observed via `git ls-remote`.

## Decision

`corpus/manifest.toml` pins every repository to the exact 40-hex SHA
reported by `git ls-remote <url> HEAD` on 2026-11-12, with the capture date
recorded (`pinned_at`). `corpus fetch` has two modes:

- `--shallow` (default): `git clone --depth 1 --filter=blob:none <url>`, then
  **verify `HEAD == pinned SHA`**. Fails loudly once upstream has moved past
  the pin (expected; pins age).
- `--full`: full clone + `checkout --detach <SHA>` — always exact.

Re-pinning is a deliberate, reviewable manifest change (recorded in the PR),
never a silent fetch.

## Consequences

- Fetching is reproducible by SHA today and verifiable until upstream
  advances; `--full` keeps old pins exact forever.
- Shallow clones carry **no tags** — observed to break tag-anchored
  competitor tools (recorded in `bench/competitors/REPORT.md` and the
  research notes); tag-dependent measurement runs must use `--full`.
- The smoke test (`--only fd`) is what nightly CI runs; full fetches are
  developer-triggered.

## Alternatives considered

1. **Pin to release tags** — rejected: tags move (can be deleted/re-pushed);
   immutable SHAs are the only honest pin.
2. **Git submodules** — rejected: submodule checkouts are stateful and the
   ecosystem tools (cargo/nextest) would still need explicit SHA discipline;
   a manifest + verifier keeps the invariant machine-checked.
