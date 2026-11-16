# ADR-0033: npm publish options (provenance tri-state + access argv, the runtime gate)

- **Status:** accepted
- **Date:** 2026-11-16
- **Milestone:** M5.3 step 6's npm half — the declared "provenance stays
  M5.3-later" gap, with the VERIFY research recorded at HEAD
  (`docs/research/npm-provenance-oidc.md`)
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

`publish.npm.provenance` and `publish.npm.access` existed as parsed
config keys since the schema landed (the loader's did-you-mean table
listed them) with **no consumer**: `ResolvedConfig` materialized them
into guessed defaults (`access = Public`, `provenance = true`) that
`plan_config()` silently dropped — the plan contract and the spawn argv
never carried them. A pnpm.io fetch recorded at HEAD (2026-11-16) made
the tool grammar explicit and contradicted the guessed defaults: the
`provenance` config's "Default: undefined" means "When unset, trusted
publishing decides" — an invented `--provenance` flag would have
overridden the tool's own posture on every npm publish.

## Decision

1. **Unset is semantic.** `ResolvedConfig` carries
   `Option<NpmAccess>`/`Option<bool>`; `PlanConfig` and `PlanPublish`
   (optional, schema-additive fields) carry them; `PublishTarget` and
   `PublishRow` surface them. The adapter emits `--provenance` only on
   an explicit `true`, `--no-provenance` (the documented self-hosted
   opt-out) only on an explicit `false`, `--access` only on an explicit
   value — and nothing when unset.
2. **The plan contract is the vehicle** (I8): publish consumes a
   verified plan only, so the spawn-side options must be part of it,
   not re-read from the shell. The planner fills the npm contract only;
   cargo releases carry `None` both ways.
3. **The runtime gate, as data.** A live `provenance = true` npm
   target refuses `E-PUB-AUTH` before any call when no supported cloud
   runtime is detected (npm's verified musts: a hosted runner + the
   `id-token: write` permission; the attestations are generated at
   publish time, never from a laptop). The detection (GHA OIDC context,
   or the GitLab `GITLAB_CI` marker — VERIFIED "true when available")
   stays in the shell (`provenance_runtime_detected` over an injected
   reader); the engine consumes the verdict as a
   `PublishPolicy::provenance_runtime` field — plain data, default
   `NotDetected` (fail-closed), validated first in `run_publication`
   (zero probes, zero spawns). The dry forecast never refuses (I5: the
   attestation is publish-time semantics).
4. **Doctor observability**: the `publish-provenance` check reports the
   detected verdict and the gate's consequence; absence is `ok` with
   the refusal spelled.

## Consequences

- npm publishes with explicit options ride the verified argv grammar;
  unset options change nothing (byte-stable argv for existing
  workspaces).
- The report rows and the human/gha/JSON renders surface the resolved
  options (absent when unset — I3).
- The old guessed defaults are gone: `config_hash` inputs change for
  workspaces that set the keys; the cache key changes accordingly (§8.7
  content-hash discipline).
- The plan schema gains two optional props (additive, version-stable).

## Alternatives considered

- **Materialized defaults (the old shape).** Rejected: an invented
  `--access public` changes the scoped-package first-publish default,
  and an invented `--provenance` overrides the tool's own
  trusted-publishing posture — both contradicted the verified docs.
- **Spawn-side config read (no plan contract).** Rejected: the dry
  forecast and the engine report would not show what a live run would
  spawn (I7), and publish's I8 gate would not cover the options.
