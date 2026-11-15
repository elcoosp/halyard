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
sides. The gate this does **not** open: the shell's routing map owns
the two §13 registry clients (crates.io sparse index, npm packument) —
a third-party cargo index has no resume-probe client, so a
custom-registry run refuses `E-PUB-REGISTRY` **before any spawn** (dry
included, by design). The end-to-end custom-registry publish waits on
a third-party client joining the routing map; the argv grammar and the
spawn wiring above are complete and unit-pinned meanwhile.

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
