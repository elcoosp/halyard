# Halyard stability contract (M6.4)

> Halyard is pre-1.0: minor releases may add anything, and breaking
> changes are allowed with a changelog note. **From 1.0**, the promises
> below hold: a minor release must keep every surface on this page
> working; a change that cannot keep it needs a major release and an
> ADR. Each promise names the gate that pins it — the promise is only
> as good as the machine check that fails when it breaks.

This page is the §17 M6.4 deliverable ("Stability promises: CLI JSON,
plan schema, exit codes, config semver policy — `docs/stability.md`;
schema-compat tests"). The machine half is
`crates/halyard-cli/tests/cli_stability.rs` (the gates cited per
promise); the schema documents are additionally gated by
`cargo run -p xtask -- schema-check` (M0.7's drift test).

## Exit codes

The exit-code vocabulary is three values, stable forever:

| Code | Meaning | Source |
|---|---|---|
| `0` | the run did what was asked (including a dry run) | the command's success path |
| `1` | an application error: the diagnosis is the stable §15.4 code (`E-*`), printed with the condition and the fix | the `CliError` path in `crates/halyard-cli/src/main.rs` |
| `2` | a usage error: unknown flag or subcommand — the process never starts | clap's own refusal |

Shell scripts gate on the code, not the text. The pinned gate:
`exit_codes_are_the_documented_contract`.

## CLI JSON

- The output-format vocabulary is exactly `human | json | gha`
  (`--format`; §16.2). New formats are additive; renaming or removing
  a value is breaking.
- Every command accepts `--format json` and prints **exactly one JSON
  document** to stdout on both the success and the failure path.
- The error envelope is `{"error": {"code": "E-…", "message": "…"}}`:
  `code` is the stable §15.4 code (the same code the human report
  renders), `message` is the diagnosis. New keys may appear inside the
  `error` object; the two documented keys never move or rename.
- Success payloads may gain fields in a minor release (additive);
  removing or renaming a field is breaking. The `plan` command's JSON
  is the plan wire contract itself (below), not a separate shape.
- Pinned gates: `json_errors_ride_the_documented_envelope`,
  `the_format_vocabulary_is_the_pinned_three`.

## Plan schema

- The plan wire format is **`halyard-plan/v1`**
  (`halyard_core::plan::PLAN_SCHEMA_V1`, §5.8). The identity string is
  frozen: a `halyard-plan/v2` is a new major boundary, not an edit.
- The JSON Schema document `schemas/halyard-plan-v1.json` is
  generated from the Rust plan types by
  `cargo run -p xtask -- schema-check --write` and checked in; the CI
  gate fails any type drift that skips regeneration, and the schema's
  `$comment` names the generator — hand edits refuse.
- The Release PR's embedded plan (`<!-- halyard:plan v1 … -->`, §11.3)
  and `halyard release`'s verification read the same contract, so a
  plan produced by one halyard version is consumable by the newer one
  within the same major.
- Pinned gates: `the_plan_schema_identity_is_pinned` (this file's
  suite) + the `schema-check` CI gate (M0.7).

## Config semver policy

- `halyard.toml` is **v1**; its schema document
  (`schemas/halyard-config-v1.json`) is generated and drift-gated the
  same way as the plan schema.
- Unknown keys **fail closed** with `E-CFG-UNKNOWN-KEY` (naming the
  key, the file, the section and the valid alternatives — see
  `docs/errors/E-CFG-UNKNOWN-KEY.md`). halyard never silently accepts
  a key it does not know, so a config written for a newer halyard
  cannot misparse under an older one: the refusal is the
  forward-compatibility contract.
- Adding an optional key is a minor release. Renaming a key or
  changing its value grammar is breaking: the old spelling must keep
  working through the major (the loader's `did-you-mean` refusal is
  the migration surface, not a silent alias).
- Env layers and layered files resolve by the same rule: every
  surface that feeds the loader refuses unknown keys identically.
- Pinned gate: `config_unknown_keys_fail_closed`.

## What is explicitly *not* stable pre-1.0

The CLI's command set, flag spellings, and the human-rendered output
(plain text and the `gha` rendering) may change between minor
versions — the README's status section says the same. The four
surfaces above are the promise; everything else is discovered by
running `--help`. The error **codes** (`E-*`, §15.4) are stable
earlier than 1.0 by their own contract: scripts and the docs/errors
pages key on them, and the taxonomy only ever grows additively.
