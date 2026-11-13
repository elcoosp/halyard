# halyard CLI

`halyard` plans, explains and ships releases from conventional commits in
Cargo/pnpm repositories. Zero config required: run `halyard plan` inside
any repository root (§16.1). Planning is pure and offline (no network, no
subprocess during planning — the git engine is in-process `gix`).

## Local flow

```bash
# Compute and print the release plan (pure, offline).
halyard plan

# Stable JSON: the frozen halyard-plan/v1 document (byte-stable across runs).
halyard plan --json

# Same, but only the releases array (requires jq).
halyard plan --json | jq '.releases'

# Show every package, its version, last tag, unreleased commits and pending state.
halyard status

# Print the dependency graph (text | mermaid | dot | json).
halyard graph --format mermaid

# Environment diagnostics: shallow clone? commit-graph? tags?
halyard doctor

# Why is this package being released? The reason chain (§5.6).
halyard explain app

# Preview the filesystem edits (version bumps) without writing anything.
halyard apply --dry-run          # (M3.6)

# Apply the plan's edits atomically (the only filesystem-writing command).
halyard apply                    # (M3.6)

# Also refresh Cargo.lock / pnpm-lock.yaml after the version bumps.
halyard apply --refresh-lockfile # (M3.6)
```

## Command status (honest)

| Command | Status |
|---|---|
| `plan [--json]` | available (M1.10) |
| `explain <pkg>` | available (M1.10) |
| `status` | available (M1.10) |
| `graph [--format]` | available (M1.10) |
| `doctor` | available (M1.10) |
| `schema plan\|config` | available (M1.10) |
| `completions <shell>` | available (M1.10) |
| `apply [--dry-run] [--refresh-lockfile]` | **(M3.6)** — not present yet |
| `pr` / `release` / `publish` / `fetch` / `check` / `migrate` / `init` / `action` | **(M3.6–M4.x)** — absent rather than stubbed |

`apply` is delivered in M3.6 and will be the **only** filesystem-writing
command: it applies the plan's version-bump edits atomically
(fsync + rename, `halyard-edit`), never partially. Everything above it is
read-only.

## Global flags

`--format human|json|gha`, `--color auto|always|never` (honours
`NO_COLOR`/`CLICOLOR_FORCE`), `-C <dir>` (run as if started in `dir`),
`--config <file>`, `--offline`, `--no-cache` (accepted for forward
compatibility), `--profile` (Chrome-trace JSON to `$HALYARD_PROFILE_PATH`).

## Errors

Failures carry the §15.4 stable codes (`E-DISC-*`, `E-GIT-*`, `E-PLAN-*`,
`E-CLI-*`, …) and render through `miette`; every code has a page under
[docs/errors](../../docs/errors/README.md).
