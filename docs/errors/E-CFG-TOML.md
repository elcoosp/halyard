# E-CFG-TOML

**Code:** `E-CFG-TOML` · **Area:** config (M1.8) · **Kinds:** `io`,
`toml`, `schema`

## What it means

A configuration file could not be read or parsed as `halyard.toml` v1:
the file is missing, unreadable, syntactically invalid TOML, or a value
does not fit the typed model (unknown enum variant, wrong type).

## Cause

- the file does not exist at the given path (wrong `-C` or `--config`
  argument);
- invalid TOML syntax (unclosed string, duplicate key);
- a value outside the documented set — e.g. `cascade = "sometimes"`
  (`always` | `when-unsatisfied` | `never`).

## Fix

Read the diagnostic: it carries the path as given (never canonicalized)
and the TOML/serde message with the offending location. Run
`cargo run -p xtask -- schema-check` to confirm the checked-in schema
matches your halyard build.

## Command

```bash
# parse exactly what the tool sees and print the first error:
cargo run -p halyard-config --example resolve_layers
# or type-check a file directly:
cargo run -p halyard-config -- load --config ./halyard.toml   # (CLI: M1.10)
```
