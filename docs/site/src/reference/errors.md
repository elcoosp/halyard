# Error codes

Every user-facing error variant carries a stable code of the form
`E-<AREA>-<NAME>`. The rendered output goes through `miette` with the code
as the report code, and each code has a page in the repository's
`docs/errors/` directory with four parts: what it means, the cause, the fix,
and a copy-pasteable command to diagnose or recover. The code is the machine
surface (stable across releases), the page is the human one.

## The areas

| Family | Covers |
|---|---|
| `E-CFG-*` | the config loader: unknown keys, tag templates, env layer, channels, maintenance lines, init and migrate refusals |
| `E-DISC-*` | workspace discovery: the filesystem walk, manifest parsing, nested workspaces, member globs, polyglot links |
| `E-GRAPH-*` | the workspace dependency graph |
| `E-GIT-*` | the git engine: repo open, shallow clones, refs, objects, the tag write path, pushes, fetches, cache durability, the content-commit machinery |
| `E-PLAN-*` | the planner: cycles, `Release-As`, requirement rewrites, maintenance lines, change files |
| `E-EDIT-*` | the edit engine: patch validation, re-parse verification, atomic writes, unrewritable requirements |
| `E-LOCK-*` | the lockfile patch seam and the native refresh fallback |
| `E-CHLOG-*` | changelog styles and templates, the Release PR body, change files |
| `E-CACHE-*` | the persistent per-commit cache |
| `E-FORGE-*` | the forge seam: credentials, transport failures, rate limits, CAS conflicts, call budget |
| `E-REG-*` | the registry probes: names, transport, uninterpretable responses |
| `E-PUB-*` | the publish engine: plan validation, ordering, routing, tool failures, auth |
| `E-CLI-*` | the shell itself: working directory, rendering, lint refusals, live-mode declines |
| `E-ACTION-*` | the GitHub Actions output seam |
| `E-TEST-FIXTURE` | test-support fixture failures |

## Exit codes

| Code | Meaning |
|---|---|
| `0` | ok, or nothing to do |
| `1` | generic failure |
| `2` | usage/config error |
| `3` | plan has a pending release (for `check --exit-code`) |
| `4` | partial publish (retryable) |
| `5` | forge rate-limited (retryable) |

## The code table

The table below mirrors the `docs/errors/` index one-to-one — the
`check-docs-site` gate fails the build if the two sets drift apart. The
repository path column names the page file to open.

