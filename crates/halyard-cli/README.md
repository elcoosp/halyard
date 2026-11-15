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

# Apply the plan's edits atomically (manifests, lockfiles, changelogs;
# `release` (live) additionally writes git refs — the two writing commands).
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
# written; no network is touched (I5). The JSON carries each pending
# release's forge notes (§9.4 `github-release-notes`, rendered from the
# package changelog's section for the released version — no section, no
# fabricated notes).
halyard pr --dry --format json | jq '.pending_releases[].notes'

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
| `doctor [--registries]` | available (M1.10; M5.1 adds the opt-in `--registries` probe — crates.io + npm reachability and protocol comprehension through the `Registry` seam, network touched only by the flag, I5) |
| `schema plan\|config` | available (M1.10) |
| `completions <shell>` | available (M1.10) |
| `apply [--dry]` | available (M3.6) |
| `pr [--dry]` | available (M4.2b step 1 forecast + M4.1b live): live mode runs the §11.1 machine over the GitHub REST adapter — fail-closed on missing credentials and foreign forges; the live report carries the §11.5 governor summary (M5.5) |
| `check [--pr-title] [--exit-code]` | available (M4.3) |
| `release [--dry] [--no-push]` | available (§11.1 step 2, git side): live mode creates the annotated tags through the real writer (targets resolved by the §11.1 manifest-pinpoint walk with per-row provenance) and pushes them to `origin` through the §11.2 push seam (`--no-push` skips; a missing remote is an honest skip); the forge half (GET-by-tag-first release creation) runs over the M4.1b adapter — through the §11.5 governor (M5.5) — when credentials allow it and is an honest skip otherwise; `--dry` forecasts everything offline |
| `forge-smoke [--repo] [--api-base]` | available (M4.1b, the nightly job's read-only probe; M4.1c adds the App-auth mode — the JSON carries the resolved `auth_mode`; M5.5 adds the governor row) |
| `fetch` | available (§8.8, M4.6 step 1, ADR-0016): probes `origin`'s tags, selects each package's latest release tag through the planner's template rule, fetches those tags plus the base branch in one minimal `git fetch` (blob-less; shallow clones deepen bounded by the previous releases; a failing minimal fetch retries once as the full fetch of the same refs); idempotent (I1) — tags already local at the probed target are not requested; a remote without a configured URL is an honest no-op; no `--dry` by design (the remote listing is the one input an offline forecast cannot have) |
| `publish [--dry]` | available (M5.2 step 2, ADR-0018): the §13 engine validates the plan (recomputed hash, I8), layers the targets dependencies-first, resumes through the registry existence probes (already-published versions are no-ops, I1), waits for index visibility before dependents publish, and spawns `cargo publish` / `pnpm publish` per target through the adapter crates' verified argv tables (§13.4); `--dry` forecasts offline — the tools run their documented dry form, the registry is never probed (I5); failures are rows and the re-run continues from the first failure (§13.7); a partial publish exits 4 (§15.4); live mode under `--offline` fails closed (`E-CLI-PUBLISH`) |
| `migrate` / `init` / `action` | **(M4.x/M5)** — absent rather than stubbed |

## Forge credentials (M4.1c, ADR-0014)

Two modes, resolved from the environment (App wins when present; a partial
App set fails closed naming what is missing):

```bash
# GitHub App (recommended: App actions trigger the repository's own
# workflows, which a static GITHUB_TOKEN does not — §11.5):
export HALYARD_APP_ID=12345                     # the App ID (or client ID)
export HALYARD_APP_PRIVATE_KEY_PATH=~/halyard-app.private-key.pem
export HALYARD_APP_INSTALLATION_ID=42

# …or the static token (HALYARD_FORGE_TOKEN first — the nightly job's
# contract — then GITHUB_TOKEN):
export HALYARD_FORGE_TOKEN=…

halyard doctor        # reports the resolved auth mode before the run
```

`apply` is the **only** filesystem-writing command: it applies the plan's
version-bump edits atomically (fsync + rename, `halyard-edit`), never
partially. `release` (live) writes git refs (annotated release tags) and
pushes them to `origin` — the git-side §11.1 step-2 effects above.
`publish` (live) writes to the **registries** (the §13 steps 2–4: the
resume probes, the tool uploads, the visibility wait) — the plan gates
what may publish (I8), and the re-run after a failure never duplicates an
artifact (I1). Everything else is read-only.

## Forge call governor (M5.5, §11.5, ADR-0015)

The three live forge paths (`pr` live, the `release` forge half,
`forge-smoke`) run through the §11.5 call governor: content-creating
calls are spaced one second apart, `Retry-After` rate limits are
honored exactly, transport failures retry with a deterministic capped
backoff, and the report carries the end-of-run usage summary (policy,
spend, waits). Three `HALYARD_FORGE_*` env knobs override the default
policy — absent variables keep the default; an invalid value fails
closed (`E-FORGE-REQUEST`) naming the variable:

```bash
export HALYARD_FORGE_MIN_CALL_SPACING_MS=1000   # pacing between content calls (0 = off)
export HALYARD_FORGE_MAX_CALLS=40               # per-run call budget (E-FORGE-BUDGET when spent)
export HALYARD_FORGE_MAX_RETRIES=3              # retries per operation after the first failure

halyard doctor      # reports the resolved governor policy
halyard pr --format json | jq '.governor'   # the run's summary: policy, spend, events
```

## Publishing (M5.2, §13, ADR-0018)

```bash
# The offline forecast: the tools run their documented dry form
# (`cargo publish --dry-run --manifest-path … --offline`,
# `pnpm publish --dry-run --no-git-checks` — verified against the tool
# docs), the registry is never probed (I5).
halyard publish --dry --format json | jq '.rows[].outcome'

# The live run: resume probes (already-published versions skip, I1),
# topological layers (dependencies first, bounded within-layer
# parallelism), the §13 step-3 visibility wait before dependents
# publish, and the real tool spawn. Failures are rows — the re-run
# continues from the first failure; a partial publish exits 4 (§15.4).
halyard publish
```

Publishing is opt-in (`[publish] enabled = true`, §16.4); the engine
refuses a tampered plan (`E-PUB-PLAN`), a target dependency cycle
(`E-PUB-ORDER`), an unroutable registry (`E-PUB-REGISTRY`) and a failed
tool (`E-PUB-EFFECT`) with the stable `E-PUB-*` codes.

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
