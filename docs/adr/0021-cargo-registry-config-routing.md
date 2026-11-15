# ADR-0021: the custom-registry routing over the cargo configuration (M5.2 continued, §13)

- **Status:** accepted
- **Date:** 2026-11-15
- **Milestone:** M5.2 continued — the third-party cargo registry resume client
  the M4.5 session declared as the documented open item: a
  `publish.registry` name other than the default resolves through the
  cargo configuration (`[registries.<name>]` and its
  `CARGO_REGISTRIES_<NAME>_INDEX` override) into a probeable
  `halyard_registry::CratesIo` client that joins the routing map, so
  the §13 resume probe, the tool spawn and the visibility wait all run
  against the custom registry
- **Deciders:** autonomous implementation agent (per §0 rule 3; the
  config grammar rides the VERIFIED Cargo Book "Registries" quotes
  recorded in `docs/research/publish-tooling.md` — no new external
  facts; the routing seam and the client URL grammar already existed)

## Context

The routing map shipped at M5.2 with exactly two §13 clients —
`crates.io` and `npm` — and refused everything else with
`E-PUB-REGISTRY` before any call. M5.3 continued the argv side
(`cargo publish --registry <name>` mapping, unit-pinned) but left the
probe side closed; the M4.5 session pinned the refusal honestly in
`tests/cli_publish.rs` and recorded the open item in the research note:
"the end-to-end custom-registry publish waits on a third-party registry
client joining the routing map". The gap at base:

- a custom-registry run could not *resume* (the §13 step-2 probe had no
  client), could not *confirm visibility* (the §13 step-3 wait needs
  the same client), and was refused even in dry mode (routing
  validation is mode-independent, §13.8);
- nothing read the cargo configuration — the file the spawned
  `cargo publish --registry <name>` consults for the same name — so a
  registry's *index URL* had no authoritative in-repo source.

Constraints that shaped the design:

1. **One source of truth for where a registry lives.** The probe and
   the tool must resolve the same registry the same way; inventing a
   halyard-side config key (`[publish.cargo.index]`) would let the two
   disagree. The cargo configuration is the source the tool itself
   reads, so it is the one the probe reads.
2. **The probe speaks sparse HTTP only.** The Cargo Book's `index` key
   accepts a git-repository URL *or* a `sparse+` URL; the resume probe
   is one GET of an NDJSON file — a git index is not probeable without
   a git client and a clone. Fail-closed decline, never a guess.
3. **Config-reading must be honest about absence and corruption.** A
   config file that cannot be parsed might have carried the entry;
   reading it as "absent" would half-honor a broken configuration and
   could route a publish to the *default* registry by accident (the
   spawn's `--registry` flag would then fail at the tool — after the
   run reported routing as validated).

## Decision

1. **The resolution lives in the cargo adapter** (`halyard_eco_cargo::
   registry_config`), beside the other cargo-config grammars (manifest
   parse, lockfile, publish argv). The pure core
   (`resolve_registry_index`) takes `(label, text)` config pairs in
   ascending precedence plus the env value; the I/O shell
   (`registry_index`) reads `$CARGO_HOME/config.toml` (default
   `$HOME/.cargo/config.toml`), `<project>/.cargo/config.toml`, and
   the `CARGO_REGISTRIES_<NAME>_INDEX` variable — the env override
   wins over every file, a set-but-empty value is refused (the auth
   seam's empty-token rule at the index scale), and the env-fragment
   rule (uppercase, dots and dashes to underscores) is the documented
   twin of `halyard_publish::auth`'s token fragment.
2. **Only the `sparse+` HTTP form joins the routing map.**
   `sparse_index_base` strips the scheme and trims the trailing slash
   into the base a `CratesIo::with_index_base` client probes. A bare
   `http(s)://` URL is classified as the git-index form and declined
   with the protocol named; any other shape (`ssh://`, `file://`,
   `git@host:path`, a bare path) declines the same way. The decline is
   the shell's `E-PUB-REGISTRY` — no new error code (the existing
   surface covers "the run has no probeable client for this registry").