| Code | Area | In this repository |
|---|---|---|
| `E-CFG-UNKNOWN-KEY` | config loader (M1.8) | `docs/errors/E-CFG-UNKNOWN-KEY.md` |
| `E-CFG-TAG-STYLE` | config tag template (M1.8) | `docs/errors/E-CFG-TAG-STYLE.md` |
| `E-CFG-TOML` | config read/parse (M1.8) | `docs/errors/E-CFG-TOML.md` |
| `E-CFG-ENV` | config env layer (M1.8) | `docs/errors/E-CFG-ENV.md` |
| `E-CFG-MIGRATE` | foreign-config migration survey + `--write` refusals (M4.7) | `docs/errors/E-CFG-MIGRATE.md` |
| `E-CFG-CHANNEL` | config prerelease channel name — repo-wide, branch bindings, `[[package]]` overrides (§10.3, ADR-0025/ADR-0026) | `docs/errors/E-CFG-CHANNEL.md` |
| `E-EDIT-PATCHES` | edit patch validation (M1.4) | `docs/errors/E-EDIT-PATCHES.md` |
| `E-EDIT-VERIFY` | edit re-parse verification (M1.4) | `docs/errors/E-EDIT-VERIFY.md` |
| `E-EDIT-IO` | edit atomic write (M1.4) | `docs/errors/E-EDIT-IO.md` |
| `E-EDIT-UNREWRITABLE` | version requirement rewriter (M1.3, §A.2) | `docs/errors/E-EDIT-UNREWRITABLE.md` |
| `E-TEST-FIXTURE` | test-support fixture failures | `docs/errors/E-TEST-FIXTURE.md` |
| `E-DISC-IO` | discovery filesystem walk (M1.5) | `docs/errors/E-DISC-IO.md` |
| `E-DISC-MANIFEST` | discovery manifest parse (M1.5) | `docs/errors/E-DISC-MANIFEST.md` |
| `E-DISC-NESTED-WS` | discovery nested workspaces (M1.5) | `docs/errors/E-DISC-NESTED-WS.md` |
| `E-DISC-GLOB` | discovery member globs (M1.5) | `docs/errors/E-DISC-GLOB.md` |
| `E-DISC-NO-PACKAGE` | discovery empty repo (M1.5) | `docs/errors/E-DISC-NO-PACKAGE.md` |
| `E-DISC-DUPLICATE` | discovery duplicate members (M1.5) | `docs/errors/E-DISC-DUPLICATE.md` |
| `E-DISC-LINK-UNKNOWN` | discovery polyglot links (M1.7) | `docs/errors/E-DISC-LINK-UNKNOWN.md` |
| `E-GRAPH-CYCLE` | workspace graph cycles (M1.7) | `docs/errors/E-GRAPH-CYCLE.md` |
| `E-GIT-OPEN` | git engine repo open (M2) | `docs/errors/E-GIT-OPEN.md` |
| `E-GIT-HEAD` | git engine unborn HEAD (M2) | `docs/errors/E-GIT-HEAD.md` |
| `E-GIT-SHALLOW` | git engine shallow clone (M2) | `docs/errors/E-GIT-SHALLOW.md` |
| `E-GIT-REF` | git engine ref database (M2) | `docs/errors/E-GIT-REF.md` |
| `E-GIT-OBJECT` | git engine object reads (M2) | `docs/errors/E-GIT-OBJECT.md` |
| `E-GIT-TAG` | git engine tag write path (M4.2b, §11.1 step 2) | `docs/errors/E-GIT-TAG.md` |
| `E-GIT-TAG-CONFLICT` | git engine release-tag conflict (M4.2b, §11.1 step 2) | `docs/errors/E-GIT-TAG-CONFLICT.md` |
| `E-GIT-PUSH` | git engine tag-push effect (§11.1 step 2 / §11.2) | `docs/errors/E-GIT-PUSH.md` |
| `E-GIT-FETCH` | git engine smart-minimal-fetch effect (§8.8, M4.6 step 1) | `docs/errors/E-GIT-FETCH.md` |
| `E-GIT-CACHE` | git engine cache write-back durability (§8.7) | `docs/errors/E-GIT-CACHE.md` |
| `E-GIT-CONTENT` | git engine §11.1 step-3 content machinery (M4.2 remainder) | `docs/errors/E-GIT-CONTENT.md` |
| `E-PLAN-CYCLE` | planner cycle (M1.9) | `docs/errors/E-PLAN-CYCLE.md` |
| `E-PLAN-RELEASE-AS` | planner Release-As footer (M1.9) | `docs/errors/E-PLAN-RELEASE-AS.md` |
| `E-PLAN-UNREWRITABLE` | planner cascade rewriter (M1.9, A.2) | `docs/errors/E-PLAN-UNREWRITABLE.md` |
| `E-PLAN-MAINTENANCE` | planner maintenance-line guard (§10.3, ADR-0027) | `docs/errors/E-PLAN-MAINTENANCE.md` |
| `E-PLAN-CHANGE-FILE` | planner change-file unknown package (§9.4, ADR-0028) | `docs/errors/E-PLAN-CHANGE-FILE.md` |
| `E-CFG-MAINTENANCE` | config maintenance-branch entry (§10.3, ADR-0027) | `docs/errors/E-CFG-MAINTENANCE.md` |
| `E-CFG-INIT` | the `halyard init` zero-config scaffold refusals (§16.1/§16.2, ADR-0029) | `docs/errors/E-CFG-INIT.md` |
| `E-LOCK-PARSE` | lockfile patch seam (M3.2, §9.2 · M3.4, §9.3) | `docs/errors/E-LOCK-PARSE.md` |
| `E-LOCK-NATIVE` | native lockfile refresh fallback (M3.2, §9.2 rule 4 · M3.4, §9.3) | `docs/errors/E-LOCK-NATIVE.md` |
| `E-CACHE-OPEN` | cache open (M2.5) | `docs/errors/E-CACHE-OPEN.md` |
| `E-CACHE-HEADER` | cache header validation (M2.5) | `docs/errors/E-CACHE-HEADER.md` |
| `E-CACHE-IO` | cache read/write/fsync (M2.5) | `docs/errors/E-CACHE-IO.md` |
| `E-CACHE-LOCK` | cache advisory lock (M2.5) | `docs/errors/E-CACHE-LOCK.md` |
| `E-CHLOG-STYLE` | changelog style (M3.x) | `docs/errors/E-CHLOG-STYLE.md` |
| `E-CHLOG-TEMPLATE` | changelog user template (M3.5) | `docs/errors/E-CHLOG-TEMPLATE.md` |
| `E-CHLOG-FILE` | changelog existing file (M3.x) | `docs/errors/E-CHLOG-FILE.md` |
| `E-CHLOG-PRBODY` | Release-PR body / embedded plan (M4.3a, §11.3) | `docs/errors/E-CHLOG-PRBODY.md` |
| `E-CHLOG-CHANGE-FILE` | change-file front-matter grammar (§9.4, ADR-0028) | `docs/errors/E-CHLOG-CHANGE-FILE.md` |
| `E-CLI-CWD` | CLI -C handling (M1.10) | `docs/errors/E-CLI-CWD.md` |
| `E-CLI-BENCH` | CLI `bench` zero-iteration refusal (§16.2, ADR-0042) | `docs/errors/E-CLI-BENCH.md` |
| `E-CLI-RENDER` | CLI stable output rendering (M1.10) | `docs/errors/E-CLI-RENDER.md` |
| `E-CLI-NO-PACKAGE` | CLI explain target (M1.10) | `docs/errors/E-CLI-NO-PACKAGE.md` |
| `E-CLI-TITLE` | CLI PR-title lint (M4.3 `check`, §11.6) | `docs/errors/E-CLI-TITLE.md` |
| `E-CLI-COMMIT` | pending commit type is a near-miss of a known release type — the `check --commits` typo gate refuses | `docs/errors/E-CLI-COMMIT.md` |
| `E-CLI-COMMENT` | CLI check-comment mode (M4.5 step 1, §11.6) | `docs/errors/E-CLI-COMMENT.md` |
| `E-CLI-PR` | CLI live Release-PR decline (M4.2b `pr --dry`) | `docs/errors/E-CLI-PR.md` |
| `E-CLI-RELEASE` | **retired** — the live release shell reports tag/push rows through their engine codes (`E-GIT-*`, `E-FORGE-*`); no code emits this since M4.2b landed, kept for history | `docs/errors/E-CLI-RELEASE.md` |
| `E-FORGE-AUTH` | forge credentials (M4.1a, §11.4; token + App modes since M4.1c) | `docs/errors/E-FORGE-AUTH.md` |
| `E-FORGE-TRANSPORT` | Forge transport failure — connection, 5xx, unreadable response (M4.1b) | `docs/errors/E-FORGE-TRANSPORT.md` |
| `E-FORGE-RATE-LIMIT` | forge throttling (M4.1a, exit code 5) | `docs/errors/E-FORGE-RATE-LIMIT.md` |
| `E-FORGE-CONFLICT` | forge CAS precondition (M4.1a, §11.2) | `docs/errors/E-FORGE-CONFLICT.md` |
| `E-FORGE-REQUEST` | forge spec validation (M4.1a) | `docs/errors/E-FORGE-REQUEST.md` |
| `E-FORGE-UNSUPPORTED` | forge capability decline (M4.1a) | `docs/errors/E-FORGE-UNSUPPORTED.md` |
| `E-FORGE-BUDGET` | per-run forge call budget exhausted by the governor (M5.5, §11.5) | `docs/errors/E-FORGE-BUDGET.md` |
| `E-REG-NAME` | registry probe invalid package name (M5.1, §13) | `docs/errors/E-REG-NAME.md` |
| `E-REG-IO` | registry probe transport failure (M5.1, §13) | `docs/errors/E-REG-IO.md` |
| `E-REG-PROTOCOL` | registry probe uninterpretable response (M5.1, §13) | `docs/errors/E-REG-PROTOCOL.md` |
| `E-PUB-PLAN` | publish plan validation refused (M5.2, §13/I8) | `docs/errors/E-PUB-PLAN.md` |
| `E-PUB-ORDER` | publish target dependency cycle (M5.2, §13 step 1) | `docs/errors/E-PUB-ORDER.md` |
| `E-PUB-REGISTRY` | publish registry routing gap (M5.2, §13 step 2) | `docs/errors/E-PUB-REGISTRY.md` |
| `E-PUB-EFFECT` | publish tool failure row (M5.2, §13.7) | `docs/errors/E-PUB-EFFECT.md` |
| `E-PUB-AUTH` | publish token present-but-empty refused before any spawn (M5.3 step 1, ADR-0019); the OIDC context detection + exchange refusals (M5.3 step 3, ADR-0031); the npm provenance runtime refusal (M5.3 step 6, ADR-0033) | `docs/errors/E-PUB-AUTH.md` |
| `E-CLI-PUBLISH` | `publish` live mode contradicts `--offline` (M5.2) | `docs/errors/E-CLI-PUBLISH.md` |
| `E-ACTION-OUTPUT` | action-output seam: an output name/value the GITHUB_OUTPUT grammar cannot carry verbatim, or the sink is unwritable (§12 step 1, ADR-0037) | `docs/errors/E-ACTION-OUTPUT.md` |
| `E-ACTION-SUMMARY` | action-output seam: the §12 job summary has no GITHUB_STEP_SUMMARY sink or the sink is unwritable (§12 step 1, ADR-0037) | `docs/errors/E-ACTION-SUMMARY.md` |
| `E-ACTION-PLAN` | action-output seam: the canonical plan file cannot be written (§12 step 1, ADR-0037) | `docs/errors/E-ACTION-PLAN.md` |
