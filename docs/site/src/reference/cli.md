# CLI commands

The `halyard` shell's wired commands. Every effect-carrying command supports
`--dry`; output is available as `human`, `json` (byte-stable documents), and
`gha` (GitHub Actions `::`-annotated) through `--format`.

## Planning and inspection

| Command | Purpose |
|---|---|
| `plan [--json] [--package P]` | compute and print the release plan (pure, offline) |
| `explain <pkg>` | why this version: the reason chain, skipped commits and why |
| `status` | table: package, version, last tag, unreleased commits, pending state |
| `graph` | print the dependency graph (`text`, `mermaid`, `dot`, `json`) |
| `affected [--base <ref>] [--ci SYSTEM]` | which packages must rebuild and test; `--ci` renders the selection in a CI system's own grammar |
| `doctor` | environment diagnostics: shallow clone?, commit-graph?, tags?, token scopes?, config lint, registry reachability (`--registries`), publish-auth rows |

## Mutations and release flow

| Command | Purpose |
|---|---|
| `apply [--dry]` | write the plan's manifest and lockfile edits to the working tree (local release flow) |
| `pr [--dry]` | prepare or update the Release PR: plan → §11.3 body → the release state machine over the forge adapter |
| `release [--dry] [--no-push]` | create the annotated tags, push them, create the forge releases; targets resolved by the manifest-pinpoint walk |
| `publish [--dry]` | publish pending packages: verified-plan layers, registry resume probes, visibility wait, the verified-argv tool spawn |

## CI and automation

| Command | Purpose |
|---|---|
| `check` | lint the PR title (`--pr-title`) and pending commits (`--commits`); `--exit-code` exits 3 when a release is pending; `--comment` posts the verdict to the PR |
| `action [--mode auto\|pr\|release\|plan\|check] [--dry] [--plan-out PATH]` | the GitHub Action surface: the §12 outputs and job summary over the action-output seam |
| `fetch` | smart minimal fetch: probes the remote, fetches only the refs the release bases need (a shallow clone stays shallow) |
| `bench` | time the planning pipeline stages over the current repository (a measurement, never a gate) |
| `forge-smoke` | run the forge adapter against a live GitHub target (nightly lane, credentials required) |

## Setup and integration

| Command | Purpose |
|---|---|
| `init [--github]` | scaffold a comment-only `halyard.toml` and, with `--github`, the one-file release workflow (create-only, never required) |
| `migrate [--write]` | survey release-please / changesets / knope / release-plz configs into a mapped report; `--write` lands the reviewed draft `halyard.toml` |
| `schema` | emit the frozen JSON Schemas (config, plan) |
| `completions` | shell completions |
| `man [<command>]` | the `halyard(1)` manual page and one page per subcommand |

## Global flags

| Flag | Meaning |
|---|---|
| `--format human\|json\|gha` | output format (default `human`; the JSON documents are byte-stable) |
| `--color auto\|always\|never` | color policy (the renderers honor `NO_COLOR`) |
| `-C <dir>` | run as if started in `<dir>` |
| `--config <file>` | explicit `halyard.toml` path |
| `--offline` | refuse every network-touching effect |
| `--no-cache` | bypass the persistent per-commit cache (the cold measurement `bench` uses) |
| `--profile` | emit the stage-span trace used by `bench` |

Exit codes follow the error taxonomy: `0` ok / nothing to do, `1` generic
failure, `2` usage/config, `3` plan has a pending release (for
`check --exit-code`), `4` partial publish (retryable), `5` forge
rate-limited (retryable) — see [Error codes](errors.md#exit-codes).
