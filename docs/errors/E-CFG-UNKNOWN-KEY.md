# E-CFG-UNKNOWN-KEY

**Code:** `E-CFG-UNKNOWN-KEY` · **Area:** config (M1.8) · **Raised by:**
`halyard_config::load` / `load_from_str`

## What it means

`halyard.toml` (or `package.metadata.halyard` / a `HALYARD_*` mapping
applied through the loader) contains a key that is not part of the frozen
`halyard.toml` v1 schema. Halyard refuses the file instead of silently
ignoring the section — a typo'd key would otherwise disable behaviour with
no visible effect.

## Cause

- a typo in a key (`repoz` instead of `repo`, `base_branh` instead of
  `base_branch`);
- a key from a different tool (release-please/changesets config pasted in);
- a key from a newer/older halyard version than the schema in use.

## Fix

Use the suggested key from the message (`did you mean ...?` covers every
key within an edit distance of 2). The full list of valid keys for the
location is printed. Schema changes require an ADR (§16.4 is frozen for v1).

## Command

```bash
# see exactly which key is rejected and what is valid there:
cargo run -p halyard-config --example resolve_layers
# schema reference:
cat schemas/halyard-config-v1.json
```
