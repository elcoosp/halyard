# halyard CLI

`halyard` plans, explains and ships releases from conventional commits in
Cargo/pnpm repositories. Zero config required: run `halyard plan` inside
any repository root (§16.1). Planning is pure and offline (no network, no
subprocess during planning — the git engine is in-process `gix`) and
consults the §8.7 persistent per-commit cache.

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
halyard apply --dry

# Apply the plan's edits atomically (the only filesystem-writing command).
halyard apply

# A lockfile the in-memory patcher declines is refreshed natively and
# verified automatically (cargo update -w --offline / pnpm install
# --lockfile-only --offline, §9.2 rule 4 / §9.3) — there is no separate
# flag to pass.
```

## Release-PR forecast and checks

```bash
# Forecast the §11.1 machine pass over the preview forge: the tag-phase
# reconciliation (manifest-vs-tags), the prepare-phase Release-PR
# upsert/close, the §11.5 call budget and the §11.3 body. Nothing is
# written; no network is touched (I5).
halyard pr --dry

# Lint a squash-merge PR title and predict the conveyed bump (§11.6).
halyard check --pr-title "feat(parser): accept spans"

# Gate CI: exit code 3 when a release is pending (§15.4).
halyard check --exit-code
```

## Command status (honest)

| Command | Status |
|---|---|
| `plan [--json] [--package]` | available (M1.10) |
| `explain <pkg>` | available (M1.10) |
| `status` | available (M1.10) |
| `graph [--format]` | available (M1.10) |
| `doctor` | available (M1.10) |
| `schema plan\|config` | available (M1.10) |
| `completions <shell>` | available (M1.10) |
| `apply [--dry]` | available (M3.6) |
| `pr --dry` | available (M4.2b step 1; live mode declines with `E-CLI-PR` until the M4.1b forge adapter) |
| `check [--pr-title] [--exit-code]` | available (M4.3) |
| `release` / `publish` / `fetch` / `migrate` / `init` / `action` | **(M4.x/M5)** — absent rather than stubbed |

`apply` is the **only** filesystem-writing command: it applies the plan's
version-bump edits atomically (fsync + rename, `halyard-edit`), never
partially. Everything above it is read-only.

## Global flags

`--format human|json|gha`, `--color auto|always|never` (parsed for the
§16.3 colour-aware renderers, which land with their milestone — output is
plain text until then), `-C <dir>` (run as if started in `dir`),
`--config <file>`, `--offline`, `--no-cache` (skip the §8.7 persistent
per-commit cache; leave `$GIT_DIR/halyard/cache/v1/` untouched),
`--profile` (Chrome-trace JSON to `$HALYARD_PROFILE_PATH`).

## Errors

Failures carry the §15.4 stable codes (`E-DISC-*`, `E-GIT-*`, `E-PLAN-*`,
`E-CLI-*`, …) and render through `miette`; every code has a page under
[docs/errors](../../docs/errors/README.md).
