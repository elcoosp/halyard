# E-CFG-ENV

**Code:** `E-CFG-ENV` · **Area:** config (M1.8) · **Raised by:**
`halyard_config::env_layer`

## What it means

A `HALYARD_*` environment variable carried a value the loader cannot map
into the v1 model. The documented set is `HALYARD_BASE_BRANCH`,
`HALYARD_TAG_STYLE`, `HALYARD_CASCADE`, `HALYARD_POLICY`.

## Cause

- a misspelled enum value (`HALYARD_CASCADE=sometimes` — must be `always`,
  `when-unsatisfied` or `never`);
- a variable from a different tool colliding with the `HALYARD_` prefix.

## Fix

Correct the value in the workflow/CI environment (the message lists the
accepted words). Unset variables are simply skipped — there is no need to
define all of them.

## Command

```bash
# show what the env layer would contribute:
HALYARD_CASCADE=never cargo run -p halyard-config --example resolve_layers
env | grep '^HALYARD_'   # inspect current overrides
```
