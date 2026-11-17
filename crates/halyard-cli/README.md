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

# Prerelease channel (§10.3, ADR-0025): with `[versioning].channel = "rc"`
# in `halyard.toml`, every planned release becomes `{target}-{rc}.{n}` —
# `1.3.0-rc.1`, `1.3.0-rc.2`, … with the counter derived from the tags the
# repository already carries (no state file). Non-breaking steps on an
# active train advance the counter; a breaking commit opens a new base;
# removing the key promotes a leftover prerelease manifest to its own
# stable triple. `Release-As` targets are exempt. An invalid name fails
# closed (`E-CFG-CHANNEL`).
#
# Branch-bound channels: `[versioning].channel_branches = { next = "rc" }`
# binds a channel to a branch — HEAD on `next` plans `rc` prereleases,
# every other branch keeps the unbound default (or stays stable). The
# binding rides the §8.7 cache key, so the two branches never share
# attributions.
#
# Per-package trains (ADR-0026): `[[package]] channel = "rc"` names that
# package's train — the most explicit policy, winning over the default
# and the branch bindings; every other package keeps the repo-wide (or
# stable) derivation, and promotion splits per package. An override
# requires a package `name` (path-glob overrides refine include/exclude
# filters) and an invalid name fails closed (`E-CFG-CHANNEL`).
#
# Maintenance trains (ADR-0027): `[versioning].maintenance_branches =
# ["1.x"]` pins the checked-out branch to the line it names — a
# backport `fix` plans the in-line patch, a bump that would leave the
# line refuses (`E-PLAN-MAINTENANCE`, the target named; `Release-As`
# stays the explicit escape hatch). The pin rides the §8.7 cache key
# like the channel binding, so a pinned branch and the default branch
# never share attributions. An entry must parse as `MAJOR.x` or
# `MAJOR.MINOR.x` and is otherwise refused at load (`E-CFG-MAINTENANCE`).

# Show every package, its version, last tag, unreleased commits and pending state.
halyard status

# Print the dependency graph (text | mermaid | dot | json).
halyard graph --graph-format mermaid

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

# Post the verdict to the PR under review (§11.6, M4.5 step 1): the
# comment is found by the stable `<!-- halyard:check v1 -->` marker and
# rewritten in place on every push (I1 — no duplicate comments); both
# verdict shapes post (a rejected title posts its diagnosis, then the
# command still fails with `E-CLI-TITLE`). Live-only: needs forge
# credentials and a GitHub-shaped origin (`E-CLI-COMMENT` declines with
# the fix otherwise); the PR is found by its head branch
# (`GITHUB_HEAD_REF`, else the checked-out branch) and the call rides
# the §11.5 governor as a content call.
halyard check --pr-title "feat(parser): accept spans" --comment
```

## Zero-config scaffold (ADR-0029)

```bash
# Survey the repo and land the documented halyard.toml skeleton
# (create-only; every key optional — the defaults ride as comments).
halyard init

# Also land the §16.1 one-file release workflow:
#   .github/workflows/release.yml (the parent directories are created).
halyard init --github

# The scaffolded repo plans zero-config, right now:
halyard plan
```

Create-only, fail-closed: an existing `halyard.toml` or workflow file is
never clobbered and a refusal never leaves a half-scaffold behind
(`E-CFG-INIT`); an existing `.config/halyard.toml` refuses first — a root
scaffold would shadow it. Needs no git repo and reads no config.

## Affected packages (CI selection, ADR-0032)

```bash
# Which packages does the pending work touch, directly and through
# dependents (dev-dependencies included — the CI selector counts every
# edge kind, unlike the §10.4 publish order)?
halyard affected --format json | jq '.packages'

# Gate a PR job against the merge target: attribute against one explicit
# ref instead of the release bases. An unresolvable ref refuses
# fail-closed (`E-GIT-REF` naming the ref; a shallow clone may need
# `halyard fetch` first).
halyard affected --base origin/main --format json

# The gha render carries the markdown table plus the machine-readable
# tally line (affected=N changed=M).
halyard affected --format gha

