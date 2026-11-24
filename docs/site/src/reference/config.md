# Configuration

Everything in `halyard.toml` is optional; running without a config file uses
the built-in defaults, and `halyard init` scaffolds a comment-only skeleton
whose commented defaults load exactly as the built-in defaults. Unknown keys
are errors with a did-you-mean suggestion (`E-CFG-UNKNOWN-KEY`).

## The key reference

```toml
[repo]
base_branch = "main"                    # (auto: remote HEAD)
tag_style   = "auto"                    # auto | "{name}-v{version}" | "v{version}"  (single pkg => v{version})

[versioning]
policy      = "independent"             # independent | fixed | linked
pre_major   = { breaking = "minor", feat = "patch" }   # (cargo-compatible)
cascade     = "when-unsatisfied"        # always | when-unsatisfied | never
channel     = "rc"                      # (unset) prerelease channel; removing it promotes
channel_branches = { next = "rc" }      # (unset) branch -> channel bindings
maintenance_branches = ["1.x"]          # (unset) maintenance lines: MAJOR.x / MAJOR.MINOR.x

[commits]
types.feat  = "minor"
types.fix   = "patch"
types.perf  = "patch"
types.refactor = "none"
merge_policy = "first-parent"
initial_release = { max_commits = 10000 }

[changelog]
style = "keep-a-changelog"              # or path to a minijinja template
hidden_types = ["chore", "docs", "ci", "test", "style"]

[publish]
enabled = false                         # opt-in
cargo = { registry = "crates.io", wait_for_index = "90s" }
npm   = { access = "public", provenance = true }

[forge]
kind = "github"                         # github | gitlab | gitea
commit_mode = "auto"                    # git | api

[[package]]                             # per-package override (match by name or path glob)
name = "internal-tool"
release = false
channel = "rc"                          # (unset) per-package train; wins over [versioning]
include = ["src/**", "Cargo.toml"]
exclude = ["**/*.md"]

[[link]]                                # explicit polyglot link
from = "my-crate"
to   = "@scope/my-crate-node"
mode = "lockstep"
```

Per-package `[versioning]` overrides on `[[package]]` refine the repo-wide
policy: `release = false` removes a package from the release set, `channel`
gives it its own prerelease train (see
[Channels and maintenance trains](../concepts/channels.md)).

## Resolution layers

Low to high, later layers win:

1. built-in defaults
2. `halyard.toml` (the repository root; the `.config/halyard.toml` shadow
   location is deliberately refused by `init` to avoid the two-file footgun)
3. `package.metadata.halyard` (Cargo) / `package.json#halyard` (npm)
4. environment variables (`HALYARD_*`; the forge/publish credential grammar
   rides the documented tool variables)
5. CLI flags

The resolved configuration feeds the `config_hash`, which is part of the
persistent cache key — a policy change invalidates the cached attributions,
so two configurations never share walk results.

## Foreign configuration migration

`halyard migrate` reads release-please, changesets, knope, and release-plz
configurations and produces a mapped report (what maps, what differs
semantically, what has no equivalent) plus a validated draft `halyard.toml`.
`halyard migrate --write` lands that draft create-only: it refuses when the
target exists or when two configured tools would both claim the repository,
and it never writes without a review.
