# ADR-0055: the M6.4 stability promises — `docs/stability.md` and the schema-compat gates

- **Status:** accepted
- **Date:** 2026-11-25
- **Milestone:** §17 M6.4 — "Stability promises: CLI JSON, plan
  schema, exit codes, config semver policy — `docs/stability.md`;
  schema-compat tests." The row's deliverable was entirely absent from
  the tree (`find docs -name 'stability*'` empty; no stability test
  file in `crates/halyard-cli/tests/`); the M6 row's other items
  (M6.1's corpus fetch, M6.2's hardware-budget rerun, M6.3's external
  audit, M6.5's public benchmark site, M6.6's three-month dogfood) are
  network-, hardware-, or time-bound and stay recorded-open.
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

M6.4 is the first M6 row whose whole acceptance is in-tree, and both
halves were missing: no `docs/stability.md`, and no test pinning the
surfaces a consumer may hold halyard to. The underlying contracts
exist and are already machine-gated *individually* — the exit-code
path and the JSON error envelope ride the `CliError` machinery, the
`--format` vocabulary is the §16.2 clap enum, the plan schema is
M0.7's `schema-check` drift gate over `halyard-plan/v1`, and the
config loader's unknown-key refusal is `E-CFG-UNKNOWN-KEY` — but the
*promise* (what a consumer may rely on across releases) was written
nowhere, and no single test file proved the promise's surfaces against
the shipped binary end-to-end.

## Decision

- **The contract document is `docs/stability.md`** (the plan-named
  path): four sections, one per plan area — exit codes (0 success /
  1 app error / 2 clap usage error), CLI JSON (the `human | json |
  gha` vocabulary, the one-JSON-document rule, the
  `{"error":{"code","message"}}` envelope, additive-field policy), the
  plan schema (`halyard-plan/v1` frozen identity, the generated
  checked-in schema, the embedded-plan consumer), and the config
  semver policy (`halyard.toml` v1, unknown keys fail closed with
  `E-CFG-UNKNOWN-KEY`, additive keys are minor, renames are major).
  Each section names the gate that pins it; a closing section states
  honestly what is *not* stable pre-1.0 (command set, flag spellings,
  human renderings) and that the `E-*` codes are stable earlier by
  their own contract.
- **The machine half is `crates/halyard-cli/tests/cli_stability.rs`**
  — the schema-compat tests the plan names: six gates driving the
  built binary (not re-implementing the internals): exit codes over
  `--help`/a workspace-less `status`/an unknown subcommand; the JSON
  error envelope parsed and its keys + `E-`-prefix asserted; the
  `--format` vocabulary pinned from `--help` text; the
  `halyard-plan/v1` identity via `halyard_core::plan::PLAN_SCHEMA_V1`
  plus the schema document's existence/JSON-ness/`$comment`
  generator; the config unknown-key refusal exercised through
  `--config` with a `bogus-key` fixture asserting the stable code.
- **No new schema, no new error code, no CLI change.** The ADR
  records a promise over surfaces that already exist; where a pin
  found the surface stronger than the prose claimed (the exit codes'
  third value is clap's own `2`), the document records the machine
  truth, not a nicer fiction.

## Consequences

- A consumer can now cite one page (and one test file) when holding
  halyard to its word; a maintainer proposing a breaking change must
  edit both the page and the pinning gates — the ADR-for-format-changes
  rule the schema's `$comment` already states extends to the contract
  page.
- The M6.4 row's remaining acceptance is fully landed in-tree; the
  row's corpus/hardware/time-bound siblings stay recorded-open.
- The stability suite drives the shipped binary through the temp-dir
  error paths — it adds no fixture workspaces and runs offline.