3. **The shell resolves eagerly, before any call.** `publish_router`
   takes the repo root and the publication's targets, routes the two
   built-in names as before, and resolves every other distinct target
   registry name through the adapter; the outcome is either a client
   in the map or the honest decline naming the registry and the fix.
   Lazy per-call resolution would need the `RegistryRouter` seam to
   own interior mutability for no benefit — the engine already
   resolves routing up front (validate-then-act, §15.3), and the
   resolution reads local files only.
4. **Precedence follows cargo's own merge semantics, documented.** A
   later (closer-to-project) file overrides an earlier one *per key*;
   a `[registries.<name>]` table without an `index` key does not erase
   an earlier file's index, but an entry-without-index with nothing
   beneath it is a distinct, honest decline (a named config gap, not
   "unknown"). A malformed file refuses the resolution outright.
5. **The end-to-end proof is offline.** The live-mode CLI test runs a
   stateful loopback sparse index (first GET per path `404`, later
   GETs the NDJSON version line): the resume probe misses, the shimmed
   `cargo` records the `--registry corp` argv, the §13 step-3 wait
   confirms through the same index, and the served-path set pins that
   the probes rode the custom registry at its sparse paths — no
   network beyond `127.0.0.1`, no real registry touched.
6. **The npm side routes per target.** Two npm packages of one
   workspace may publish to different registries — the spawned
   `pnpm publish` reads each package's own configuration — so a
   by-name route cannot carry the npm resolution. The routing seam
   gains `RegistryRouter::registry_for` (a default method delegating
   to the by-name route, so every M5.2 router keeps working
   unchanged) and `TargetRouter` (per-`(registry, package)` overrides
   beside the by-name map); the engine resolves every target through
   `registry_for`. The shell reads each npm target's manifest and
   `.npmrc` chain through `halyard_eco_npm::publish_registry`
   (VERIFIED against the npm docs — `publishConfig.registry` outranks
   `NPM_CONFIG_REGISTRY`, which outranks the `.npmrc` files, a scoped
   package's `@scope:registry` key beats the plain key; verbatim
   quotes in `docs/research/publish-tooling.md`) and joins each custom
   base as an `Npm::with_registry_base` override; the documented
   default rides the by-name route; a non-HTTP value refuses the run
   closed naming the package and the manifest. One ordering is
   npm-undocumented and pinned by decision: `publishConfig.registry`
   beats the env override (publish-specific-over-general).

## Consequences

- A workspace publishing to a configured private registry gets the full
  §13 machinery — resume (I1 no-ops on the re-run), layer gating and
  the visibility wait — instead of a structural refusal; the
  unconfigured name keeps the same refusal it had (now naming the
  config keys that would fix it).
- The dry forecast gains fidelity it was missing: a configured
  registry forecasts instead of refusing, and the refusal cases (git
  index, malformed config) are refused *with the reason*, so the
  forecast shows exactly what the live run would do.
- `halyard-publish` and the engine keep their shapes — the cargo
  half rides the existing `RegistryRouter` seam and the existing
  `E-PUB-REGISTRY` code; `halyard-registry` is untouched (its
  `with_index_base` was built mirror-ready at M5.1). The npm half
  adds the per-target seam (Decision 6): the engine's routing loop
  moves from `registry(name)` to `registry_for(target)` — the default
  method means no shipped router changes behavior.
- What stays open: source-replacement (`[source.<name>]` mirroring)
  is not a publish-target form and stays out of scope; a git-index
  probe client (spawning `git` against the index repo) has no
  §-requirement.
- *(As-built, 2026-11-15, the routing-completion slice: the two
  declared open items above are closed — the legacy `config` filename
  is read with the Cargo Book's documented precedence (the file
  without the extension wins when both exist; the earlier "deprecated
  upstream" note was wrong — the page never says deprecated) and the
  npmrc scoped-registry environment variables resolve through the
  probe-verified literal `npm_config_@<scope>:registry` name
  (case-insensitive; the underscore intermediate form is NOT the
  scoped key), with the scoped key governing a scoped package
  regardless of the source tier. The VERIFIED quotes and the seven
  probes live in `docs/research/publish-tooling.md`.)*