# The CI-consumer surface: render the selection in a CI system's own
# grammar — one line per argv atom / the one expression the step
# consumes (ADR-0034/0035/0036; the seven systems are turbo, nx, bazel,
# gha, gitlab, azure, teamcity).
halyard affected --ci turbo
halyard affected --ci azure
```

## Man pages (§16.2)

```bash
# The halyard(1) page — every command and global flag, roff.
halyard man > /usr/local/share/man/man1/halyard.1

# One page per subcommand (the affected(1) page, for example):
halyard man affected

# Pages derive from the parser itself, so they never drift from --help.
halyard man plan | groff -mandoc -T ascii | less
```

## Command status (honest)

| Command | Status |
|---|---|
| `plan [--json] [--package]` | available (M1.10) |
| `explain <pkg>` | available (M1.10) |
| `status` | available (M1.10) |
| `graph [--graph-format]` | available (M1.10; the rendering flag is `--graph-format` — text \| mermaid \| dot \| json — while the global `--format` owns the output vocabulary; sharing one flag made every `graph` invocation panic at HEAD, fixed with this rename) |
| `affected [--base <ref>] [--ci SYSTEM]` | available (ADR-0032, §16.3/M10 step 1 + ADR-0034/0035/0036 step 2): the CI selector — the packages a change touches, directly (pending commits since the base) and transitively (dependents closed over the workspace's reverse dependency edges, dev edges included, the nearest affected ancestor as `via`); `--base` selects against one explicit ref (branch, remote-tracking ref or tag) instead of the release bases — the PR-selection shape — and refuses an unresolvable ref fail-closed (`E-GIT-REF` naming it); offline (I5), JSON byte-stable (I3); `--ci SYSTEM` renders the selection in the CI system's own grammar (turbo \| nx \| bazel \| gha \| gitlab \| azure \| teamcity — see the adapters section below) |
| `doctor [--registries]` | available (M1.10; M5.1 adds the opt-in `--registries` probe — crates.io + npm reachability and protocol comprehension through the `Registry` seam, network touched only by the flag, I5) |
| `schema plan\|config` | available (M1.10) |
| `completions <shell>` | available (M1.10) |
| `apply [--dry]` | available (M3.6) |
| `pr [--dry]` | available (M4.2b step 1 forecast + M4.1b live): live mode runs the §11.1 machine over the GitHub REST adapter — fail-closed on missing credentials and foreign forges; the live report carries the §11.5 governor summary (M5.5) |
| `check [--pr-title] [--exit-code] [--comment]` | available (M4.3; M4.5 step 1 adds `--comment` — the §11.6 verdict comment posted through the forge seam's marker-based `upsert_pr_comment`, rewrite-in-place idempotent, live-only with the honest `E-CLI-COMMENT` decline) |
| `release [--dry] [--no-push]` | available (§11.1 step 2, git side): live mode creates the annotated tags through the real writer (targets resolved by the §11.1 manifest-pinpoint walk with per-row provenance) and pushes them to `origin` through the §11.2 push seam (`--no-push` skips; a missing remote is an honest skip); the forge half (GET-by-tag-first release creation) runs over the M4.1b adapter — through the §11.5 governor (M5.5) — when credentials allow it and is an honest skip otherwise; `--dry` forecasts everything offline |
| `forge-smoke [--repo] [--api-base]` | available (M4.1b, the nightly job's read-only probe; M4.1c adds the App-auth mode — the JSON carries the resolved `auth_mode`; M5.5 adds the governor row) |
| `fetch` | available (§8.8, M4.6 step 1, ADR-0016): probes `origin`'s tags, selects each package's latest release tag through the planner's template rule, fetches those tags plus the base branch in one minimal `git fetch` (blob-less; shallow clones deepen bounded by the previous releases; a failing minimal fetch retries once as the full fetch of the same refs); idempotent (I1) — tags already local at the probed target are not requested; a remote without a configured URL is an honest no-op; no `--dry` by design (the remote listing is the one input an offline forecast cannot have) |
| `publish [--dry]` | available (M5.2 step 2, ADR-0018): the §13 engine validates the plan (recomputed hash, I8), layers the targets dependencies-first, resumes through the registry existence probes (already-published versions are no-ops, I1), waits for index visibility before dependents publish, and spawns `cargo publish` / `pnpm publish` per target through the adapter crates' verified argv tables (§13.4); `--dry` forecasts offline — the tools run their documented dry form, the registry is never probed (I5); failures are rows and the re-run continues from the first failure (§13.7); a partial publish exits 4 (§15.4); live mode under `--offline` fails closed (`E-CLI-PUBLISH`); the spawn injects the resolved publish credentials through the `PublishAuth` seam and scrubs every resolved token from surfaced tool output (M5.3 step 1, ADR-0019 — `E-PUB-AUTH` refuses a set-but-empty variable before any spawn); the JSON carries the per-registry `auth` provenance (variable names only); `publish.cargo.registry` maps to the spawn as `--registry <name>` for a non-default registry (M5.3 continued — the default spellings ride the tool's own documented default with no flag, and the token side resolves the same name, so argv and env agree); a configured custom registry routes its resume probes and visibility waits through the cargo configuration the spawned tool itself reads — the `[registries.<name>]` `index` key (the `sparse+` HTTP form) or the `CARGO_REGISTRIES_<NAME>_INDEX` override (M5.2 continued, ADR-0021) — while an unconfigured name, a git-protocol index, a malformed config or a set-but-empty override refuses `E-PUB-REGISTRY` before any spawn, with the case named — and the routing reads the legacy `.cargo/config` filename too, with the Cargo Book's documented precedence (the file without the extension wins when both exist); the npm side routes per package — each npm target's `publishConfig.registry` / `npm_config_@<scope>:registry` (the probe-verified literal-key env form) / `NPM_CONFIG_REGISTRY` / `.npmrc` chain (the configuration the spawned `pnpm publish` reads, VERIFIED) resolves its probe base — the scoped key governs a scoped package regardless of the source tier — a custom base joins the map as a per-target override and a non-HTTP value refuses naming the package; the auth resolution rides the OIDC trusted-publishing fallback (M5.3 step 3, ADR-0031) — under a GitHub Actions OIDC context (`id-token: write`) a live run exchanges the short-lived crates.io token when no documented variable covers the registry, the dry forecast projects `oidc:<audience>` provenance without network, and a half-configured context refuses `E-PUB-AUTH` before any spawn |
| `migrate` | available (M4.7 step 1, ADR-0022 + ADR-0024): the release-please (`release-please-config.json` + `.release-please-manifest.json`), changesets (`.changeset/config.json`), knope (`knope.toml`) and release-plz (`release-plz.toml` / `.release-plz.toml` / `.config/release-plz.toml`, the documented top-down search order) configurations are surveyed against the `halyard.toml` v1 surface — every mappable foreign key is a mapped row with the pinned value (the foreign tool's documented defaults preserved where they differ from halyard's, the note saying which side is the foreign default), every semantic difference is an unmapped row with its reason, and a validated draft `halyard.toml` rides the report; `--write` lands the reviewed draft at `halyard.toml` through the engine's `DraftSink` — atomic (temp sibling → fsync → rename), create-only: an existing config is never clobbered and two configured tools are a refusal naming every candidate, never a guess (`E-CFG-MIGRATE`; nothing configured is an honest `nothing_configured` no-op; ADR-0023); without the flag the command stays report-only; needs no git repo and no existing `halyard.toml`; a configured-but-unparseable foreign file refuses fail-closed `E-CFG-MIGRATE`, an absent one is `not detected` in the report |
| `init [--github]` | available (ADR-0029, §16.1/§16.2): discovery surveys the repository and the engine plans a create-only write — the documented `halyard.toml` skeleton (every key optional; the defaults ride as comments and loading it yields the built-in defaults exactly, so zero-config stays zero-config) and, with `--github`, the §16.1 one-file release workflow (`.github/workflows/release.yml`); atomic (temp sibling → fsync → rename) with the workflow's parent directories created; gated before any write — a refusal never leaves a half-scaffold (`E-CFG-INIT`); an existing `.config/halyard.toml` refuses first (a root scaffold would shadow it); needs no git repo and reads no config |
| `man [<command>]` | available (§16.2 shell integration): `halyard man` renders the `halyard(1)` page and `halyard man <command>` one subcommand's page — roff, derived from the clap definition itself (the completions renderer's single-source-of-truth rule, so a flag added to the parser appears on the page without a second edit), byte-stable across runs (I3), printed to stdout (the `schema`/`completions` convention); an unknown topic is clap's own usage error naming the valid values |
| `bench` / `action` | **(their milestones)** — absent rather than stubbed (§16.2) |

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

## Publish auth (M5.3 step 1, ADR-0019)

The `publish` spawn resolves credentials through the VERIFIED tools' own
env grammar — halyard never writes auth files (pnpm's project-`.npmrc`
anti-exfiltration rule is exactly why):

```bash
export CARGO_REGISTRY_TOKEN=…          # crates.io (Cargo Book; singular form)
export CARGO_REGISTRIES_<NAME>_TOKEN=… # other cargo registries (NAME uppercased,
                                       # dots/dashes → underscores)
