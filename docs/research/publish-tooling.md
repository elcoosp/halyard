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
out of scope. *(The routing-completion slice, 2026-11-15, closed the
legacy-`config` gap — see the section below — and the npm
per-target override shipped with M5.2 continued.)*

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

## The routing completion (ADR-0021's declared open items) — VERIFIED 2026-11-15

The ADR-0021 map's two declared open items, both settled this slice.

### The cargo legacy `config` filename

VERIFIED against the Cargo Book "Configuration" page
(doc.rust-lang.org/cargo/reference/config.html, Hierarchical structure,
fetched 2026-11-15), verbatim:

> Cargo also reads config files without the .toml extension, such as
> .cargo/config. Support for the .toml extension was added in version
> 1.39 and is the preferred form. If both files exist, Cargo will use
> the file without the extension.

Two facts matter and one earlier draft claim was wrong: the page
nowhere says the legacy name is deprecated (it says the `.toml` form is
"the preferred form"), and **the file without the extension wins** when
both exist in one directory. An operator with the old layout therefore
reads a registry config `cargo publish` honors and the resolver would
have ignored — exactly the probe/tool disagreement the seam exists to
prevent. `halyard_eco_cargo::registry_config::config_paths_for`
enumerates, in ascending precedence: `$CARGO_HOME/config.toml`,
`$CARGO_HOME/config`, `<project>/.cargo/config.toml`,
`<project>/.cargo/config` — the legacy file is the later
(higher-precedence) entry of its directory, and the resolver's
per-key merge reproduces the documented rule (the legacy file wins on
conflicting keys; a file without the key does not erase the other
file's entries). Pinned by five tests in
`crates/halyard-eco-cargo/tests/registry_config.rs`.

### The npm scoped-registry environment variable

The npm *config* docs do not document an env form for scoped registry
keys (they point to the .npmrc page, which documents neither), so the
grammar was settled by **probe** against npm 11.19.1 (the
registry-probes method of ADR-0017). Seven probes, observed 2026-11-15:

1. `npm_config_myorg_registry` does **not** map to the scoped key —
   `npm config get @myorg:registry` prints `undefined` and npm warns
   `Unknown env config "myorg-registry". This will stop working in the
   next major version of npm.` (the underscore form becomes a separate
   unknown key).
2. `npm_config_@myorg:registry` **does** set the scoped key — the
   variable name is the literal config-key suffix.
3. `npm config list` shows the underscore form as `myorg-registry` (a
   distinct key, warning attached).
4. `NPM_CONFIG_@MYORG:REGISTRY` resolves identically — "Config values
   are case-insensitive, so NPM_CONFIG_FOO=bar will work the same"
   (the config docs' rule, quoted verbatim) — with no unknown-key
   warning.
5. The env variable beats the same key's .npmrc value (env ranks above
   the npmrc files in the documented priority ladder).
6. A plain `npm_config_registry` env value does **not** beat a
   `@myorg:registry` key from an .npmrc file — the scoped key governs a
   scoped package regardless of the source tier (npm resolves the
   `@myorg:registry` key first for `@myorg/…` packages).
7. Scoped and plain keys coexist independently (`npm config get
   @myorg:registry` vs `npm config get registry` return their own
   values).

`halyard_eco_npm::publish_registry` implements exactly this grammar:
`scoped_registry_env_variable(scope)` is the literal
`npm_config_@<scope>:registry` name, the environment scan is
case-insensitive (`scoped_env_name_matches`), and the resolution order
is: `publishConfig.registry` → the scoped key (env → npmrc) → the
plain key (`NPM_CONFIG_REGISTRY` → npmrc `registry`) → the documented
default — with the set-but-empty refusal naming the variable. Pinned by
seven tests in `crates/halyard-eco-npm/tests/publish_registry.rs`. The
POSIX-shell caveat is recorded honestly: `@` and `:` are not valid
shell identifier characters, so the variable is set through CI env
maps (or `env -i`), never through `VAR=… cmd` — the probe used Python's
environ.

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
