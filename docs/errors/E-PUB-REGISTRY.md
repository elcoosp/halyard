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
§16.4's `[publish]` table). Since M5.2 continued (ADR-0021) a
non-default cargo registry name is resolved through the cargo
configuration before the run — this error is raised when that
resolution yields nothing probeable, and the message says which case:

- **unconfigured** — no `[registries.<name>]` entry in
  `.cargo/config.toml` / `$CARGO_HOME/config.toml` and no
  `CARGO_REGISTRIES_<NAME>_INDEX` override; or a
  `[registries.<name>]` entry that carries no `index` key;
- **a git-protocol index** — the entry's `index` is a bare
  `http(s)://` URL (the Cargo Book's git-repository index form); the
  resume probe speaks the sparse-HTTP protocol only;
- **a malformed config file** — the TOML cannot be parsed, so the
  entry cannot be read from it (refused, never read as absent);
- **a set-but-empty env override** —
  `CARGO_REGISTRIES_<NAME>_INDEX` is set to `""` (the `E-PUB-AUTH`
  empty-token rule at the index scale);
- **an npm package naming a non-HTTP registry** — the package's
  `publishConfig.registry` (or its `NPM_CONFIG_REGISTRY` / `.npmrc`
  chain) carries a value the packument probe cannot speak (not
  `http(s)`, set-but-empty); the message names the package and the
  manifest (M5.2 continued, ADR-0021's per-target routing).

## Fix

For a cargo registry, configure its **sparse** index URL; for an npm
package, fix the package's `publishConfig.registry` (or the `.npmrc`
chain) to an `http(s)` base URL — or disable publishing for the
affected packages:

```toml
# .cargo/config.toml — the file `cargo publish --registry <name>` reads
[registries.my-registry]
index = "sparse+https://my-registry.example/index/"
```

The `sparse+` scheme is required — a git-protocol index URL is refused
with the protocol named in the message. The documented override form
is the environment variable, e.g.
`CARGO_REGISTRIES_MY_REGISTRY_INDEX=sparse+https://…`.

## Command

```bash
# What the plan asks for, and what the resolved config provides
halyard plan --json | jq '.releases[].publish'
halyard doctor
```
