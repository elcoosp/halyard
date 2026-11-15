# Publish tooling (M5.2 step 2) — VERIFIED argv grammar

Status: **VERIFIED live 2026-11-14** (per §0.4: both pages fetched, quotes
below are verbatim from the fetched documentation).

## `cargo publish` (Cargo Book — doc.rust-lang.org/cargo/commands/cargo-publish.html)

| Flag | Documented meaning (verbatim) |
|---|---|
| `--dry-run` | "Perform all checks without uploading." |
| `--manifest-path path` | "Path to the Cargo.toml file. By default, Cargo searches for the Cargo.toml file in the current directory or any parent directory." |
| `--offline` | "Prevents Cargo from accessing the network for any reason. … With this flag, Cargo will attempt to proceed without the network if possible." |
| `--registry registry` | "Name of the registry to publish to. Registry names are defined in Cargo config files. … Otherwise it will use the default registry, which is defined by the `registry.default` config key which defaults to crates-io." |
| `--index index` | "The URL of the registry index to use." |

The book's own example combines the dry path hermetically:
`cargo publish --dry-run --manifest-path ./my-crate/Cargo.toml --offline`.

**Decision:** dry argv = `cargo publish --dry-run --manifest-path
<manifest> --offline` (the documented hermetic check — a forecast must
not touch the network); live argv = `cargo publish --manifest-path
<manifest>` (credentials and network are the live run's business).
`--registry`/`--index` mapping from `PlanPublish.registry` is deferred
to M5.3's auth plumbing (the default registry needs no flag; a custom
one is a config-defined name the shell does not guess).

## `pnpm publish` (pnpm.io/cli/publish)

| Flag | Documented meaning (verbatim) |
|---|---|
| `--dry-run` | "Does everything a publish would do except actually publishing to the registry." |
| `--no-git-checks` | "Skip the git checks." (by default pnpm verifies "that the working tree is clean, that the current branch is the publish branch") |
| `--access <public\|restricted>` | "Tells the registry whether the published package should be public or restricted." |
| `--tag <tag>` | "Publishes the package with the given tag. By default, pnpm publish updates the latest tag." |

**No directory flag is documented on the page** — the documented
directory mechanism is `publishConfig.directory` in `package.json`.
The engine therefore spawns `pnpm publish` with `current_dir()` set to
the package root (the same cwd-rooting discipline the tag-push effect
uses; the ambient cwd is never inherited).