export NPM_TOKEN=…                     # npm (the user-level auth-file ${NPM_TOKEN}
export NODE_AUTH_TOKEN=…               #   interpolation; setup-node's convention)

halyard doctor     # the publish-auth row: which variable authenticates each registry
halyard publish --dry --format json | jq '.auth'   # the same provenance in the forecast
```

Absent variables are honest (`none` — the tools' own credential stores,
`cargo login` / the user-level auth file, govern); a **set-but-empty**
variable fails closed (`E-PUB-AUTH`) before any spawn, and every token
resolved is scrubbed from tool output before it reaches a report row.

### OIDC trusted publishing (M5.3 step 3, ADR-0031)

Under a GitHub Actions workflow with `permissions: id-token: write`,
a live publish exchanges the runner's OIDC id-token for a short-lived
crates.io token (audience = the registry URL sans scheme,
`POST {url}/api/v1/trusted_publishing/tokens`, VERIFIED in
`docs/research/oidc-trusted-publishing.md`) and injects it as
`CARGO_REGISTRY_TOKEN` — the documented env grammar keeps precedence
(an explicit token is never overridden), the exchange is memoized per
run, and the provenance spells `oidc:crates.io` (names only). The dry
forecast projects the same provenance without any network; a
half-configured or present-but-empty OIDC context refuses
`E-PUB-AUTH` before any spawn (both modes), and `doctor`'s
`publish-oidc` row reports the detected context. Trusted publishing is
GitHub-Actions + crates.io today: npm targets and custom registries
keep the env grammar.

## Check the commits (M2, §16.2)

```bash
halyard check --commits --format json   # the pending history's own subjects, linted
halyard check --commits --exit-code     # the release gate (exit 3) when pending
```

The merge-commit flow's counterpart to `check --pr-title`: the same
title machinery over every pending commit subject, per-commit rows in
every format, and the typo gate refusing `E-CLI-COMMIT` — a near-miss
kind (`feet:` for `feat:`) silently drops a bump, so the lint refuses
instead. Mapped-to-no-bump subjects are informational rows; autosquash
(`fixup!`/`squash!`) subjects never reach the lint (§8.5 drops them at
the parser).

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
tool (`E-PUB-EFFECT`) with the stable `E-PUB-*` codes. The npm rows
carry the resolved `publish.npm.access`/`provenance` options
(ADR-0033); an explicit `provenance = true` live run refuses
`E-PUB-AUTH` outside a supported cloud runtime (the `doctor`
`publish-provenance` check shows the verdict).

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

## The CI-selector adapters (`affected --ci`, ADR-0034 + ADR-0035 + ADR-0036)

`halyard affected --ci turbo|nx|bazel|gha|gitlab|azure|teamcity` renders
the selection in the CI system's own grammar — turbo's `--filter=...pkg`
dependent filters (one per change root, unioned by turbo), nx's
`run-many --projects=a,b` list (the affected closure), bazel's
`rdeps(//..., //pkg/...)` query, GitHub Actions' matrix-output value
(the affected closure as one compact JSON array, fed to `fromJSON`
inside the downstream `strategy.matrix`), GitLab's `parallel:matrix`
hashes (one `- HALYARD_PKG: <name>` line per affected package), Azure
Pipelines' `##vso[task.setvariable variable=HALYARD_PKGS;isOutput=true]`
output variable (the closure comma-joined; the documented consumption is
the `split(…, ',')` + `${{ each }}` example) and TeamCity's Matrix Build
`param("env.HALYARD_PKG", listOf(value(…), …))` line (pasted inside the
`matrix { }` block; each instance reads `$HALYARD_PKG`). The grammars
are VERIFIED in `docs/research/ci-selector-grammars.md`; an empty
selection prints nothing (the CI step skips itself, exit 0).
