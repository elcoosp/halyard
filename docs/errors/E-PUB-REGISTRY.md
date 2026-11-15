# E-PUB-REGISTRY

**Code:** `E-PUB-REGISTRY` · **Area:** publish engine registry routing
(M5.2, §13 step 2) · **Raised by:** [`run_publication`]

[`run_publication`]: https://docs.rs/halyard-publish/latest/halyard_publish/fn.run_publication.html

## What it means

The publish engine has no registry client for one of the plan's
publish targets. Routing is resolved **before any call** (§15.3
validate-then-act): a run with an unroutable registry refuses closed
with zero probes and zero tool spawns. The message names the registry
the engine could not route (e.g. `crates.io`).

This is not a network failure — a probe that could not *reach* the
registry is `E-REG-IO`, and an uninterpretable answer is
`E-REG-PROTOCOL`; both surface as report rows, never as this error.

## Cause

The shell's routing map (the `RegistryRouter`) lacks an entry for the
registry named in the plan's publish contract (`PlanPublish.registry`,
§16.4's `[publish]` table). Typing a registry name that is not
`crates.io` / `npm` (or a configured mirror) produces exactly this.

## Fix

Correct the registry name in the configuration, or disable publishing
for the affected packages:

```toml
# halyard.toml (§16.4)
[publish]
cargo = { registry = "crates.io" }
npm   = { registry = "npm" }
```

## Command

```bash
# What the plan asks for, and what the resolved config provides
halyard plan --json | jq '.releases[].publish'
halyard doctor
```