**Decision:** dry argv = `pnpm publish --dry-run --no-git-checks` (a
forecast must not depend on the working tree's git state — I3; the
checks are pnpm's live-mode guards, not forecast inputs); live argv =
`pnpm publish` bare (the tool's own clean-tree/branch checks stay the
last line of defense in the run that actually publishes).

**Tool selection (npm family):** §13.4 names `pnpm publish` as the npm-
side tool; halyard follows the spec letter — the npm ecosystem's argv
table is `pnpm publish`. A plain-npm repository without pnpm fails
honestly (`E-PUB-EFFECT`: tool missing), never silently switches to a
different tool with different packaging semantics. `npm publish` /
`yarn npm publish` / `bun publish` variants are a documented Year-2
extension (config-selectable), not a v1 guess.

## The `--registry` flag mapping (M5.3 continued, VERIFIED 2026-11-15)

Fetched `doc.rust-lang.org/cargo/commands/cargo-publish.html` (the
Cargo Book's `cargo publish` page) and quoted verbatim:

> **`--registry` *registry*** — "Name of the registry to publish to.
> Registry names are defined in [Cargo config files]. If not specified,
> and there is a [package.publish] field in Cargo.toml with a single
> registry, then it will publish to that registry. Otherwise it will
> use the default registry, which is defined by the [registry.default]
> config key which defaults to crates-io."

The mapping halyard ships: the plan's publish contract carries the
configured registry name (`publish.cargo.registry`, default
`crates.io`); the argv table emits `--registry <name>` **only when the
name is not the default registry** — the two accepted default spellings
(`crates.io` and Cargo's own dashed config spelling `crates-io`) ride
the tool's own documented default (`registry.default` → `crates-io`)
with no flag, exactly the no-guess rule: halyard never passes a flag
for the state the tool already defaults to. Any other configured name
reaches the spawn as `--registry <name>` verbatim (names are Cargo
config keys — the token side of the same name resolves through
`CARGO_REGISTRIES_<NAME>_TOKEN`, the auth seam's documented grammar,
so argv and env can never disagree about which registry a run
publishes to). The default-spelling rule mirrors
`halyard_publish::auth`'s `is_crates_io` and is pinned by tests on both
sides.

## The custom-registry routing (M5.2 continued, ADR-0021) — VERIFIED 2026-11-15

The resume-probe side of the same name resolves through the cargo
configuration — the file `cargo publish --registry <name>` itself
reads, so the probe and the tool cannot disagree about where a registry
lives. VERIFIED against the Cargo Book "Registries" page
(doc.rust-lang.org/cargo/reference/registries.html, fetched
2026-11-15), verbatim:

> To use a registry other than crates.io, the name and index URL of
> the registry must be added to a `.cargo/config.toml` file. The
> registries table has a key for each registry, for example:

```toml
[registries]
my-registry = { index = "https://my-intranet:8080/git/index" }
```

> The `index` key should be a URL to a git repository with the
> registry's index or a Cargo sparse registry URL with the `sparse+`
> prefix.

and the environment override, verbatim:

> As with most config values, the index may be specified with an
> environment variable instead of a config file. For example, setting
> the following environment variable will accomplish the same thing as
> defining a config file:
>
> ```
> CARGO_REGISTRIES_MY_REGISTRY_INDEX=https://my-intranet:8080/git/index
> ```

The `--registry` name resolution is on the same page: "Several of
Cargo's commands such as `cargo publish` take a `--registry`
command-line flag to indicate which registry to use" — the name being
the key defined under `[registries]` (the env-variable rule converts
dashes and dots to underscores, the same rule the auth seam's token
fragment documents).
`halyard_eco_cargo::registry_config` implements exactly this grammar:
files in ascending precedence (`$CARGO_HOME/config.toml`, then
`<project>/.cargo/config.toml`), the env override winning over both, a
set-but-empty override refused (the `E-PUB-AUTH` empty-token rule at
the index scale), a malformed file refused rather than read as absent,
and only the `sparse+` HTTP form classified probeable — a git-protocol
index is an honest `E-PUB-REGISTRY` decline naming the protocol and
the fix, because the §13 resume probe is one GET of an NDJSON file and
cannot speak the git protocol. The loopback CLI test proves the full
§13 sequence over a configured custom registry (resume-probe miss →
the shimmed tool's `--registry` argv → visibility confirmed through
the same index). The gaps this does **not** open: source replacement
(`[source.<name>]` mirroring) is not a publish-target form and stays
out of scope; the legacy `config` filename (deprecated upstream) is
not read; an npm-side per-package override (`publishConfig.registry`)
needs per-target routing and is the next slice.

## The npm per-target routing (M5.2 continued, ADR-0021) — VERIFIED 2026-11-15

The npm side routes per *package*: two npm packages of one workspace
may publish to different registries, and the spawned `pnpm publish`
reads the package's own configuration. VERIFIED against the npm
documentation (docs.npmjs.com, fetched 2026-11-15), verbatim:

`publishConfig` (the package.json docs):

> This is a set of config values that will be used at publish-time.

> If you would like to ensure that a given package is only ever
> published to a specific registry … then use the publishConfig
> dictionary described below to override the registry config param at
> publish-time

The `registry` config key (the config docs):

> The base URL of the npm registry.

with the default `https://registry.npmjs.org/`, and the environment
override rule, verbatim:

> Any environment variables that start with `npm_config_` will be
> interpreted as a configuration parameter.

`halyard_eco_npm::publish_registry` implements exactly this grammar:
`publishConfig.registry` (the per-package publish-time override)
outranks `NPM_CONFIG_REGISTRY`, which outranks the `.npmrc` files (the
per-project file wins over the per-user `$HOME/.npmrc`, passed in
ascending precedence — the cargo-resolution convention); a scoped
package's `@scope:registry` key beats the plain `registry` key; a
set-but-empty value is refused (the house empty-value rule at the URL
scale) and a non-HTTP value is an honest decline naming the source.
One ordering is npm-undocumented and pinned by decision: env vs
`publishConfig` — the documented "override the registry config param
at publish-time" rule reads as publish-specific-over-general, so
`publishConfig.registry` wins; recorded here as a decision, not a
quote. The engine's routing seam grew the per-target shape this
needs: `RegistryRouter::registry_for` (the by-name default, so every
M5.2 router keeps working) and `TargetRouter` (per-`(registry,
package)` overrides) — the CLI shell resolves every npm target's
manifest + npmrc chain and joins each custom base as a
`Npm::with_registry_base` override. The loopback CLI test proves the
full §13 sequence over a publishConfig-pinned workspace (resume-probe
miss → the shimmed `pnpm publish` → visibility confirmed through the
packument at the custom base — the default registry is never
contacted).

## Remaining VERIFY items (open, like ADR-0017's)

- `cargo publish --dry-run` without `--offline` reaches the network for
  index/dep resolution in some setups — the dry argv pins `--offline`
  to stay hermetic; whether a *live* run needs
  `--allowed-registry`-style scoping is an M5.3 auth question (the
  `--registry` flag mapping itself is resolved — see the section
  above).
- pnpm `publishConfig.directory` interaction with monorepo roots
  (recorded, untested — the engine passes the package root as cwd).
- npm/yarn/bun publish argv equivalence (Year-2 tool selection).
