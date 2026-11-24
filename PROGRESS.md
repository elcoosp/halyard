# HALYARD progress

Append-only record: the next agent (or human) resumes from the last row.
Status values: `not started` / `partial` / `done` / `blocked`.

| Milestone | Status | Branch | Tasks done | Tasks deferred | Notes |
| --------- | ------ | ------ | ---------- | -------------- | ----- |
| M0 | done | feat/m0-bootstrap | M0.1, M0.2, M0.4, M0.5, M0.6, M0.7 | — | 2026-11-12; see below for scope notes on M0.3 |
| M0.3 | partial | feat/m0-bootstrap | harness (`xtask bench-competitors`, tools/claims/README, REPORT.md generated) | measured runs for release-please (needs forge API + token), release-plz & sampo (not installed, argv unobserved) | 2026-11-12; cocogitto 7.0.0 + knope 0.23.2 measured on the `fd` fixture; findings in `docs/research/competitor-cli-surfaces.md` |
| M1 | not started | — | — | — | depends on M0 done; next session starts at M1.1 (core types, interner, SmallBitset) |
| M1 slice A (foundation) | partial | feat/m1-foundation | M1.1, M1.2, M1.3, M1.4, M1.8 | M1.5, M1.6 (discovery), M1.7 (graph), M1.9 (planner), M1.10 (CLI) — slice B then slice C, next session; reason: single-session budget, slice A kept green end-to-end instead of half-wiring discovery/planning | 2026-11-12; full inventory + evidence in `.github/pr-body.md` (session log below) |
| M1 slice C (planner+git+cli) | partial | feat/m1-planner-m2-history | M1.9 (planner v1: pure plan(), cascade fixpoint, SCC-safe, reason chains, goldens+proptests green), M2.1 (ref enumeration + tag matcher + base resolution; differential vs `git merge-base` green on 5 fixture repos + scoped/annotated/packed cases), M2.2 (painter walk §8.3; differential vs `git rev-list` green on 10 fixture repos + 60 randomized samples; dhat 12/commit no-graph, 5/commit with commit-graph), M1.10 CLI implemented (8 commands, human/json/gha, doctor, profile) but unverified end-to-end, M3.5 keep-a-changelog renderer implemented with idempotence goldens | full gate battery NOT re-run in-session (context budget exhausted: fmt/clippy/nextest/deny/audit/doc + CLI snapshot tests + 10k-sample git-diff campaign deferred to next session); `docs/errors/` pages for E-PLAN-*/E-GIT-* not yet written; Slice-B latent `RelPath("")` bug found and fixed (package.rs, empty path = repo root) | 2026-11-12; see the slice-C session log below |
| M1 slice B (discovery) | partial | feat/m1-discovery | M1.5, M1.6, M1.7 (code + tests + benches + examples + docs landed; workspace compiles clean, check-layers ✓ 10 crates, schema-check ✓, discover-diff gate in xtask) | gate battery verification (full `cargo test --workspace` run, clippy pedantic `-D warnings`, deny/audit re-run, fuzz `json_span` execution, corpus fetch + `discover-diff` measured runs) unfinished in-session: fresh sandbox had no Rust toolchain (installed stable 1.99.1 from scratch) and no local clone; `TASK.txt` not present in the clone — protocol reconstructed from `docs/HALYARD_ARCHITECTURE.md` + the session prompt | 2026-11-12; see the slice-B session log below |
| M2 slice D (tree diff, cache, perf) | partial | feat/m2-tree-diff-cache-perf | M2.3/M2.4/M2.5 implemented: `halyard-core::trie` (build_trie + layout_hash, 4 tests), `halyard-git::tree_diff` (trie-guided lockstep walk, memo LRU, stats counters), `halyard-git::bloom` (BIDX/BDAT changed-path reader, murmur3 v2, verified against git's bloom.c), `halyard-git::merge` (MergePolicy FirstParentDiff/FirstParentMergeMessage/Off, in-window revert cancellation, out-of-window→fix, 6 tests), `halyard-cache` crate (append-only checksummed mmap cache, per-op flock, failpoint + real-SIGKILL kill-during-write tests, Miri/loom suites, dump example — 15 tests green incl. loom models), `halyard-conventional` revert_target/trailing_pr_number + golden/totality tests, `GitHistory::build_with_options` wiring (cache consulted before tree diff, attribution ∩ painted window), check-layers close-out (rules for git/changelog/cache + manifest regression test), broken Slice C example fixed | M2.6 NOT started (dhat re-audit/iai-callgrind/perf-gate extension/cache-check/pgo/CLI profile spans), D0 remainder (schema-check-plan + git-diff xtask subcommands, fuzz targets changelog_render/trie_diff/cache_read, docs/errors E-PLAN-*/E-GIT-*/E-CHLOG-*/E-CLI-*/E-CACHE-* pages, CLI README + snapshot tests, changelog golden tests + example, CI steps), the full Step-5 gate battery re-run, M2.7 replay (SKIP: corpus not fetched); halyard-git lib tests: 8/11 green at session end (3 bloom unit tests + a glob-path unit test hang/fail to triage — see the slice-D session log below); integration engine tests not re-run after the attribution change | 2026-11-12; see the slice-D session log below |

| M3 apply slice (M3.1/M3.3/M3.6) | partial | main | M3.1 Cargo manifest patcher (`halyard-eco-cargo/src/edit.rs`: byte-level span patcher, quote-inclusive spans, dep-kind + `[workspace.dependencies]` + `[target.*]` sections, own `[package]` version, workspace-inherited rows skipped-and-counted, `E-EDIT-UNREWRITABLE` fail-closed, formatting/CRLF preserved), M3.3 package.json patcher (`halyard-eco-npm/src/edit.rs` + `JsonDoc::replace_span`; `catalog:`/`workspace:`/`link:`/`file:` skip-and-count via the M1.6 `classify_req`), `EcosystemAdapter::edit_versions` seam in core + both adapters (check-layers green, 14 crates), M3.6 `halyard-edit/src/plan_apply.rs` (`edit_set_from_plan`, `dry_run_preview`, `apply_plan` — staged in memory then atomic writes) + CLI `apply [--dry]` | M3.2 Cargo.lock patcher and M3.4 pnpm-lock patcher (lockfile slice, not started); perf-gate honest misses recorded (plan-200x40 p50 38 ms vs 25 ms budget; plan-1000x50k p50 1022 ms vs 400 ms — sandbox CPU ceiling, budgets untouched); bench byte-determinism impossible for wall-clock timings (only `docs/replay.md` is byte-gated) | 2026-11-13; clippy 0, tests 321/0, gate battery green (replay 6/6 = 100 % on conventional repos + byte-identical ×2, discover-diff 4/4 cargo, dhat 12/5 within bounds, git-diff attribution ✓); session log below |
| M3.2 lockfile slice (patcher + native fallback) | done | main | M3.2 Cargo.lock patcher (`halyard-eco-cargo/src/lockfile.rs`: §9.2 rules 1–3, byte-in place, read-back verify, idempotent) + `EcosystemAdapter::patch_lockfile` seam (core `lockfile.rs`; default = documented `NeedsNative` fallback) + `apply_plan` lockfile stage (`LockfileSummary` in the report; decline reported, never guessed) + CLI render/JSON rows, M3.2 native fallback (`EcosystemAdapter::refresh_lockfile_native` + `verify_native_refresh` post-refresh gate + CLI orchestration, §9.2 rule 4), criterion bench (`lockfile_patch`), consumer example (`patch_lockfile.rs`), differential vs `cargo update -w --offline` byte-identical | M3.4 pnpm-lock patcher still open; the full §17 M3.2 acceptance runs "200 mutated fixtures" — this slice ships the differential harness with 1 fixture + unit-matrix coverage (scaling the fixture count is an xtask/nightly task); perf budgets untouched | 2026-11-13; see the session log below |
| M3.4 pnpm-lock slice (patcher + native fallback) | done | feat/pnpm-lock-patcher | M3.4 `pnpm-lock.yaml` patcher (`halyard-eco-npm/src/lockfile.rs`: restricted importer-grammar scanner, specifier advance through the canonical `rewrite_requirement`, `<version>(link:…)` ref moves, importers-only byte patches, read-back re-analysis verify, idempotent) + `NpmAdapter::patch_lockfile` routing (package-lock.json/yarn.lock/bun.lock native-only declines) + `NpmAdapter::refresh_lockfile_native` dispatch (`pnpm install --lockfile-only --offline` / `npm install --package-lock-only` / `yarn install --mode update-lockfile` / `bun install --lockfile-only`; pure `refresh_command` table) + pnpm `verify_native_refresh` (importer-row snapshots, resolution-key/header changes fail closed) + core `LockfileVersionChange::from_release_for_ecosystem` + generalized `apply_plan` lockfile stage (`ApplyReport.lockfiles`, per-ecosystem rows incl. `ecosystem`) + CLI render/JSON rows and per-declined-row native refresh with verifier dispatch + criterion bench `lockfile_patch` + consumer example `patch_pnpm_lockfile` + differential vs real pnpm (skips honestly when pnpm absent) + CLI `--package` edit filter now resolves package roots from the workspace (was a hardcoded `crates/…` layout) | M3.4 §9.3 acceptance runs "pnpm differential on corpus" — this slice ships the workspace-protocol + linked-plain-semver differential harness (runs when a pnpm binary is present); the corpus-wide pnpm differential and the npm/yarn/bun lockfile patchers (native-only in v1 by design) remain open; perf budgets untouched | 2026-11-13; see the session log below |
| M3.5b changelog styles slice (style registry + conventional/compact) | done | feat/pnpm-lock-patcher | M3.5b `Style` registry (`halyard-changelog`: `Style::{parse,name}` replacing the `style != "keep-a-changelog"` string check; `render_release`/`render_plan` dispatch through it), `conventional` style (planner section titles mapped through the documented type map — Features→feat, Bug Fixes/Fixed→fix, Performance→perf, Reverts→revert, unknown pass through verbatim; `### ⚠ BREAKING CHANGES` first), `compact` style (flat bullet list, breaking entries first), both idempotent with goldens; `github-release-notes` stays unshipped (needs forge context — documented); E-CHLOG-STYLE message + page updated to name the shipped set; `render` example now renders all three styles | minijinja template path (user-supplied template files) still open; `github-release-notes` waits for the forge milestone | 2026-11-13; see the session log below |
| M4.1a forge seam slice (trait + reference adapter + taxonomy) | done | main | M4.1a `halyard-forge` crate (ADR-0011): the §11.4 `Forge` trait (object-safe, `&dyn`), the data vocabulary (`Pr`/`PrSpec`/`Release`/`ReleaseSpec`/`ApiCommit`/`ApiFile`/`BranchName`/`RateInfo`), the `E-FORGE-*` taxonomy with docs pages, the deterministic `MemoryForge` reference adapter (find-by-branch upserts, GET-by-tag-first releases, synthetic `fe`-prefixed commits, call log, failpoint harness), `commit_via_api` default honest decline, contract tests, `memory_forge` bench, `release_lifecycle` example, test-support re-exports, layer rule `forge → core` | M4.1b GitHub REST adapter (ETag/backoff/App-JWT, wiremock) open — nightly `forge-smoke` stays latent; PROGRESS row + README status were not updated when the slice landed (drift, repaired 2026-11-13) | 2026-11-13 (committed before the M4.2a/M4.3a session; row added retroactively) |
| M4.2a state machine + M4.3a PR body slice | done | main (dirty; no commits) | M4.2a `halyard-forge::state` (`run_release_machine` over `&dyn Forge`: §11.1 tag phase with GET-by-tag-first idempotence + PROPAGATE gating, prepare phase find→upsert/`PrUnchanged` no-op/close; §11.5 call budgets counted from the adapter log; fail-closed input validation) + `Forge::close_pr` seam (default honest decline, §11.4 not-found-is-a-value convention) + `MemoryForge` open/closed PR modeling (closed PRs not re-found, upsert-after-close opens fresh) + 12 machine tests incl. the §15.2 failure-injection matrix (fail at every operation → re-run reaches the uninterrupted state, zero duplicates) + `state_machine` bench (5 pass shapes) + `release_machine` example; M4.3a `halyard-changelog::pr_body` (§11.3 body: plan table with deterministic Why map, Skipped line §16.3, changelog preview, override footer, embedded plan = canonical JSON → DEFLATE → base64 with recomputed `plan_hash`; `extract_embedded_plan` I8 verifier fails closed on missing/corrupt/hash-mismatched markers, `E-CHLOG-PRBODY` + docs page) + 10 tests + golden snapshot + `render_pr_body` example + forge example wired through the renderer (dev-dep) + layer rule widened (`changelog`: minijinja/serde_json/flate2/base64) | M4.1b GitHub REST adapter; M4.2b shell git-side tag/push effects (annotated tags, `--force-with-lease` CAS) and the CLI `pr`/`release` commands that consume the machine; corpus-wide pnpm differential xtask; M2.6 perf-budget close-out | 2026-11-13; see the session log below |
| M4.2b step 1 + M4.3 `check` slice (pr --dry forecast + title lint) | done | main (dirty; no commits) | M4.2b step 1: `halyard-forge::preview::PreviewForge` (dry-run `Forge` adapter: probes see an empty forge, mutations are `preview://`-marked forecasts, §11.5 call log observable, `commit_via_api` declines per the seam default) + 9 contract tests incl. the machine composition (open forecast = find+upsert 2 calls, empty plan = 1 probe, tag phase PROPAGATE forecast), `pr_preview` bench (3 pass shapes), `pr_preview` example, CLI `pr --dry` (plan → §11.3 body → machine pass; effects + call-budget + body in human/json/gha; `E-CLI-PR` honest decline + docs page for live mode), `tests/cli_pr.rs` (6 tests incl. determinism + JSON stability); M4.3 `check`: `halyard-conventional::title_lint` (`TitleBumpRule` seam + `lint_title` — planner-exact as-written kind lookup, fixed §10.1 breaking→major rule, subject-line-only judgement, did-you-mean only for kinds outside the rule's known set), 12 engine tests + `title_lint` bench + `lint_title` example, CLI `check.rs` (`PlanTitleRule` over `PlanConfig::commit_types`, per-package §10.2 pre-major prediction), `halyard check --pr-title` / `--exit-code` (§15.4 exit 3), `E-CLI-TITLE` + docs page, `tests/cli_check.rs` (9 tests) | M4.1b GitHub REST adapter (the nightly `forge-smoke` job stays latent), the live `pr`/`release` shell effects (git-side tag/push), the tag-phase manifest-vs-tags reconciliation in `pr --dry` (declared not-forecast in the report), `github-release-notes` changelog style (needs forge context), M2.6 perf close-out (`--no-cache` still a forward-compat no-op — help text now states it precisely) | 2026-11-13; see the session log below |
| M1 reconciliation slice (§11.1 tag phase) + M2.6 CLI cache wiring | done | feat/tag-phase-reconciliation (dirty; no commits) | M1: `halyard_core::release_state` (the §11.1 step-2 stateless manifest-vs-tags reconciliation: `reconcile_tags` → Tagged/Pending/Behind rows, `pending_releases` → the deduplicated tag-phase queue; template resolution shared with the planner (`effective_template`, config override → single-package default → discovery template), `release = false` skip set honored, version-less packages skipped, output sorted by package name, 11 integration + 2 inline tests) + `GitHistory::tags()` accessor (the enumerated §8.2 candidates no longer dropped) + CLI `pr --dry` wired to the real reconciliation (the tag phase is forecast, PROPAGATE visible, Behind rows reported never tagged; JSON carries `pending_releases` + `propagated`; `cli_pr.rs` re-pinned, 10 tests) + `release_state` criterion bench + `pending_releases` example; M2.6: CLI pipeline opens `$GIT_DIR/halyard/cache/v1/` and feeds `GitHistory::build_with_options` with the trie `layout_hash` + resolved `config_hash`, `--no-cache` is real, a corrupt/unopenable cache degrades to an uncached run with a warning (`cli_cache.rs`, 3 tests) + `cache` `roundtrip` bench + `cached_walk` example. Micro-debt: dead `--color` policy computation removed (flag documented as reserved for the §16.3 renderers), non-UTF-8 tag-name skip made explicit in `refs.rs`, the load-bearing empty-default in `tree_diff.rs::path_matches` documented. Docs drift fixed: CLI README (`--dry-run`/`--refresh-lockfile` were wrong, stale status table), root README status + pr --dry description, `Command` enum doc (pr/check exist), render.rs "tag phase: not forecast" line replaced by the real forecast | M4.1b GitHub REST adapter, the live `pr`/`release` shell effects, `github-release-notes` style, M2.6 perf-budget close-out beyond the wiring (dhat re-audit vs the budgets), the corpus-wide pnpm differential as an xtask subcommand | 2026-11-14; see the session log below (correction: the slice landed as commits `bb2be7a`..`ebb1dfd` on main — the "dirty; no commits" branch claim was stale) |
| Tag-phase write path + forecast slice (§11.1 git-side effects) | done | main (landed as `0d0a9da..57d3864`) | M1: `halyard-git::tag` — the git engine's first write path (the `TagWriter` seam, `GixTagWriter`: annotated tag objects + create-only ref updates via `gix`, validation fail-closed `E-GIT-TAG`, idempotence `AlreadyTagged` incl. lightweight tags, `E-GIT-TAG-CONFLICT` never moves a release tag, race reclassify on the create, `run_tag_phase` executor (sorted+dedup, continue-on-error, deterministic report, I2 resume shape), `directives_for_pending` (HEAD fallback target, documented)), 11 integration tests vs `git` ground truth + deterministic tag-object pin, `tag_write` bench (create ≈124 µs; already-64 re-run; forecast-64), `tag_phase` example, `E-GIT-TAG`/`E-GIT-TAG-CONFLICT` pages; M2: `PreviewTagWriter` (probes real — validation/target/existing-tag classification identical to the live run — mutations forecast with the documented neutral-identity shape oid, zero writes) + CLI `release --dry` (reconcile → directives → forecast, human/json/gha, `E-CLI-RELEASE` honest decline, byte-stable JSON), 4 preview tests + 5 CLI tests, `tag_forecast` example. Docs drift fixed: `E-CHLOG-TEMPLATE` page missing (code shipped without its page — added + indexed), xtask module doc table missing 5 shipped subcommands, arch doc §8.1 `max-performance-safe` gix feature drift noted as-built, CLI README + root README command/status rows, stale "dirty; no commits" branch claim in the previous row corrected; the rustdoc gate itself was red at HEAD (`reconcile_tags`' doc linked the private `effective_template` — the §15.6 doc job had never been run green) — de-linked, full `-D warnings` doc sweep green after the fix | M4.1b GitHub REST adapter, the live release shell (tag push + forge releases + publish), the §11.1 manifest-blob pinpoint walk for tag targets, `github-release-notes` style, M2.6 dhat re-audit, the corpus-wide pnpm differential | 2026-11-14; see the session log below |
| Tag-target pinpoint slice (§11.1 precision rule) + release-dry provenance | done | main (dirty; no commits) | M1: `halyard-git::tag_target` — the §11.1 precision rule as a seam (`TagTargetResolver` trait, `GixTagTargetResolver`: bounded first-parent walk on manifest blob-OID change from the tip to the package's release base, content-verified by a caller-supplied `ManifestVersionProbe` — the newest change event whose manifest carries V while its parent does not; identical-OID runs skip blob reads; unreadable objects (the `--filter=blob:none` case the pinpoint was deferred for, shallow cuts, exhausted bounds, never-committed versions) degrade to the documented HEAD fallback instead of guessing, only an unreadable request tip is `E-GIT-OBJECT`) + `directives_for_pending_resolved` glue (per-release manifest/ecosystem/base from the workspace + digest, stale names keep the fallback directive) + 11 integration tests vs `git` ground truth (pinpoint, later-manifest-edits guard, never-committed fallback, initial release to the root, real blob-less clone degradation, walk bound, no-ff merge landing, two packages independently, glue + stale names, probe ecosystem contract, determinism) + `tag_target` bench (pinpoint-500 ≈ 18.7 ms, fallback-500 ≈ 18.7 ms) + `tag_target` example; M2: CLI `release --dry` targets resolve through the seam (shell probe dispatches `halyard_eco_cargo::manifest_version` / `halyard_eco_npm::manifest_version`, inherited versions routed at their authoritative workspace manifest) with per-row provenance in json/human/gha (`manifest pinpoint` vs `HEAD fallback`), 3 new CLI tests (red→green cycle demonstrated against the pre-provenance renderer), `release_pipeline` example (the missing end-to-end consumer demo: discover → reconcile → pinpoint → forecast → machine). Micro-debt/docs drift fixed: the misleading `// crc placeholder` comment in `halyard-cache` (the CRC is patched in place three lines below), stale "dirty; no commits" branch claim on the previous row, arch doc as-built notes for §11.1 (pinpoint shipped), §11.4 (`close_pr`/`calls()` + the two adapters), §15.6 (wall-clock perf gate per ADR-0010, actual nightly lanes), §16.2 (wired commands vs the table), root README layout rows (schema/completions, schema-check-plan), CLI README `release --dry` row, nightly.yml `differential` job ran a nonexistent xtask subcommand — now runs the real differential test suites | M4.1b GitHub REST adapter, the live release shell (tag push CAS + forge releases + publish), `github-release-notes` style, M2.6 dhat re-audit, the corpus-wide pnpm differential | 2026-11-14; see the session log below |

| Live release shell (§11.1/§11.2 git side) + release-notes pipeline (§9.4) | done | feat/live-release-tag-phase (dirty; no commits) | M1: `halyard-git::push` — the §11.2 tag-push effect seam (`TagPusher` trait `&dyn`, `GixTagPusher`: one `git ls-remote --tags` probe → per-tag classification (already-on-remote I1 / divergent remote tag fail-closed `E-GIT-TAG-CONFLICT` / missing-local `E-GIT-PUSH`) → one `git push --porcelain` with per-ref outcome attribution; argv arrays, `GIT_TERMINAL_PROMPT=0`, no `--force`/`--delete`, subprocess rooted at the opened repo path; `PreviewTagPusher` forecasts `WouldPush` offline (I5); wire-format parsers pinned against probed empirical output) + `GitError::TagPush` (`E-GIT-PUSH` page + index) + CLI `release` live mode (annotated tags via the real writer, shell-owned tagger identity = gix config + wall clock, incomplete identity fails closed per directive; push phase over the seam, `--no-push` skips, missing remote is an honest skip; forge releases reported as deferred (M4.1b); failures exit 1, resumable I2) + `tag_push` bench (refspec/parse) + `tag_push` example + ADR-0012 + crate-doc as-built amendment (the planning read path stays subprocess-free; the §11.1 effect steps are the sanctioned exceptions); `release --dry` extended with the push forecast (still offline); M2: the release-notes pipeline (`halyard-changelog::release_notes`: `Style::GithubReleaseNotes` registered, `compare_url` normalizing the four GitHub remote shapes (foreign forges → no link), `render_release_notes` over the plan entry preview, `extract_version_section` + `render_release_notes_for_version` extracting the package changelog's own version section — the §11.1 tag phase's `create_release` payload; `pr --dry` fills `ReleaseSpec.notes` from the changelog section (plan-tag fallback, else honest empty) and the JSON carries per-release `notes`) + `release_notes` bench + `render_release_notes` example + 2 inline web_base tests | M4.1b GitHub REST adapter (forge releases live), the live `pr` mode, publish (M5), the minijinja user-template path, the M2.6 dhat re-audit, the corpus-wide pnpm differential | 2026-11-14; see the session log below |

| M4.1c App-JWT auth + walk-provenance reporting slice | partial | feat/github-app-jwt-auth (dirty; no commits) | M1 (M4.1c): `halyard-forge-github/src/app_auth.rs` — `AppCredentials` (PKCS#1/PKCS#8 PEM → `ring` RsaKeyPair, nine-INTEGER validation, fail-closed), `mint_jwt` (RS256, `iat`=now−60, `exp`=iat+600, injected clock), `ForgeCredentials::{Token,App}` + `kind()`, `EpochClock`/`system_epoch`, `INSTALLATION_TOKEN_TTL_SECS`=2700 (45-min conservative TTL, `expires_at` deliberately not parsed); the adapter's `bearer()` resolution + mint exchange (`POST /app/installations/{id}/access_tokens`, recorded §11.5); `wire::parse_installation_token`; 14 contract tests (JWT claims + RS256 round-trip verification, mint/probe/cache/re-mint, fail-closed matrix) + bench `app_auth` (mint ≈535 µs) + example `app_exchange`; CLI wire-up `HALYARD_APP_*` (partial set fails closed naming the gap), smoke/release/pr live paths, `doctor` auth-mode line, smoke JSON `auth_mode`; 2 loopback e2e tests; ADR-0014 + research §7 + E-FORGE-AUTH page/index + arch §11.4/§11.5 as-built + READMEs; layers.rs `ring` edge. M2: `render::HistoryStats` seam (tree-diff/merge counters + degraded/truncated) surfaced through plan human/GHA and pr/release JSON+human/GHA; 4 `cli_stats` tests; bench `history_stats` in halyard-git. Pre-existing reds repaired at base: 11 broken `Option::unwrap_or_else(|error|…)` closures in contract.rs (did not compile), 2 impossible human-render code assertions (forge-smoke/cli_pr), the hanging `pr_live` loopback fixture (pre-PROPAGATE expectations, `server.join()` blocked forever) | The CLI test/clippy tail (2 `cli_stats` human-line pins vs the precomputed-line refactor + 2 bin-test compile errors) unfinished — step budget cut mid-restructure; the engine + forge-github suites are fully green (the cli_pr half of that tail was repaired and committed post-session: `cb1a19e`, `ebda22b`; two warm-cache determinism pins in cli_pr remained red at HEAD — repaired this session, see below) | 2026-11-14; session log below |

| M5.5 call-governor slice (§11.5 pacing/retry/budget) | done | feat/forge-call-governor (dirty; no commits) | M1: `halyard_forge::governor` (ADR-0015) — the `Governor` `Forge` decorator (wraps any `&dyn Forge`, `Forge::calls` keeps delegating so the machine's §11.5 accounting is untouched): `GovernorPolicy` (spacing/budget/retries/backoff base+cap; `default()` passive, `shell_default()` the §11.5 production shape), content-call pacing start-to-start (probes + `rate_limit()` unpaced), `Retry-After` honored exactly, transport backoff deterministic `base × 2^attempt` capped (no jitter — the I3 deviation ADR-0015 documents), per-run budget as a forwarded-attempts ledger (failures + retries count) refusing fail-closed *before* the call with the new `ForgeError::Budget` (`E-FORGE-BUDGET` page + index row; non-retryable errors never wait; exhausted retries return the last error verbatim), injected `GovernorClock`/`GovernorSleeper` + `system_time()` production pair, deterministic `events()` log; 12 contract tests (`tests/governor.rs`, red→green on record) + bench `governor` (passthrough ≈894 ns, pacing decision ×16 ≈4.6 µs, budget block ≈598 ns) + example `governed_release` + E-FORGE-RATE-LIMIT as-built + arch §11.5 as-built. M2 (M1 green first): `governor_policy_from_env` (`HALYARD_FORGE_MIN_CALL_SPACING_MS`/`_MAX_CALLS`/`_MAX_RETRIES` over `shell_default()`, invalid → fail-closed `E-FORGE-REQUEST` naming the variable), the three live forge sites wrapped (`pr` live, the release forge half, `forge-smoke`), `render::GovernorSummary` (`line()`/`json()`) riding the live pr JSON document + human/gha tails (dry JSON schema byte-stable), `ForgeReleases::Ran` gains the governor summary, `doctor` gains the `forge-governor` check (resolved policy; invalid env fails the check); 4 loopback e2e tests in `cli_forge.rs` (summary row + budget block fail-closed with zero refused HTTP + env grammar naming the variable + smoke governor row; red→green on record) | Publish (M5), the corpus-wide pnpm differential, the M2.6 dhat re-audit, the `github-release-notes` PR-comment mode — all unchanged. Pre-existing reds repaired at base first: the two `cli_pr` warm-cache determinism pins now ride `--no-cache` (the cli_stats convention the M4.1c session left unfinished at HEAD) | 2026-11-14; session log below |

| §8.8 smart-minimal-fetch slice (M4.6 step 1) + M5.1 registry existence probes | done | feat/fetch-and-registry-probes (dirty; no commits) | M1 (M4.6 step 1): `halyard_git::fetch` (ADR-0016) — the pure planner (`latest_tags_for`, `deepen_boundaries_for` through the now-public `halyard_core::planner::effective_template` so fetch/plan/tag-phase cannot disagree, `plan_fetch`, `fetch_attempts`) + the live `TagFetcher`/`GixTagFetcher` effect (probe → select → classify → one minimal fetch, full-fetch fallback of the same refs, I1-idempotent, divergent release tags fail-closed `E-GIT-TAG-CONFLICT`); three recipe points settled empirically + pinned by tests (shallow machinery only on shallow repos — `--shallow-exclude` fails complete clones; the boundary is each package's previous release tag; `--deepen` loops deferred, the full fetch is the terminal fallback); `GitError::Fetch` (`E-GIT-FETCH` page + index); CLI `halyard fetch` (discovery+config only — a shallow clone must be fetchable; human/json/gha, exit 1 on failed rows) + 17 engine tests (`tests/fetch.rs`, red→green) + 4 CLI tests (`cli_fetch.rs`, red→green) + bench `fetch_plan` (selection 40×202 ≈ 596 µs, boundaries ≈ 594 µs, attempts 40 ≈ 12 µs) + example `fetch_plan` + ADR-0016 + arch §8.8/§16.2 as-built + README/CLI-README rows. M2 (M1 green first, M5.1 step 1): the `halyard-registry` crate (ADR-0017) — the `Registry` seam over the forge transport (no HTTP client; `UreqTransport` injected at runtime, `MemoryTransport` in tests; layer rule `registry → forge`, 17 crates ✓), `CratesIo` sparse-index + `Npm` packument `published()` with fail-closed status semantics (`404` ⇒ not published — verified live; uninterpretable ⇒ `E-REG-PROTOCOL`; names never reach the wire invalid ⇒ `E-REG-NAME`), `wait_until_visible` with the deterministic doubling-capped backoff + injected `RegistrySleeper`; URL grammar VERIFIED live (Cargo Book + npm registry docs + direct HTTP probes) and recorded in `docs/research/registry-existence-probes.md`; 12 contract tests (red→green) + bench `probe` (sparse parse ≈ 2.9 µs, packument ≈ 28.6 µs) + example `probe_registries` + `doctor --registries` opt-in probe (immutable-version probes; I5 respected) + `E-REG-*` pages ×3 + index rows | The M4.6 corpus checkout-time acceptance (corpus lane), the M5.1 nightly live checks (opt-in lane), the `--deepen` loops, the M5.2 publisher (declared consumer), publish/M5.2+, the corpus-wide pnpm differential, the M2.6 dhat re-audit. Micro-debt cleaned (additive): cache write-back failures remapped `E-GIT-OBJECT` → the new `GitError::Cache` (`E-GIT-CACHE` page — a durability failure is not an object-read error), the dead always-`None` `TagCandidate::version` field removed, the README daily-commands list completed (8 example rows drifted) | 2026-11-14; session log below |
| M5.2 publish engine + CLI `publish` slice (§13, ADR-0018) | done | (tree left dirty; no commits) | M1: the `halyard-publish` crate (the §7.1-reserved `halyard-publish/` slot) — `plan_publication` (the §13 step-1 pure planner: recomputed `plan_hash` gate I8, the releases × workspace join with fail-closed `E-PUB-PLAN` contradictions, name-sorted layers over the workspace graph's non-dev edges, `E-PUB-ORDER` with the rendered cycle path, dev-only cycles legal, `not_targets` observability), `run_publication` (the §13 steps-2/3 resume loop: routing resolved before any call (`E-PUB-REGISTRY`, zero side effects), the resume probe (`AlreadyPublished` = the I1 no-op), the injected `PublishEffect` tool seam, the §13 step-3 visibility wait, the §13.7 outcome model as rows (`Published{visibility_confirmed}` / `AlreadyPublished` / `Skipped{upstream_failed\|invisible\|skipped}` / `Failed{code,message,retryable}`), deterministic name-sorted report (I3) with derived tallies, bounded within-layer parallelism (`PublishPolicy::max_parallel`, scoped std threads, `Send + Sync` on the injected seams — `Registry`/`RegistrySleeper` widened in `halyard-registry`, a no-op for every shipped impl)), the reference fakes (`MemoryRegistry` with modelled index lag, `MemoryPublishEffect` with per-package failure scripts + `successes()` zero-duplicates witness, `MapRouter`), 30 tests (incl. the §15.2 failure-injected-at-every-step convergence matrix), bench `publish` (layering-40 ≈ 200 µs, engine-pass-40 ≈ 84 µs), example `publish_plan` (interrupted run → I2 resume → zero duplicates), ADR-0018 + `E-PUB-PLAN/ORDER/REGISTRY/EFFECT` pages ×4 + index rows + layer rule + arch §13 as-built. M2 (M1 green first): the CLI `publish [--dry \| live]` command — the adapter crates' argv tables (`cargo_publish_args`/`pnpm_publish_args`, VERIFIED against the Cargo Book and pnpm.io live, `docs/research/publish-tooling.md`), the engine's `PublishMode::{Live,Dry}` + `Forecast` outcome (the dry forecast runs the tools' dry form and never probes — I5), the shell's `SpawnPublishEffect` (argv arrays, processes rooted at the package root, `--manifest-path` resolved against the repo root — the cwd lesson the CLI tests caught), live registry routing over the blocking transport, json/human/gha renderers with row-level spawn provenance (`manifest`/`manifest_dir`), §15.4 exit 4 on partial publish, `E-CLI-PUBLISH` (live + `--offline` fails closed) + 5 CLI tests (shim-recorded argv, byte-stable JSON, routing fail-closed, disabled-config no-op, gated rows over a broken tool). Micro-debt/docs drift fixed (additive): the rustdoc gate was red at base — three pre-existing broken intra-doc links (`halyard-git/src/refs.rs` → `TagTemplate::extract_version`, `halyard-registry` → a nonexistent `crate::governor`, `halyard-forge/src/transport.rs` → `MemoryForge` out of scope) — de-linked/re-worded, full `-D warnings` doc sweep green; the xtask `every_current_member_has_a_rule` test list was stale (missing `halyard-forge-github`/`halyard-registry`) — extended with all current members | The M5.3 OIDC/auth plumbing, the `publish --resume` flag (the engine already resumes; the flag is UX), the M4.6 corpus checkout-time acceptance, the M5.1 nightly live checks, the corpus-wide pnpm differential (no pnpm in the sandbox), the M2.6 dhat re-audit, the two p50 perf-budget misses | 2026-11-14; session log below |

| M5.3 step 1 publish-auth plumbing + step 2 observability slice (ADR-0019) | done | (tree left dirty; no commits) | M1 (M5.3 step 1): `halyard_publish::auth` — the `PublishAuth` seam (object-safe, `Send + Sync`, `credentials(&target) → Result<AuthOutcome, PublishError>` + the `secrets()` scrubbing contract), the `EnvPublishAuth` production capture (one deterministic env snapshot shared by resolution and scrubbing; the `CARGO_REGISTRIES_*_TOKEN` scan covers custom cargo registries), the VERIFIED grammar as data (`variables_for`: cargo `CARGO_REGISTRY_TOKEN` + the `registries.crates-io` fallback / `CARGO_REGISTRIES_<NAME>_TOKEN` uppercased dots-dashes→underscores; npm `NPM_TOKEN` → `NODE_AUTH_TOKEN`; unknown ecosystems honest-absent), absent-is-a-value (`AuthOutcome::Absent` — the tools' own credential stores govern), present-but-empty refused fail-closed `PublishError::Auth` (`E-PUB-AUTH` + page + index row, structural never-retryable), `scrub_secrets` (pure, `[secret]` marker) + `CredentialEnv`/`EnvPublishAuth` Debug redaction, `auth_summary` provenance helper (M2); the CLI effect consumes the seam (`SpawnPublishEffect::new(root, dry, auth)`: the resolved pair injected via `Command::env`, every surfaced stderr tail scrubbed of all resolved values, `E-PUB-AUTH` rows), the run-level capture refuses before planning (`cmd_publish`), 19 engine tests + 5 CLI tests (scrub-leak, empty-token refusal, provenance JSON + byte-stability, doctor row) — one full red→green cycle per half, both reds on record; bench `auth` (capture ≈2.3 ns, credentials×14 ≈1.4 µs, scrub-1KiB ≈ sub-µs) + example `publish_auth` (provenance-only output) + ADR-0019 + `docs/research/publish-auth-env.md` (VERIFIED quotes: Cargo Book config + registry-authentication, pnpm.io/npmrc incl. the project-`.npmrc` non-interpolation anti-exfiltration rule) + arch §13 as-built. M2 (M1 green end-to-end first): the renderers carry the per-registry `auth` provenance (json `auth` block + human/gha `publish-auth (M5.3):` tail, names only, byte-stable) and `doctor` gains the `publish-auth` check (the same capture the run performs; empty-token fails the check). Micro-debt/docs drift fixed (additive, not milestone substitutes): the mangled `E-CLI-PUBLISH` message (stray space runs mid-sentence) rewritten with correct wrapping; the spawn's stderr tail picks the last **non-empty** line (`next_back()` on blank-line-terminated stderr produced an empty diagnosis); the `publish-tooling.md` deferral note ("--registry mapping … deferred to M5.3's auth plumbing") is now satisfied by the seam (the token env maps the §13 registry name verbatim; a cargo `--registry` config-name mapping stays a no-guess open item); root README layout row + example list, CLI README publish row + publish-auth section | The M5.3 OIDC trusted publishing (its own VERIFY — the M0.6 note's `id-token`/hosted-runner facts need a live CI runtime), `github-release-notes` PR-comment mode, the M4.6 corpus checkout-time acceptance, the M5.1 nightly live checks, the corpus-wide pnpm differential (no pnpm in the sandbox), the M2.6 dhat re-audit, the two p50 perf-budget misses, the cargo `--registry` flag mapping | 2026-11-15; session log below |

| M4.5 step 1 check-comment mode (§11.6, ADR-0020) + M5.3 continued `--registry` argv mapping | done | (tree left dirty; no commits) | M1 (M4.5 step 1): the `Forge::upsert_pr_comment` seam capability (ADR-0020) — the marker-based idempotent comment upsert (`<!-- halyard:check v1 -->` scan → rewrite-in-place / create; the §11.3 marker convention at comment scale), the shared `CommentSpec::validate` fail-closed contract (1-based PR, non-empty marker, body carries its own marker — `E-FORGE-REQUEST` before any store touch or exchange), the honest-decline trait default, the `MemoryForge` reference implementation (+ `pr_comments` test view), the `PreviewForge` forecast, the governor forwarding (content call: paced, budgeted, retried), the GitHub REST exchange (ascending 100-per-page marker scan until a short page → `PATCH issues/comments/{id}` or `POST issues/{n}/comments`) + `halyard_changelog::check_comment` (the §11.6 renderer: both verdict shapes, marker-first deterministic markdown, `CheckComment`/`CheckVerdict`/`CheckPrediction` plain data) + the CLI `check --pr-title --comment` (head branch = `GITHUB_HEAD_REF` else in-process gix, PR found by head through the adapter's find exchange, forked-head/missing-PR honest skip in the report, both verdicts post — a rejected title posts its diagnosis then still fails `E-CLI-TITLE`; creds/origin resolved before the verdict paths exit, honest decline `E-CLI-COMMENT` naming the fix; the §11.5 governor rides the live call; the `comment` report block exists only when the flag ran — byte-stable JSON when it does not) + 10 seam tests (`tests/comment.rs`: create/rewrite I1, multi-marker, closed-PR comment, fail-closed matrix, failpoint, call log, preview forecast, decline witness, governor pacing+budget) + 7 adapter wire tests (`tests/comment.rs`: scan→POST, scan→PATCH, page walk, fail-closed spec, transport verbatim, 404 taxonomy, call log) + 7 renderer tests + 3 CLI gates + 3 CLI loopback passes (post / rewrite-in-place / skip) + bench `pr_comment` (render-6, create, rewrite) + example `render_check_comment` + `E-CLI-COMMENT` page + index row + ADR-0020 + arch §11.4/§11.6/§16.2 as-built + README rows. M2 (M1 green first): the cargo `--registry` argv mapping (M5.3 continued) — `cargo_publish_args(manifest, registry, dry)` emits `--registry <name>` for a non-default registry, the two default spellings (`crates.io`/`crates-io`) ride the tool's documented `registry.default` with no flag (`is_default_registry`, mirroring the auth seam's `is_crates_io` — both pinned), VERIFIED against the Cargo Book with the verbatim quote recorded (`docs/research/publish-tooling.md`), the effect passes `target.registry` through the grammar, the CLI test pins the M5.2 routing gate honestly (a third-party cargo index has no resume-probe client → `E-PUB-REGISTRY` before any spawn, dry included, zero side effects — the end-to-end custom-registry publish waits on a client joining the routing map, documented; the argv grammar is complete and unit-pinned) + 2 adapter argv tests + the publish_args integration matrix extended. Micro-debt/docs drift fixed (additive): the garbled `EnvPublishAuth::from_table` doc sentence rewritten; the research note's stale `--allowed-registry` open item annotated as resolved for the flag mapping; a pre-existing clippy failure at base repaired (the `publish_auth` example's unterminated last statement — `semicolon_if_nothing_returned`, surfaced by this session's gate re-run) | — | 2026-11-15; session log below |
| M5.2 continued: custom-registry routing (cargo config) + per-target npm routing | done | (tree left dirty; no commits) | M1: `halyard_eco_cargo::registry_config` (ADR-0021) — the `[registries.<name>]` / `CARGO_REGISTRIES_<NAME>_INDEX` resolution (VERIFIED Cargo Book quotes in `docs/research/publish-tooling.md`): sparse-HTTP indexes join the routing map as `CratesIo::with_index_base` clients, git-index/malformed/unconfigured/set-but-empty declines are honest `E-PUB-REGISTRY` naming the fix; CLI `publish_router(root, targets)` resolves every non-default target registry name before any call; 17 contract tests + 3 CLI tests (dry forecast over a configured registry, git-index refusal, the live loopback e2e: probe miss → shim `--registry corp` argv → visibility confirmed through the custom index) + bench `registry_config` (resolve ≈3.4 µs) + example `resolve_registry`; E-PUB-REGISTRY page as-built + arch §13 as-built. M2 (M1 green first): `RegistryRouter::registry_for` (by-name default) + `TargetRouter` (per-`(registry, package)` overrides) + engine routing through `registry_for`; `halyard_eco_npm::publish_registry` (VERIFIED npm quotes: `publishConfig.registry` > `NPM_CONFIG_REGISTRY` > `.npmrc` `@scope:registry`/`registry`, set-but-empty refused, non-HTTP declined); CLI resolves each npm target's manifest+npmrc chain into per-target overrides; 14 npm contract tests + 4 engine routing tests + 3 CLI tests (npm dry forecast, the live loopback packument e2e with the default registry never contacted, non-HTTP refusal) + bench `target_router` (≈27–33 ns) + example `resolve_publish_registry` | OIDC trusted publishing, the corpus-wide pnpm differential, the M2.6 dhat re-audit, npmrc scoped-registry env vars, source replacement, the legacy `config` filename — all unchanged | 2026-11-15; session log below |
| M4.7 step 1 migration report slice (`halyard migrate`, ADR-0022) | done | (tree left dirty; no commits) | M1: `halyard_config::migrate` — the `MigrationSource` seam (tool name + declared config paths + survey over the discovered `ForeignFiles`; object-safe, the knope/release-plz sources are the declared next impls), the release-please and changesets sources over the VERIFIED grammars (`docs/research/migrate-config-grammars.md`: the release-please manifest-releaser doc + config schema quotes, the changesets config-page quotes), the plain-data report (`MigrationReport`/`ToolSurvey`/`MappingRow`/`UnmappedRow`/`PackageSurvey`, `Serialize`, deterministic I3), `E-CFG-MIGRATE` (`ConfigError::Migrate` + page + index row) fail-closed on a configured-but-unparseable file, the draft `halyard.toml` validated through the strict loader before it is surfaced (never written — the §16.2 write step is its own slice); release-please maps the pre-major pins (foreign defaults preserved with the note naming the side), the tag switches → `repo.tag_style` through the core's `TagTemplate` grammar (per-package disagreement reported, never guessed), per-package components + manifest last-release notes; changesets maps `access`/`baseBranch`/`ignore` (→ `[[package]] release = false` overrides)/`updateInternalDependencies` (→ cascade "always", the closest v1 shape, noted); 19 engine tests + 5 CLI tests (byte-stable JSON, honest not-detected, malformed refusal, gha/human rows; red→green on record: E0432 unresolved `halyard_config::migrate`) + bench `migrate` (release-please 4-pkg survey ≈7.3 µs, changesets ≈8.9 µs, empty tree ≈103 ns) + example `migrate_report` + CLI `halyard migrate` (report-only, no git repo needed) + ADR-0022 + arch §16.2 as-built + root/CLI README rows. M2 (M1 green first): the routing completions (the cargo legacy `config` filename + npmrc scoped-registry env vars, the ADR-0021 map's declared open items) — DEFERRED, not started. Micro-debt/docs drift fixed (additive, not milestone substitutes): the changelog minijinja errors now carry their full source chain (`render_template_source` was dropping the "caused by" line the E-CHLOG-TEMPLATE fix hangs on), the `check` JSON print path's `unreachable!` panic surface replaced with a map-first build, the README daily-commands list completed (probe_registries + release_pipeline rows drifted) and the status paragraph brought current (the ADR-0019/0021 rows and this slice were missing) | The §16.2 `migrate --write` step, the knope/release-plz sources, the M4.7 corpus-lane acceptance (20 real-world configs), the changesets git-tag grammar VERIFY (the docs page quoting `name@version` no longer exists at `main` — the survey carries the honest advisory row instead), M5.3 OIDC (live-CI lane), the corpus-wide pnpm differential, the M2.6 dhat re-audit | 2026-11-15; session log below |



| M4.7 step 2 `--write` slice + ADR-0021 routing-completion slice | done | (tree left dirty; no commits) | M1 (M4.7 step 2, ADR-0023): the §16.2 write step — `halyard_config::migrate` gains the `DraftSink` seam (object-safe, `exists` + create-only atomic `write`), the production `AtomicDraftSink` (temp sibling → fsync → rename, refuses a missing parent dir instead of mkdir-ing), the reference fake `MemoryDraftSink` (models pre-existing files, records writes in order), the pure `plan_draft_write` (zero configured tools → `None` — absence is a value; one → its draft **verbatim** from the report; two or more → `E-CFG-MIGRATE` naming every candidate, never a guess) + `run_draft_write` (the exists gate reads before any write; plain-data `DraftWriteReport` with snake-case `DraftWriteOutcome`); CLI `migrate --write` (JSON gains a `write` block **only when the flag ran** — the frozen report-only shape stays byte-stable; the human header names what landed; a `::notice` in gha; refusals surface `E-CFG-MIGRATE`, nothing-configured is an honest no-op); 12 engine tests (`tests/migrate_write.rs`) + 6 CLI gates (`cli_migrate.rs`) — one red→green cycle (red on E0432 unresolved write-seam imports + the unknown `--write` flag); bench group `migrate_write` (plan + memory-sink run); example `migrate_write` (lands the draft in a temp dir, re-loads it through the strict loader, demonstrates the create-only refusal); ADR-0023 + E-CFG-MIGRATE as-built (three new causes) + index row + arch §16.2 as-built + root/CLI README rows. M2 (ADR-0021's declared routing completions): the cargo legacy `config` filename (VERIFIED Cargo Book quote — "If both files exist, Cargo will use the file without the extension"; the module doc's earlier "deprecated upstream, out of scope" claim was WRONG — drift fixed) via the pure `config_paths_for` enumeration (`config.toml` first, legacy `config` last per directory — the merge core already implements per-key precedence); the npmrc scoped-registry env variables (the npm docs document NO env form — settled by probe against npm 11.19.1, seven probes recorded: the literal `npm_config_@<scope>:registry` name is the scoped key's env form, case-insensitive; the underscore intermediate form is NOT the scoped key — npm reads it as an unknown `myscope-registry` key and warns; the scoped key governs a scoped package regardless of the source tier) via `scoped_registry_env_variable`/`scoped_env_name_matches`/`scan_scoped_registry_env` + the 5th `env_scoped_registry` parameter on `resolve_publish_registry` (precedence: publishConfig → scoped key env→npmrc → plain key env→npmrc → default); 5 cargo tests + 7 npm tests — one red→green cycle (red on E0425/E0061); research note section (verbatim quotes + the seven probes) + ADR-0021 as-built + module docs + CLI README publish row. Doc-drift fixes (additive): the `Command` enum doc listed `migrate` among the absent commands (wired since M4.7) — corrected; the human migrate renderer header + `ToolSurvey::draft` doc moved from "never written" to as-built; `publish-tooling.md`'s "legacy config not read" gap note replaced by the completion record | The knope/release-plz migration sources, the M4.7 corpus-lane acceptance, M5.3 OIDC (live-CI lane), the corpus-wide pnpm differential, the M2.6 dhat re-audit, the two p50 perf-budget misses — all unchanged | 2026-11-15; see the session log below |

| M4.7 continued: knope + release-plz migration sources slice (ADR-0024) | done | (tree left dirty; no commits) | M1: the §16.2 row's remaining two `MigrationSource` impls over the ADR-0022 seam — `KnopeSource` (`knope.toml`: the documented `[package]`/`[packages."<name>"]` shapes, the Release step's tag rules `v{version}` / `{name}/v{version}` composed into one appendix-A.3 template per shape, `versioned_files` as provenance notes, workflows/scopes/extra_changelog_sections/assets/github/jira/variables as unmapped rows with reasons, both package shapes together a fail-closed `E-CFG-MIGRATE`, packageless configs an honest advisory row) and `ReleasePlzSource` (`release-plz.toml` / `.release-plz.toml` / `.config/release-plz.toml` in the documented top-down search order, first-match-only; the Tera `git_tag_name` translated onto the appendix-A.3 grammar by placeholder substitution with any other Tera variable an honest unmapped row — never half-translated; the documented workspace default mapped with the `auto`-equivalence note; per-package `release = false` → the exact `[[package]].release` override; `changelog.style = "keep-a-changelog"` pinned from the documented default; `publish` always unmapped with the I7 opt-in reason; `version_group` a review row; per-package tag disagreements reported, never guessed; `[[package]]` without `name` fail-closed) — both wired into `default_sources()` (the registry is four), no renderer changes (the report is data), the write path untouched (empty drafts are not candidates, the ADR-0023 rule); 21 engine tests + 1 new CLI gate + the `tools.len()` pin moved 2→4 (one red→green cycle, red on `unreachable!("knope survey present")` + the pin); bench groups `knope_survey_2_packages` (≈6.5 µs) + `release_plz_survey_2_packages` (≈9.5 µs) + the empty-tree pin moved to four sources; example `migrate_report` extended with a release-plz survey; the two grammars VERIFIED (knope config/packages/Release-step pages, release-plz config reference — verbatim quotes in `docs/research/migrate-config-grammars.md`) + ADR-0024 + E-CFG-MIGRATE page/index + module doc as-built + README/arch/CLI-README rows | M2 (the corpus-lane acceptance, 20 real-world foreign configs) deferred with the inventory recorded — never half-shipped; the changesets tag-grammar VERIFY, M5.3 OIDC (live-CI lane), the corpus-wide pnpm differential, the M2.6 dhat re-audit, the two p50 perf-budget misses — all unchanged | 2026-11-15; session log below |


| §10.3 prerelease channels — planner integration (M5.6 step 1, ADR-0025) + branch-bound channels (step 2) | done | (tree left dirty; no commits) | M1 (M5.6 step 1): the M1.3 algebra's channel half consumed at last — `advance_prerelease`/`PrereleaseChannel`/`promote_to_stable` moved to `halyard-core::bump` (the ADR-0008 canonical home; `halyard-version` re-exports unchanged, its public API + tests untouched); `PlanConfig.prerelease_channel` + the train-aware `materialize_versions` (an active train = the release base tag is a prerelease; a non-breaking step is absorbed into the train's committed triple — `1.3.0-rc.1` + fix → `1.3.0-rc.2`, never `1.3.1`; a breaking step opens a new base), `promote_pending_stable` (removing the key with nothing new landed plans the manifest's own stable triple — the §10.3 promotion rule; skipped when the manifest is behind the latest tag), `channelize_targets` (the counter witnesses = the base tag's parsed version + the manifest version, applied before requirement rewrites so dependent manifests carry the exact prerelease triple; `Release-As` targets exempt; channels are parallel tracks — the skip guard is equality, not `<=`), the release row's `bump` reports the derived bump on a same-triple advance (an owed release never renders `none`); `halyard-config`: `[versioning].channel` validated at resolve time (`ConfigError::Channel` = `E-CFG-CHANNEL` + page + index row), merged low→high, carried in `ResolvedConfig::prerelease_channel`, `plan_config`, and `config_hash` (a channel change invalidates the §8.7 cache key); schema regenerated (`xtask schema-check --write`); 7 planner tests + 4 config tests + 3 CLI gates (red→green on record: E0609/E0560/E0599 on the missing seam, then the train semantics surfaced by failing expectations — the red phase corrected the design, not the tests); bench `planner_channel` (fresh-train ≈9.5 µs, active-train counter ≈10.7 µs, stable baseline — the channel overhead ≈1 µs); example `prerelease_channel` (the full lifecycle: rc.1 → rc.2 → promotion); existing goldens byte-identical (no channel ⇒ the stable planner unchanged); M2 (step 2, same ADR): `[versioning].channel_branches = { next = "rc" }` — branch→channel bindings validated per key at resolve time (E-CFG-CHANNEL names the branch), merged per-key low→high, the pure `ResolvedConfig::channel_for_branch` (binding wins, detached/unborn HEAD plans the default); CLI `checked_out_branch()` (`GITHUB_HEAD_REF` wins, lenient `None` otherwise) applied in `context()` before the `config_hash` is computed (planning `next` and `main` never share attributions); 4 config tests + 2 CLI gates (branch-binding red→green: E0560/E0599); bench `channel_branch` in halyard-config (hit ≈36 ns, fallback ≈17 ns); arch §10.3 as-built + §16.4 key + ADR-0025 (both steps) + E-CFG-CHANNEL page + README/CLI-README rows + example row | Per-package `[[package]] channel` overrides deferred (the policy is repo-wide like `pre_major` — one train definition per repo); the M5.6 maintenance-branch *backport* flows (planning against `1.x` histories) stay their own slice; the M4.7 corpus-lane acceptance, M5.3 OIDC (live-CI lane), the corpus-wide pnpm differential, the M2.6 dhat re-audit, the two p50 perf-budget misses — all unchanged | 2026-11-15; session log below |


| §10.3 channels completed — per-package overrides (M5.6 step 3, ADR-0026) + maintenance-line trains (M5.6 completion, ADR-0027) | done | (tree left dirty; no commits) | M1 (step 3): `[[package]] channel = "rc"` — `PackageOverride.channel` (loader `package.element` keys + regenerated `halyard-config-v1` schema), validated at resolve time into `ResolvedConfig::package_channels` (E-CFG-CHANNEL names the package; a name-less path-glob override fails closed), merged low→high with later entries winning; `PlanConfig.package_channels` + `channel_for_package` as the ONE resolution point (override > branch binding > unbound default) consulted at all three channel decision points — the active-train absorption rule, promotion (now per package: a trained package continues its train while a channel-less package promotes) and `channelize_targets`; rides `config_hash`; migrate drafts keep `channel: None` (ADR-0022); 4 planner tests + 6 config tests + 3 CLI gates (red→green on record: E0609/E0560 on the missing seam); bench `plan_channel_override` (override hit ≈9.3 µs, mixed two-package ≈18.7 µs vs stable ≈16.4 µs — one pass, no re-planning); `prerelease_channel` example gains the override pass; ADR-0026 + arch §10.3/§16.4 + README/CLI-README + E-CFG-CHANNEL page/index. M2 (M1 green first, same session): the maintenance-branch half of M5.6 — `MaintenanceLine` (parse `MAJOR.x`/`MAJOR.MINOR.x`, matches the final target, renders back), `PlanConfig.maintenance_line` (branch-resolved, shell-applied before the cache key like the channel), `enforce_maintenance_line` as ONE guard pass over the final targets (the channelized triple is what is checked; `Release-As` exempt; the §10.2 remap applies first — on `0.1.x` the remapped `0.2.0` still refuses), `[versioning].maintenance_branches` validated at resolve (`E-CFG-MAINTENANCE` + page + index) and merged per-key low→high; `PlanError::MaintenanceLine` (`E-PLAN-MAINTENANCE` + page + index) refuses with the package, line and target named; 6 planner tests + 4 config tests + 3 CLI gates (red→green on record: E0433/E0609/E0560/E0599, plus two test-side shadowing corrections and a fixture reshaped to a `chore` scaffold so the gates assert the line semantics under either base behavior); bench `plan_maintenance` (in-line patch ≈9.0 µs, unpinned baseline ≈9.9 µs, refusal ≈0.9 µs — the guard exits before rewrites); example `maintenance_line` (backport → refusal → unpinned free); ADR-0027 + arch §10.3/§16.4 + README/CLI-README rows + example row. Micro-debt + drift cleaned en passant: the dead `path` parameter in `halyard-cache::load_locked` removed (`let _ = path;` gone), the CLI README absent-commands row now names `bench`/`man` per §16.2, the versioning did-you-mean snapshot regenerated for both new keys | M5.3 OIDC (live-CI lane), the M4.7 corpus-lane acceptance, the corpus-wide pnpm differential, the M2.6 dhat re-audit — all unchanged; recorded quirk: CLI git fixtures plan from the initial release when no base tag is selected (the repo's single-feat-after-tag shapes make base-derived and initial targets coincide; the maintenance gates shape around it) | 2026-11-16; session log below |

| §9.4 change-file input slice (M3.5 completion, ADR-0028) | done | (tree left dirty; no commits) | M1: the pre-wired-but-unproduced `Reason::Changeset` seam finally fed — `halyard-core::change_file`: the pure `ChangeFiles` input type (bumps + notes per package name, strongest-wins `record`, 4 inline tests) carried by `Snapshot::change_files`; `halyard-changelog::change_files`: the strict TOML-style grammar (`parse_change_file`: `---` delimiters, one `packages = { name = "bump" }` inline table, quoted `major`/`minor`/`patch`, npm scoped-name charset, `ChangelogError::ChangeFile` = `E-CHLOG-CHANGE-FILE` + page + index) + `load_change_files` (path-order merge, fail-closed on any bad file); planner: `direct_bumps` applies the explicit bump after the commit loop (overwrite — replaces the commit-derived level in both directions, the ADR-0028 escape-hatch reading; the monotone cascade guard exempts explicit packages for free), `validate_change_files` refuses unknown names (`PlanError::ChangeFilePackage` = `E-PLAN-CHANGE-FILE` + page + index), `render_entry` merges the notes verbatim (one merge point feeding the §11.3 body, the CHANGELOG.md write and the `create_release` payload); CLI `plan_from_context` scans `.halyard/changes/*.md` (missing directory = zero-config); `Release-As` stays above the change file (both reasons ride the chain when both present); 8 planner tests + golden `plan__change_file` snapshot + 15 parser tests + 3 CLI gates (red→green on record: E0432/E0560/E0599 across the three suites) + bench `planner_changes` (baseline ≈10.2 µs, explicit-bump ≈13 µs, note-merge) + example `change_file` + ADR-0028; every `Snapshot` construction site updated (tests ×3, benches ×3, examples ×2, xtask replay/bench, CLI). Micro-debt + drift fixed (additive, not milestone substitutes): `PlanError` now exposes its structured cause through `Error::source()` (`Unrewritable`'s `VersionError` — the `EditFailure`/`LockfileFailure` convention extended to the planner), the retired `E-CLI-RELEASE` page marked retired (no producer since the live release shell; index row annotated), the §9.4 as-built note rewritten (it still claimed the user-template path was open — it shipped with M4.1b) and now records the change-file completion + the consumed-on-release maintainer contract, README status/layout/commands rows, changelog crate description row | M2 candidate identified but deferred, inventory recorded: `halyard init` (§16.1 — zero-config draft `halyard.toml` create-only + `--github` workflow scaffold, reusing the ADR-0023 `DraftSink` machinery); M5.3 OIDC (live-CI lane), the M4.7 corpus-lane acceptance, the corpus-wide pnpm differential, the M2.6 dhat re-audit, the two p50 perf-budget misses — all unchanged | 2026-11-16; session log below |

| M5.3 step 3: OIDC trusted publishing (ADR-0031) | done | (tree left dirty; no commits) | M1: `halyard-publish::oidc` — `gha_oidc_context` (injected reader; both vars present → context, both absent → None, empty/half → `E-PUB-AUTH` fail-closed naming the variable), `OidcExchanger` over the forge `HttpTransport` (the VERIFIED shapes: audience = URL sans scheme, `GET {url}&audience=` + bearer → `{"value"}`, `POST {url}/api/v1/trusted_publishing/tokens` `{"jwt"}` → `{"token"}`, errors[].detail parsed, everything scrubbed), `OidcPublishAuth` (env grammar first, memoized crates.io exchange as fallback, dry projects `oidc:<audience>` with the exchange disabled I5), the `PublishAuth::provenance` widening (default = the old `credentials`-derived spelling; `auth_summary` rides it), the CLI wire-up (context detection at the auth-capture refusal point, `UreqTransport` live), doctor's `publish-oidc` row + the candidate table now reported in `publish-auth` (the `variables_for` doc claim finally true); 22-test contract matrix (red E0432 on record; the failure-path tests caught a memo-lock deadlock — fixed by scoping the guard), layer rule widened publish→forge consciously, bench `publish_oidc/{exchange_and_resolve,memoized}_14` (≈2.4 µs / ≈1.0 µs), example `publish_oidc`, ADR-0031 + research note (VERIFIED quotes) + E-PUB-AUTH page extended + arch/README/CLI-README rows; 18/18 cli_publish incl. 2 new (half-context refuses before any spawn, doctor row) | npm-side OIDC/provenance (M5.3-later, absent-not-stubbed), real-GHA e2e (nightly forge-smoke lane shape), staging/crs.io custom URL config | 2026-11-16; session log below |
| M2: `check --commits` (§16.2's commits half) | done | (tree left dirty; no commits) | M1: the commits lint — `check::pending_commit_rows` (digest attribution, discovery/walk order I3) + `classify_commit` (the title machinery per subject; typo gate → `E-CLI-COMMIT` + page + index row 70→71, mapped-none informational, autosquash dropped at the parser §8.5 — pinned by test), `render_commits_lint` (3 formats, report prints before the refusal), `--exit-code` interplay kept (exit 3 pending); red→green on record (3 fails on the unrecognized flag), bench group `commits_lint/subjects_200`, CLI README section + arch §16.2 + README rows | — | 2026-11-16; session log below |
| §16.1 zero-config on-ramp: the `halyard init` scaffold (ADR-0029) | done | (tree left dirty; no commits) | M1: `halyard-config::init` — `InitSurvey` (plain data; the shell's discovery fills it, the engine stays pure) → `plan_init` (refuses a zero-package survey with the new `ConfigError::Init` = `E-CFG-INIT` + page + index; renders the deterministic shape line keyed on the same count-based rule the planner's `auto` tag template applies — `effective_template` — so the shape and the tag names can never disagree; renders the comment-only `halyard.toml` skeleton whose defaults load as the built-in defaults EXACTLY — the key-free invariant re-checked at plan time by re-loading the rendered draft) → `InitPlan` (shape + ordered `ScaffoldFile`s) → `run_init` (gates EVERY target's existence through the injected `DraftSink` before the first write — a refusal never leaves a half-scaffold — then writes in plan order) → `InitReport`; `AtomicScaffoldSink` = the §9.1 atomic protocol with the one scaffold-specific difference: the target's parent directories are created (`.github/workflows/` does not exist yet — the migration's `AtomicDraftSink` deliberately never mkdirs, hence two types, the trait doc now says "the sink's stable code"); `--github` appends the §16.1 one-file release workflow VERBATIM (`halyard/action@v1` referenced per the architecture's finished surface — the human render says the glue lands with its milestone); CLI `init [--github]` (needs no git repo, reads no config): the `.config/halyard.toml` shadow footgun refuses first (resolve_config prefers the root file), then survey → plan → effect → three-format render (`init_report`/`_json`/`_gha`); 11 engine tests + 9 CLI gates (red→green on record: E0432 on the missing `halyard_config::init` API + E0599 on `INIT_CODE`, 9 CLI fails on `unrecognized subcommand`; one test-side correction during green — the npm fixture had a stray Cargo.toml, discovery was honestly reporting 2 packages) + bench group `init` (plan_single_package ≈3.4 µs, plan_workspace_40 ≈3.8 µs, plan_and_run over the memory sink ≈3.9 µs — the loader round-trip stays cheap) + example `init_scaffold` (MemoryDraftSink — nothing touches the disk) + ADR-0029. Micro-debt + drift fixed (additive, not milestone substitutes): the halyard-cache process-global hit/miss statics removed (`global_stats`/`reset_stats` had zero callers — the per-instance counters the header round-trips were the live surface), `halyard-changelog::releases_by_version` removed (its doc claimed the idempotence tests used it — they never did), the xtask member-guard test list now names all 18 members (`halyard-config` was missing), arch §7.1 rewritten (the phantom `halyard-action`/`halyard-plugin-host` rows and the nonexistent `docs/spec/` + `action/` paths replaced by an as-built note; `halyard-forge-github` added to the tree), arch §15.1 fuzz row as-built (3 real targets, not 5 aspirational) + the `shuttle` row corrected (loom only, documented-exclusive with miri, the manual invocation spelled), the errors-README intro's `include_str!` claim replaced with the actual linkage (rustdoc `code()` + pages), README status/layout/commands rows, CLI README row + a Zero-config scaffold section, CLI crate description | M2 (M1 green end-to-end first, same session, ADR-0030): the documented miri/loom exclusion is now a `compile_error!` guard in `halyard-cache/src/lib.rs` (a combined build fails loudly with the two documented invocations in the message — verified: `cargo check -p halyard-cache --features miri,loom` exits 101 with the guard, each lane alone passes); `ci-bundle` as the CI gate feature (core/config map it to `schema-gen`, cache's is deliberately empty — the exclusive pair stays out; workspace-wide `--features ci-bundle` enables per member, skips the rest); CI's clippy/test/nextest/doc jobs moved off `--all-features` onto `--features ci-bundle` and a new `loom` job runs the suite's own documented invocation on stable (the suite previously compiled only as an `--all-features` side effect — it gains a spelled lane instead of losing one); arch §15.1 rows + cache manifest notes + README daily-commands updated to the ci-bundle spelling; gates re-run under the new spelling (the guard makes `--all-features` an invalid gate invocation by design). M5.3 OIDC (live-CI lane), the M4.7 corpus-lane acceptance, the corpus-wide pnpm differential, the M2.6 dhat re-audit, the two p50 perf-budget misses — all unchanged | 2026-11-16; session log below |

| Bitbucket parallel-steps adapter (ADR-0041) + the `halyard bench` measurement command (ADR-0042) | done | (tree left dirty; no commits) | M1: `BitbucketSelector` (`affected --ci bitbucket`) — one paste-ready `parallel:` block over the affected closure, name-sorted, one `export HALYARD_PKG=<name>` script line per affected package (support.atlassian.com VERIFIED 2026-11-17: "Parallel steps allow you to group pipeline steps that can run at the same time"; the required `steps:` property "contains the lists of `step`s in a `stage` or a `parallel`. At least 1 `step` is required within the steps list"; the docs' own example varies the steps **in the script** (`--batch 1` / `--batch 2`); no per-step `env` exists — `variables` is a `pipe`-only option — so the assignment rides the script line the consumer's tool invocation reads; the relative indentation follows the documented example exactly; the "100-step limit on pipelines includes each step in a parallel group" cap is the consumer's policy, the render never the guard), lookup + trait/module docs + CLI value enum + flag doc + the `affected_ci` example + bench `ci_selector_bitbucket_40chain` all moved one row over; the refused-unknown pin moved `bitbucket` → `drone` (an unrecorded system — the recorded CI surface ADR-0034→0041 is complete). M2 (M1 green end-to-end first): the §16.2 declared-absent `bench` command landed — the §14 model pipeline (`discover` → `config` → `history` → `plan`) timed per pass over the current repo's real shape, one untimed warmup before the timed passes (the §14.1 no-op-warm model; `--no-cache` = the cold measurement, the posture reported either way), the stage names riding the `--profile` trace's span vocabulary through the new read-only `Profiler::recorded_spans()` accessor, the nearest-rank percentiles the §14.1 budget table speaks; the aggregation is pure data on the lib target (`halyard_cli::bench` — the man renderer's rationale: bench harness and bin share one implementation) + `bench_report` criterion bench (8/200 passes); a measurement never a gate — the §14.1 budgets stay shape-bound to the xtask fixtures and the render says so; `E-CLI-BENCH` (zero-iteration refusal) + page + index row; 5 CLI gates + 5 module tests; the man bench's mirror tree repaired to the real 20-command count (it had drifted by one, missing `action`) | The M5.4 provenance/SLSA (live-CI lane), the corpus-wide pnpm differential (no pnpm in the sandbox), the M2.6 dhat re-audit, the two p50 perf-budget misses — all unchanged | 2026-11-17; see the session log below |
| Drone environment adapter (ADR-0045) + the §12 `release` mode (ADR-0046) | done | (tree left dirty; no commits) | M1: `DroneSelector` (`affected --ci drone`) — one step-scoped `environment:` fragment per affected package (docs.drone.io VERIFIED 2026-11-17: "Drone provides the ability to define environment variables scoped to individual build steps", the documented per-step mapping; "the environment section cannot expand environment variables or evaluate shell expressions" — the bare-word `HALYARD_PKG` value rides verbatim; steps are "executed sequentially by default" and the `depends_on` DAG — "you must configure dependencies for all pipeline steps" — is the consumer's policy), lookup entry + CLI value-enum variant + flag doc, the refused-unknown pin moved `drone` → `travis` (the contract-test loop names twelve systems), `affected_ci` example row + `ci_selector_drone_40chain` bench (≈82 ns, the 40-package chain) + research-note section + ADR-0045 + README/CLI-README/arch rows. M2 (M1 green end-to-end first): the §12 `release` mode (ADR-0046) — `cmd_release` extracted into the shared `run_release_pass` (one plan + one reconciliation feeding the tag phase, the §11.2 push, the forge half; the forge half's internal re-plan/re-reconcile deleted), `ReleasePass`/`ForgePosture` plain-data view (the created tags in the tag phase's name-sorted order I3, the minted URLs by tag, the skip's reason verbatim, `released` honest — a dry forecast stays false, I5), `release_outputs`/`release_summary` renderers (the plan vocabulary minus the PR keys — the release mode opens no Release PR), `ActionMode::Release` + `cmd_action_release` wired (`--dry` forecasts over the preview writer/pusher), `release` reports byte-identical (the `cli_release` suite pins them); 3 action contract tests + 3 CLI gates (red→green on record: E0432 on the missing API + `invalid value 'release'`; the red phase corrected two test fixtures, not the design — the fully-tagged shape forecasts zero creations honestly, the ahead-of-tags shape names the would-create tag), bench `release_outputs`/`release_summary_40_releases` (≈45 µs outputs), example `release_forecast`; `--dry` flag doc now names both modes. Micro-debt cleaned (additive, not milestone substitutes): the bench aggregation's `unreachable!` panic surface replaced with the fail-closed `E-CLI-BENCH` taxonomy error (the guarantee lives in the error, not a panic) and `commit_typo_message`'s `unreachable!` guard deleted — the typo parts ride in from the caller's own map-first match (no re-match, no panic surface) | The §12 `auto` mode (the trigger-policy composition, its own ADR), M5.4 provenance/SLSA (VERIFY first), the M2.6 dhat re-audit, the corpus-wide pnpm differential (no pnpm in the sandbox), the two p50 perf-budget misses — all unchanged | 2026-11-17; see the session log below |
| The §12 `auto` mode — the composition policy over the two executors (ADR-0047) | done | (tree left dirty; no commits) | M1: `ActionMode::Auto` + `cmd_action_auto`/`run_auto_pass` — the §11.1 stateless reconciliation as one §12 mode, the composition policy over the two shipped executors (§11.1's own order: one context → plan → manifest-vs-tags reconciliation; the release executor's git-side tag effects first — the tag phase over the posture-selected writer, the §11.2 push, an empty pending an honest no-op; then the §11.1 machine pass with BOTH halves' inputs (`pending_releases` + the Release-PR spec), so the half selection is the machine's own PROPAGATE rule and never a caller guess — pending releases gate the prepare phase (§11.1 step 2), nothing pending upserts or closes the Release PR (§11.1 step 3)); one plan, one reconciliation, one machine pass (the ADR-0046 lesson carried across the composition — the auto pass never re-plans between the halves and never runs the release executor's separate forge half, whose machine input carries no Release-PR spec); `halyard-action`: the plain `AutoPass` view + `auto_outputs`/`auto_summary` (the pr/release superset — the PR keys ride only when the pass has a Release-PR effect, `released` is the git-side tag phase's honest execution value, the minted URLs by tag, the forge posture, the forecast marker; the crate stays git-free and forge-free); `--dry` forecasts the whole pass offline (I5 — preview writer, pusher, forge); live fails-closed on credentials like the `pr` executor, the §11.5 governor governs the machine's calls; 4 CLI gates (`cli_action.rs`, red→green on record: E0004 non-exhaustive `ActionMode` before any implementation) + bench groups `auto_outputs_40_releases`/`auto_summary_40_releases` (≈38.6 µs, microsecond-scale) + example `auto_forecast` (the PROPAGATE vocabulary); ADR-0047 + arch §12 as-built (the stale ADR-0037 note's `auto\|release\|check`-absent sentence amended — release and check had landed) + README example row + CLI README rows. M2 deferred: the remaining candidates cannot ship fully green here (see the PR body) | The §11.1 step-3 content commit (M4.2 remainder: the plan's EditSet → tree-OID compare → the CAS branch push; `commit_via_api` still has no caller), M5.4 provenance/SLSA (VERIFY first), the M2.6 dhat re-audit, the corpus-wide pnpm differential (no pnpm in the sandbox), the M0.3 measured runs (needs forge API + token) — all unchanged | 2026-11-24; see the session log below |
| The §12 composite distribution (M5.4 own-artifact slice, ADR-0052) + the GitLab CI component (M7, ADR-0053) | done | (tree left dirty; no commits) | M1: the §12 composite distribution — `action/action.yml` (the §7.1 as-built's declared `action/` path: the five §12 steps — resolve OS/arch, download, verify checksum **and** attestation, cache by version, exec `halyard action`; composite step grammar VERIFIED: `shell`/`if`/`env`/`working-directory`) + `.github/workflows/release.yml` (the producer: the pinned static targets, `actions/attest@v4` with `id-token: write`/`attestations: write` — the M5.4 acceptance's producer half — the `SHA256SUMS` assembly in the grammar's canonical order, `gh release upload`) + the single-sourced grammar `halyard_action::distribution` (the §12 `ACTION_INPUTS` with each input's surface — `--mode`/`--config`/`--dry`, the `GITHUB_TOKEN`+`HALYARD_FORGE_TOKEN`/`HALYARD_APP_*` env pairs, the `halyard publish` executor, the `working-directory` key — `ACTION_MODES`, `ACTION_OUTPUTS`, five pinned `RELEASE_TARGETS` with the `RUNNER_OS`/`RUNNER_ARCH` mapping, the `halyard-<triple>.tar.gz`/`.zip` asset grammar, the `<digest>␣␣<file>` `ChecksumsManifest` parse/render/verify/require_coverage — `sha2` joins the halyard-action layer consciously, layer rule widened) + `E-ACTION-DISTRIBUTION` page + index + the drift gate `cargo run -p xtask -- check-release` (the schema-check discipline over the templates: restricted inputs reader fail-closed on unknown sub-keys, the platform table resolved against the pinned set with a fail-closed `*` arm, the manifest order, the attestation producer/consumer pair; wired into CI as the `templates` job) + the CLI contract test `cli_action_template.rs` (6 gates, red→green on record: E0432/E0433 on the missing module, then the file-absent pins — the §12 line quoted in the test doc) + 9 grammar unit tests + bench `distribution` (manifest build 5×1 MiB, parse/render, verify 1 MiB ≈ 603 µs) + example `distribution_plan` + ADR-0052 + research note `own-artifact-attestation.md` (gh attestation verify + attest@v4 + composite grammar + Get-FileHash VERIFIED quotes; the coreutils-manual fetch timeouts recorded honestly) + arch §7.1/§12/§17 as-built + README/CLI-README rows. M2 (M1 green first): the GitLab CI component (M7's "CI component" row, ADR-0053) — `templates/halyard.yml` (the documented component layout VERIFIED on docs.gitlab.com/ci/components: `spec:`/`inputs` above `---`, `$[[ inputs.* ]]`; the `stage`/`mode`/`dry-run` inputs; the same musl release binary over `alpine:3.20`, the `SHA256SUMS` line verified with `sha256sum -c`; the token riding `HALYARD_FORGE_TOKEN`/`GITHUB_TOKEN`; the attestation verify the GitHub lane's honest decline) + the component half of the drift gate + ADR-0053 + the research note's GitLab section + arch §17 M7 as-built + README/CLI-README rows. Docs drift fixed (milestone-coupled, additive): the init scaffold's "glue lands with its milestone" doc + render line now as-built (ADR-0052 landed it). Micro-debt converted (additive): two zero-caller `pub fn`s deleted — `halyard_core::edit::span_range` (the dead-code scan's only pre-existing hit) and this session's own `latest_download_url` (the discipline catching the grammar's unused half) | The M7 corpus-lane e2e (forge-token-bound, recorded), M5.4's cosign/SBOM rows and the live attested-release run (a real `v*` tag push), the M2.6 dhat re-audit, the M0.3 measured runs, the corpus-wide pnpm differential — all unchanged, all recorded | 2026-11-24; see the session log below |


### 2026-11-16 — §16.1 zero-config on-ramp: the `halyard init` scaffold (ADR-0029) (tree left dirty, no commits)

- **Inventory at session start.** Main was green at `299cf47` (the
  change-file session): fresh stable toolchain 1.100.0,
  `cargo check --workspace --all-features --all-targets --locked` exit 0
  as the baseline. The survey (three parallel read-only agents + hand
  verification) classified the tree: zero `todo!`/`unimplemented!`/
  `#[allow]` anywhere; the error taxonomy bidirectionally exact (69
  pages ↔ 69 index rows ↔ 68 producers, `E-CLI-RELEASE` explicitly
  retired); 17/18 crates carry consumer examples (xtask is the only
  one without); the declared start point from the previous session was
  `halyard init` (§16.1) with the ADR-0023 `DraftSink` machinery named
  as the reuse surface. Verified open debt: arch §7.1 still listed the
  phantom `halyard-action`/`halyard-plugin-host` crates and omitted
  `halyard-forge-github`; three public items with consumer-claiming
  doc comments and zero callers (`halyard-cache::global_stats`/
  `reset_stats`, `halyard-changelog::releases_by_version`); the xtask
  member-guard test list named 17 members and omitted
  `halyard-config`; arch §15.1 claimed five fuzz targets and a
  `shuttle` lane that do not exist; the errors-README intro claimed a
  `#[doc = include_str!(…)]` linkage no error page uses. Feature
  surface: `miri`/`loom` documented-exclusive but not
  machine-enforced, `--all-features` CI combines them, no `ci-bundle`
  feature, no bench requires nightly.
- **M1 implemented (no stubs), one full red→green cycle.** The red was
  the missing API itself (`E0432` on the `halyard_config::init` exports,
  `E0599` on `ConfigError::INIT_CODE`, 9/9 CLI gates failing on
  `unrecognized subcommand 'init'`), captured on both suites before any
  implementation. The green landed the ADR-0029 design: pure engine in
  `halyard-config`, gate-before-write in `run_init`, the skeleton's
  key-free invariant checked at plan time (the rendered draft is
  re-loaded and must equal the built-in defaults), the workflow
  verbatim from §16.1. One test-side correction during green (the red
  phase corrects the tests, not the design): the npm-mix CLI fixture
  carried a stray `Cargo.toml`, and discovery was honestly reporting
  two packages — the fixture lost the manifest, not the engine its
  count.
- **M2 implemented (M1 green end-to-end first), one red→green cycle.**
  The red was the guard itself: `cargo check -p halyard-cache
  --features miri,loom` exits 101 on the `compile_error!` once the
  exclusion is expressed in code — the failing build is the test, and
  each lane alone still compiles. The green landed ADR-0030: the
  `ci-bundle` gate feature (core/config → `schema-gen`, cache → empty,
  the exclusive pair out), CI's clippy/test/nextest/doc jobs on
  `--features ci-bundle`, and a new loom CI job running the suite's
  own documented invocation — the model check gains a spelled lane
  where `--all-features` used to switch it on as a side effect.
- **Tests/bench/example totals.** 11 engine contract tests (`tests/
  init.rs`: shapes, determinism, the key-free skeleton, the §16.1
  workflow bytes, the gate-before-write refusals over the memory sink,
  the atomic sink's mkdir + create-only), 9 CLI end-to-end gates
  (`tests/cli_init.rs`: scaffold → zero-config `plan` in one fixture,
  create-only rerun with the stable code, the workflow gate, the
  shadow refusal, the empty-tree refusal, npm survey, human + gha
  renders), bench group `init` (3 shapes), example `init_scaffold`.
- **Docs.** ADR-0029; `E-CFG-INIT` page + index row (69 → 70 codes,
  pages and producers exact); arch §16.1 as-built note + §16.2
  wired-commands note (`init` moved from the absent list to the wired
  list); README status/layout/commands rows; CLI README row + a
  Zero-config scaffold section; the `Command` enum doc and the CLI
  crate description.
- **Micro-debt + drift (additive).** The dead cache statics and
  `releases_by_version` removed with their now-unused imports; the
  xtask guard list completed to 18 members; arch §7.1/§15.1 rewritten
  as-built; the errors-README intro corrected.
- **Gates on the final tree.** fmt ✓, clippy `--workspace --all-targets
  --features ci-bundle -- -D warnings` ✓ (0 findings), tests
  `--features ci-bundle --locked` ✓ (113 suites, 1006 tests, 0 failed),
  benches ✓ (compiled and run, 0 errors; the `init` group ≈3.4-3.9 µs),
  the lint-hygiene grep empty, rustdoc `-D warnings` ✓, `xtask
  check-layers` (18 crates) + `schema-check` ✓, the loom lane ✓ (`cargo
  test -p halyard-cache --features loom --release`: 2 model checks + 9
  cache tests), the example ✓, and the conflict guard's two-sided proof
  (combined build exits 101 on the guard, each lane alone passes).
- **Honest state at cut-off.** The tree is dirty by protocol; every
  gate re-run on the final tree. The scaffolded workflow references
  `halyard/action@v1` per §16.1's finished surface — the action glue
  is its own future milestone and the render says so. The live
  `--github` write is exercised through the atomic-sink tests and the
  CLI gates; a real GitHub Actions run of the generated workflow
  belongs to the action milestone's lane.
- **Next session start point.** (1) the M4.7 corpus-lane acceptance,
  (2) the `halyard/action` glue (§12) so the scaffolded workflow
  resolves, (3) M5.3's OIDC half (live-CI lane).

### 2026-11-16 — §9.4 change-file input (M3.5 completion, ADR-0028) (tree left dirty, no commits)

- **Inventory at session start.** Main was green at `38b8873` (the
  channels-completion session): fresh stable toolchain 1.100.0,
  `cargo check --workspace --all-features --all-targets` exit 0 as the
  baseline. The survey (two parallel read-only agents + hand reads)
  classified the tree: zero `todo!`/`unimplemented!`/`#[allow]`
  anywhere (structural, not discipline), 17/18 crates carry consumer
  examples (only xtask lacks one), error taxonomy 67 codes ↔ 67 pages
  ↔ 67 producers with exactly one asymmetry (`E-CLI-RELEASE` has no
  producer — retired, unmarked), architecture §7.1 lists two phantom
  crates (`halyard-action`, `halyard-plugin-host`) while omitting
  `halyard-forge-github`, and the §9.4 as-built note still claimed the
  user-template path was open. Feature surface: `miri`/`loom` are
  documented-exclusive but not machine-enforced, and the `--all-features`
  CI lane silently combines them (the shadow-mmap path runs there);
  nothing requires nightly, no bench is nightly-gated; no `ci-bundle`
  feature exists.
- **M1 implemented (no stubs), one full red→green cycle.** The red was
  the missing API itself (`E0432` on `halyard_core::change_file` /
  `halyard_changelog::change_files`, `E0560` on the `Snapshot` field,
  `E0599` on both error variants), captured on all three suites before
  any implementation. The green landed the ADR-0028 design: pure data
  in core, strict grammar in the §9.4 crate, planner precedence
  (explicit replaces commits, `Release-As` still outranks it, cascade
  exempt by the existing monotone guard), one note-merge point in
  `render_entry`, the CLI scan wired into the shared `plan_from_context`
  pipeline. Test-side corrections during green (the red phase
  corrected the tests, not the design): the strongest-wins fixture had
  to record notes verbatim and in path order (the loader sorts,
  `record` preserves), and the cascade fixture's requirement site had
  to name the dependent's real manifest.
- **M2 deferred, inventory recorded.** `halyard init` (§16.1) is the
  chosen next milestone: zero-config draft `halyard.toml` create-only
  (the ADR-0023 `DraftSink` machinery exists — `plan_draft_write` +
  `AtomicDraftSink`) plus the `--github` workflow scaffold; it needs
  its own red→green cycle, CLI gates, docs row and an example, which
  did not fit the session budget after the full gate battery. Deferred,
  not half-shipped.
- **Tests/bench/example totals.** 8 new planner integration tests +
  the `plan__change_file` golden (wire format + plan_hash pinned), 15
  change-file grammar tests, 4 `ChangeFiles` inline tests, 3 CLI
  end-to-end gates, bench group `plan_changes` (3 shapes), example
  `change_file`, all `Snapshot` sites (11 files) updated.
- **Docs.** ADR-0028; `E-CHLOG-CHANGE-FILE` + `E-PLAN-CHANGE-FILE`
  pages + two index rows (67 → 69 codes, pages and producers exact);
  §9.4 as-built rewritten (drift fixed); README status/layout/commands;
  crate description row.
- **Micro-debt + drift (additive).** `PlanError::source()` (the
  structured `VersionError` cause surfaced), `E-CLI-RELEASE` marked
  retired (page + index row), §9.4 note's stale user-template claim
  corrected.
- **Gates on the final tree.** fmt ✓, clippy `-D warnings` workspace
  ✓, tests `--all-features` ✓ (0 failures), benches ✓, lint-hygiene
  grep empty, rustdoc `-D warnings` ✓ (session log records the exact
  counts).
- **Honest state at cut-off.** The tree is dirty by protocol; every
  gate re-run on the final tree. The `--all-features` lane still
  combines the documented-exclusive `miri`+`loom` (flagged, not
  enforced — a candidate xtask check, not landed this session).
- **Next session start point.** `halyard init` (§16.1) per the M2
  inventory above; then the M4.7 corpus-lane acceptance.

### 2026-11-16 — §10.3 channels completed: per-package overrides (M5.6 step 3, ADR-0026) + maintenance-line trains (M5.6 completion, ADR-0027) (tree left dirty, no commits)

- **Inventory at session start.** Main was green at `e08b5cc` (the
  prerelease-channel session): baseline re-verified with a fresh
  stable toolchain installed in-sandbox (1.100.0; rustfmt/clippy
  components added — the minimal profile ships neither),
  `RUSTFLAGS="-C debuginfo=0"`, `CARGO_INCREMENTAL=0`. Baseline gates
  green: `test --workspace --all-features --locked` exit 0,
  check-layers (18 crates), schema-check, the rustdoc `-D warnings`
  gate, the lint-hygiene grep empty. The declared start points were
  (1) the M4.7 corpus-lane acceptance, (2) the M2.6 dhat re-audit,
  (3) M5.3 OIDC (needs a live CI runtime), (4) per-package channel
  overrides — (1) is an acceptance run, (2) is perf, (3) is
  environment-blocked; picked (4) as M1 (it closes the known shape gap
  ADR-0025 recorded) and the M5.6 row's other open half — the
  maintenance-branch trains — as M2. The session survived a sandbox
  recycle: the tree was wiped mid-flight and every edit replayed from
  the session record onto a fresh clone of the same base commit
  (verified per-suite green before proceeding; no re-survey needed).
- **M1 implemented (no stubs), one full red→green cycle** (the
  planner/config/CLI tests written first — red on E0609
  `package_channels` and E0560 `PackageOverride.channel`, then
  implemented):
  - Design decisions (ADR-0026): exact-name matching like
    `release = false` (path-glob overrides refine include/exclude, and
    a channel on a name-less override fails closed); precedence
    override > binding > default (explicit user intent beats
    environment-derived policy — the `Release-As` reasoning); one
    resolution point (`channel_for_package`) consumed by the three
    channel decision points, so promotion becomes per package instead
    of the wholesale "channel ⇒ no promotion" early return.
  - clippy surfaced two corrections during the cycle: `or_else` →
    `or` (`unnecessary_lazy_evaluations`), and `materialize` crossed
    the `too_many_lines` budget (101/100) — the validation extracted
    into the documented `package_channels_of` helper, matching the
    ADR-0025 session's extraction precedent.
- **M2 implemented (M1 green end-to-end first), one red→green cycle**
  (the planner/config/CLI tests written first — red on E0433
  `MaintenanceLine`, E0609 `maintenance_line`, E0560/E0599 on the
  config seam, then implemented):
  - Design decisions (ADR-0027): the entry string is both the branch
    name and the pinned line (one token, no glob to keep in sync);
    fail closed instead of clamping (a clamp would publish a version
    the commits do not justify); ONE guard pass over the final targets
    (the channelized triple is checked) instead of threading Results
    through three derivation stages; branch-resolved pin applied in
    `context()` before the cache key, the ADR-0025 step-2 shape.
  - Two test-side corrections the cycle surfaced (test bugs, not
    design): a `let snapshot = ...` value shadowed the helper in the
    same scope (E0618 — bindings renamed), and the CLI fixture's
    `feat:` scaffold made the initial-release path coincide with the
    base path — the scaffold became a `chore`, so the gates assert the
    line semantics themselves.
  - Pre-existing quirk pinned while shaping that fixture: CLI git
    fixtures plan from the initial release when no base tag is
    selected — verified against a pristine `e08b5cc` worktree (same
    `1.1.0`) before concluding it is not a regression; the repo's
    single-feat-after-tag shapes make base-derived and initial targets
    coincide numerically, which is why the existing suites stayed
    green. Recorded in ADR-0027's consequences and the milestone row.
- **Gates at session end.** fmt `--all --check` green; clippy
  `--workspace --all-targets --all-features --locked -- -D warnings`
  green (zero findings across the two milestones' code); the full
  battery re-run on the complete tree — suites/tests/fmt/clippy/doc/
  layers/schema/loom — all green (numbers in the final report).

### 2026-11-15 — §10.3 prerelease channels: the planner integration (M5.6 step 1, ADR-0025) + branch-bound channels (step 2) (tree left dirty, no commits)

- **Inventory at session start.** Main was green at `de9b663` (the
  knope/release-plz slice): baseline re-verified with a fresh stable
  toolchain installed in-sandbox (`RUSTFLAGS="-C debuginfo=0"`,
  `CARGO_INCREMENTAL=0` — the disk-ceiling lesson applied: the first
  full test build filled the rootfs to 100 % once and the incremental
  dirs had to be dropped). The declared start points were (1) the
  M4.7 corpus-lane acceptance, (2) the M2.6 dhat re-audit, (3) M5.3
  OIDC (needs a live CI runtime), (4) the corpus-wide pnpm
  differential (needs a pnpm binary the sandbox lacks). Picked none of
  them as the milestone: (1) is an acceptance run, not net-new
  capability, (2) is perf, (3)/(4) are environment-blocked — and the
  roadmap's own M5.6 row ("prerelease channels + maintenance-branch
  release trains; fixtures: `next`/`rc`, `1.x` backports") is fully
  offline-verifiable and entirely unstarted: the M1.3 algebra shipped
  `advance_prerelease` with **zero planner integration** (only its
  tests and the bump example called it). M1 = the channel planner
  integration; M2 = the branch-bound channels §10.3 names in the same
  sentence.
- **M1 implemented (no stubs), one full red→green cycle** (the 7
  planner tests written first — red on E0609/E0560 (`PlanConfig` had
  no `prerelease_channel`), E0422 (`PrereleaseChannel` not in core) —
  the config tests red on E0560/E0599 (`VersioningCfg.channel`,
  `ResolvedConfig.prerelease_channel`, `E-CFG-CHANNEL` missing) — the
  3 CLI gates red at runtime (the binary ignored the key); then
  implemented. Two of the first green-run failures were **design
  corrections the tests surfaced, not test bugs**: applying the
  channel over `apply_bump`'s output produced `1.3.0-rc.1 + fix →
  1.3.1-rc.1` (the fix double-bumped the train's triple) and the
  counter dropped a channel *switch* to a semver-lower tier
  (`rc.1 → beta.1`) — the shipped semantics absorb non-breaking steps
  into the train's committed triple and guard only equality:
  - Design decisions (ADR-0025): the counter witnesses are exactly the
    §10.3 stateless inputs — the release base's parsed version (the
    resolver already picked the newest train tag) and the manifest
    version (which witnesses the applied-but-untagged case); the
    channel half of the algebra moved to `halyard-core::bump` per the
    ADR-0008 precedent with `halyard-version` re-exporting unchanged;
    `Release-As` targets are exempt (explicit user intent); promotion
    is a lifecycle act (removing the key with nothing new landed plans
    the manifest's own stable triple, skipped when the manifest is
    behind the latest tag — the reconciliation owns that state); a
    migration never sets the key (ADR-0022); the release row's `bump`
    reports the derived bump on a same-triple advance.
  - `config_hash` includes the channel (a channel change invalidates
    the §8.7 cache key) — pinned by test; the frozen
    `halyard-plan/v1` wire format needed no change (the plan's `to`
    carries the prerelease string); the §11.1 reconciliation and tag
    phase needed no change (they already compare full semver versions)
    — pinned by the existing suites.
- **M2 implemented (M1 green first), one red→green cycle** (the 4
  config tests + 2 CLI gates written first — red on E0560/E0599, then
  green): `[versioning].channel_branches` validated per key at resolve
  time (the error names the branch), merged per-key low→high, the pure
  `ResolvedConfig::channel_for_branch` (binding wins over the unbound
  default; `None` = detached/unborn HEAD plans the default); the CLI
  resolves the checked-out branch once in `context()` —
  `GITHUB_HEAD_REF` wins (the §11.5 workflow view), else the gix HEAD
  referent, lenient `None` — **before** the `config_hash` is computed,
  so planning `next` (bound `rc`) and `main` (stable) never share
  attributions. The branch test drives one fixture through two
  checkouts (`next` → `rc.1`, `main` → stable) over the same config.
- **Tests/bench/example totals.** +11 planner/config engine tests + 5
  CLI gates over base (7 channel planner tests, 4+4 config tests, 3+2
  CLI gates; the did-you-mean snapshot for `[versioning]` regenerated
  with `channel`/`channel_branches` in the valid list); bench groups
  `plan_channel` (fresh-train ≈9.5 µs, active-train ≈10.7 µs, stable
  baseline — channel overhead ≈1 µs) and `channel_branch` (bound hit
  ≈36 ns, fallback ≈17 ns); example `prerelease_channel` (the whole
  lifecycle: rc.1 → rc.2 → promotion to 1.3.0).
- **Docs.** ADR-0025 (both steps, the tradeoffs on record), the
  `E-CFG-CHANNEL` page + index row, arch §10.3 as-built + the §16.4
  `channel` key, root README status paragraph + example row, CLI
  README channel + branch-binding notes, PROGRESS row above.
- **Gates on the final tree.** fmt green; clippy
  `--workspace --all-targets --all-features --locked -- -D warnings`
  0 findings (three real findings surfaced en route and were fixed in
  code, not suppressed: two `too_many_lines` (the bump display and the
  release-please draft assembly extracted into documented helpers),
  plus bench/example-side `redundant_closure`/`field_reassign`/
  semicolon findings); the lint-hygiene grep empty; the full
  `test --workspace --all-features --locked` green; `--benches`
  compiled and run; `check-layers` ✓ 18 crates; `schema-check` ✓ (the
  schema regenerated for the two new keys). Gate notes: the literal
  `-W pedantic` appended to the clippy CLI is (a) a deprecated lint
  name on this toolchain (denied by `-D warnings` itself) and (b) when
  properly qualified `-W clippy::pedantic`, overrides the two curated
  `[workspace.lints.clippy]` allows (`must_use_candidate`,
  `missing_errors_doc`, each with its rationale) — 95+8 findings that
  are the documented curation, not regressions; the pedantic
  enforcement this repo ships is `pedantic = deny` in the lints table
  (README documents exactly this: "a CLI -W would override the curated
  allows"). The cache conflict-feature lane (`miri`/`loom` never
  combined) ran as documented: `--features loom --release`.
- **Honest state at cut-off.** Per-package `[[package]] channel`
  overrides deferred (repo-wide like `pre_major`); the M5.6
  maintenance-branch *backport* flows stay their own slice; the M4.7
  corpus-lane acceptance, M2.6 dhat re-audit, M5.3 OIDC, the
  corpus-wide pnpm differential and the two p50 perf-budget misses are
  unchanged. Tree left dirty (no commits).
- **Next session start point.** (1) the M4.7 corpus-lane acceptance,
  (2) the M2.6 dhat re-audit with the cache warm, (3) M5.3 OIDC on a
  live CI runtime, (4) per-package channel overrides.

### 2026-11-15 — M4.7 continued: the knope + release-plz migration sources (ADR-0024) (tree left dirty, no commits)

- **Inventory at session start.** Main was green at `4a701c0` (the
  M4.7 step-2 slice): baseline re-verified on a fresh toolchain
  (stable 1.100.0, `RUSTFLAGS="-C debuginfo=0"` — the disk-ceiling
  lesson applied from the first build): `test --workspace
  --all-features --locked --no-run` green at base. The declared start
  points were (1) the knope/release-plz `MigrationSource` impls, (2)
  the M4.7 corpus-lane acceptance, (3) the M2.6 dhat re-audit, (4) M5.3
  OIDC on a live CI runtime. Picked (1) as M1 — the §16.2 row's
  remaining two tools, offline-verifiable, the ADR-0022 seam's declared
  consumers — and (2) as M2; the M1 slice consumed the session budget,
  so M2 is DEFERRED with the inventory recorded, never half-wired.
- **M1 implemented (no stubs), one full red→green cycle** (the 21
  engine tests written first — red on `unreachable!("knope survey
  present")` and the two-tool registry pin; the CLI gate red on the
  `tools.len()` pin — then implemented):
  - Design decisions (ADR-0024): the foreign tag grammars translate,
    they are not re-derived — knope's Release step documents *rules*
    (`v{version}` single, `{package_name}/v{version}` multi), which
    compose into exactly one appendix-A.3 template per shape;
    release-plz documents *Tera templates*, so the translation is
    placeholder substitution over the two documented placeholders, and
    any other Tera variable or block survives as an unmapped row — the
    survey never half-translates. release-plz's `git_tag_name` default
    depends on the repo shape the survey cannot observe, so the
    documented *workspace* default maps with the note naming the rule
    halyard's own `auto` applies. `publish` is always an unmapped row
    carrying the I7 opt-in reason (a migration never sets
    `publish.enabled`); per-package `release = false` is the one key
    that maps exactly. knope's two package shapes are exclusive — both
    at once is `E-CFG-MIGRATE`, not a pick.
  - The two grammars were VERIFIED per §0.4 before any mapping (the
    knope config/packages/Release-step pages, the release-plz
    configuration reference; verbatim quotes in
    `docs/research/migrate-config-grammars.md`).
  - The write path needed no changes: empty drafts are not write
    candidates (the ADR-0023 rule), so a knope/release-plz survey with
    nothing mappable is not a `--write` candidate.
  - No renderer changes: the report is data, the CLI JSON/human/GHA
    renderers carried the new sources as-is (the new CLI gate covers
    both through the JSON path).
- **Tests/bench/example totals.** +22 tests / +1 gate over base (21
  engine `tests/migrate.rs` + 1 CLI gate; the `tools.len()` and
  empty-tree pins moved 2→4). Bench groups `knope_survey_2_packages`
  (≈6.5 µs) and `release_plz_survey_2_packages` (≈9.5 µs) registered
  and run beside the existing survey groups. Example `migrate_report`
  extended with a release-plz survey (tag translation, the release
  override, the review rows, the validated draft).
- **Gates on the final tree.** fmt green; clippy
  `--workspace --all-targets --all-features --locked -- -D warnings`
  0 findings (thirteen real findings surfaced en route and were fixed
  in code, not suppressed: two `too_many_lines` (the surveys extracted
  into documented helpers), `option_option` (the per-package tag state
  became `resolved_tag` + a provenance flag), `assigning_clones`,
  `ref_option`, `doc_markdown`, a manual `Option::map`, two
  `single_match_else`, an over-long example `main` (extracted into a
  `print_survey` helper), and test-side `assert_is_empty`,
  `single_char_pattern` and `needless_borrows`); the
  lint-hygiene grep empty; the full `test --workspace --all-features
  --locked` green; `--benches` compiled and run; the cache
  conflict-feature lane (`--features loom` release) green.
- **Honest state at cut-off.** The corpus-lane acceptance (20
  real-world foreign configs — the M4.7 full milestone), the changesets
  tag-grammar VERIFY, M5.3 OIDC, the corpus-wide pnpm differential, the
  M2.6 dhat re-audit and the two p50 perf-budget misses are unchanged.
  Tree left dirty (no commits).
- **Next session start point.** (1) the M4.7 corpus-lane acceptance
  (the knope/release-plz sources' live-config lane), (2) the M2.6 dhat
  re-audit with the cache warm, (3) M5.3 OIDC on a live CI runtime,
  (4) the corpus-wide pnpm differential as an xtask subcommand.

### 2026-11-15 — M4.7 step 2: the §16.2 `migrate --write` (ADR-0023) + the ADR-0021 routing completions (tree left dirty, no commits)

- **Inventory at session start.** Main was green at `50aaf92` (the
  M4.7 step-1 migrate slice): baseline re-verified on a fresh
  toolchain (stable 1.100.0, `RUSTFLAGS="-C debuginfo=0"` — the disk
  ceiling lesson applied from the first build) at 105 suites, 857
  tests, 0 failed. The declared start points were (1) M4.7 step 2:
  the `migrate --write` path over the validated draft, (2) the
  routing completions (the ADR-0021 map's declared open items), (3)
  the M2.6 dhat re-audit, (4) M5.3 OIDC on a live CI runtime. Picked
  (1) as M1 — the write half of the §16.2 row, report-first by
  design so the decision rules get their own review — and (2) as M2.
- **M1 implemented (no stubs), one full red→green cycle** (the 12
  engine tests written first — red on E0432 unresolved write-seam
  imports; the 6 CLI gates red on the unknown `--write` flag — then
  implemented):
  - Design decisions (ADR-0023): what the human reviewed is exactly
    what lands (the plan carries the report's draft verbatim — no
    re-serialization drift); the migration never guesses (two
    configured tools refuse with every candidate named — picking one
    would silently discard the other's mappings, the exact
    "half-understood config" failure ADR-0022 refuses); an existing
    config is never clobbered (create-only, the file byte-identical
    after the refusal — the re-run refusal *is* the honest
    "already migrated" signal); absence is a value (`NothingConfigured`
    exit 0, the sink never touched); the engine stays pure (the
    `DraftSink` seam is object-safe and injected like
    `TagWriter`/`PublishEffect`; `AtomicDraftSink` walks the §9.1
    atomic protocol and refuses a missing parent directory instead of
    mkdir-ing; `MemoryDraftSink` is the deterministic reference fake).
  - The CLI renderers keep the frozen contract byte-stable: the JSON
    `write` block exists only when the flag ran; the report-only
    header text is unchanged (pinned by the existing tests).
  - One real bug caught by the CLI fixtures: a bare `halyard.toml`
    target's `Path::parent()` is the empty path — `"".is_dir()` is
    false, so the AtomicDraftSink refused the honest happy path; the
    empty parent now resolves to `.` (the `-C` chdir lands there),
    pinned by the write test.
- **M2 implemented (M1 green end-to-end first), one red→green cycle**
  (red on E0425 unresolved `config_paths_for` + E0061 on the npm
  signature):
  - The cargo legacy `config` filename: VERIFIED per §0.4 — the Cargo
    Book's Configuration page quotes "Cargo also reads config files
    without the .toml extension … If both files exist, Cargo will use
    the file without the extension." Two facts pinned: the page
    nowhere says the legacy name is deprecated (the module doc's
    earlier "deprecated upstream, out of scope" claim was drift) and
    the extension-less file WINS per directory. The pure
    `config_paths_for` enumerates `config.toml` first and legacy
    `config` last per directory; the resolver's per-key merge then
    reproduces the documented semantics (legacy wins on conflicts, a
    file without the key erases nothing). Five tests pin the order,
    the legacy-only read, the both-files precedence, the no-entry
    merge and the fail-closed unparsable legacy file.
  - The npmrc scoped-registry env variables: the npm docs document NO
    env form for scoped keys (the config page points at the .npmrc
    page, which documents neither) — settled by **probe** against npm
    11.19.1 (the ADR-0017 method), seven probes recorded in the
    research note: the literal `npm_config_@<scope>:registry` name is
    the scoped key's env form (case-insensitive, probe 4); the
    underscore intermediate form is NOT the scoped key — npm reads it
    as an unknown `myorg-registry` key and warns it will stop working
    (probes 1/3); env beats files within the key (probe 5); and the
    scoped key governs a scoped package regardless of the source tier
    — a scoped .npmrc key beats a plain `NPM_CONFIG_REGISTRY` env
    value (the cross-tier probe). The resolver's precedence is now
    exactly npm's observable grammar: publishConfig → scoped key
    (env → npmrc) → plain key (env → npmrc) → default; the
    set-but-empty scoped env var is refused naming the variable. Seven
    tests pin it; the honest POSIX caveat recorded (`@` and `:` are
    not shell identifier characters — CI env maps set the variable).
- **Tests/bench/example totals.** +31 tests / +1 suite over base (12
  engine `tests/migrate_write.rs` + 6 new CLI gates in
  `cli_migrate.rs` + 8 npm scoped-env tests + 5 cargo legacy-config
  tests; 106 suites, 888 tests, 0 failed on the final tree). Bench
  group `migrate_write` (plan + memory-sink run) registered and run
  beside the existing survey groups. Examples `migrate_write` (lands
  the draft in a temp dir, re-loads it through the strict loader,
  demonstrates the create-only refusal) and `resolve_publish_registry`
  (extended with the scoped-env case) run clean.
- **Gates on the final tree.** fmt green; clippy
  `--workspace --all-targets --all-features --locked -- -D warnings`
  0 findings (three real findings surfaced en route and were fixed in
  code, not suppressed: a redundant closure, a `wrote`/`write`
  similar-names pair, and `indexing_slicing` violations in the new
  tests — the no-test-allowance rule held, the tests now use
  `.get()` chains); the lint-hygiene grep empty; the full
  `test --workspace --all-features --locked` green; `--benches`
  compiled and run; the cache conflict-feature lane (`--features loom`
  release) green; the `-D warnings` doc sweep green.
- **Honest state at cut-off.** The knope/release-plz migration
  sources, the M4.7 corpus-lane acceptance, M5.3 OIDC, the corpus-wide
  pnpm differential, the M2.6 dhat re-audit and the two p50
  perf-budget misses are unchanged. Tree left dirty (no commits).
- **Next session start point.** (1) the knope/release-plz
  `MigrationSource` impls (the write path needs no changes), (2) the
  M4.7 corpus-lane acceptance (20 real-world configs), (3) the M2.6
  dhat re-audit with the cache warm, (4) M5.3 OIDC on a live CI
  runtime.

### 2026-11-15 — M4.7 step 1: the migration report (`halyard migrate`, ADR-0022) (tree left dirty, no commits)

- **Inventory at session start.** Main was green at `ea8ef3f` (the
  M5.2-continued routing slice). Baseline re-verified on a fresh
  toolchain (stable 1.100.0, `RUSTFLAGS="-C debuginfo=0"` — the disk
  ceiling lesson applied from the first build):
  `test --workspace --all-features --locked` green at base (103
  suites, 833 tests, 0 failed — exactly the previous session's
  record). The declared start points were (1) M5.3 OIDC (needs a live
  CI runtime — still deferred), (2) the corpus-wide pnpm differential
  (no pnpm binary in the sandbox), (3) the M2.6 dhat re-audit, (4)
  the publish `--resume` UX flag (deliberately not a flag per the
  M5.2 log). Picked instead the last §16.2 command absent from the
  wired set that needs no network and no new dependency: M4.7 step 1
  (`migrate` — "read configs, emit report"), report-only so the
  §16.2 write step stays its own reviewed slice. M2 candidate: the
  routing completions (the ADR-0021 map's declared open items); the
  M1 work filled the session budget honestly, so M2 is DEFERRED with
  the inventory recorded, never half-wired.
- **M1 implemented (no stubs), one full red→green cycle** (the 19
  engine tests written first against the planned API — red on E0432
  unresolved `halyard_config::migrate`; the 5 CLI tests red against
  the absent command — then implemented):
  - Design decisions (ADR-0022): the report precedes the write (the
    draft is a report string, validated through the strict loader
    before it is surfaced — the write step is its own slice);
    absence is a value (`configured: false`, the command runs on any
    directory with no git repo and no `halyard.toml` yet); a
    configured-but-unparseable foreign file fails closed
    (`ConfigError::Migrate`, E-CFG-MIGRATE + page + index row) — a
    half-understood foreign config is never mapped, because a
    mis-mapped tag template or pre-major pin silently changes what
    releases; the pins preserve the foreign tool's *effective*
    behavior including its documented defaults (release-please's
    pre-1.0 defaults differ from halyard's §10.2 — the rows say which
    side is the foreign default); semantic differences are rows with
    reasons, never dropped; the seam (`MigrationSource`) is the
    extension point the later migrate slices fill, exercised by a
    test-local source so the trait is proven downstream-usable.
  - The grammars were VERIFIED per §0.4 before any mapping
    (`docs/research/migrate-config-grammars.md`): the release-please
    manifest-releaser documentation and the checked-in config schema
    (defaults quoted verbatim), the changesets config page. One
    honest open item: the changesets git-tag grammar — the docs page
    quoting the `name@version` shape no longer exists at `main`
    (404 on the blob and raw URLs, the snippet survives only in
    search caches) — the survey carries an advisory row ("set
    `repo.tag_style` against the repository's existing tags") and
    maps no changesets-derived template, rather than shipping an
    unverified claim.
  - TDD lesson pinned en route: the report rows are name-sorted, so
    the tests select surveys by tool name — the first test draft
    indexed `report.tools[0]` and passed the changesets survey off
    as the release-please one (caught by the failures, fixed in the
    tests; the engine never relied on order beyond the documented
    sort).
- **M2 (the routing completions) deferred, not started.** The cargo
  legacy `config` filename and the npmrc scoped-registry env vars
  stay declared open items on the ADR-0021 map. Reason: the M1 slice
  (engine + CLI + docs + gates) consumed the session budget; shipping
  a half-verified resolution extension would break the M2 bar ("fully
  implemented, no partial milestone"). The inventory is recorded so
  the next session starts clean.
- **Micro-debt/docs drift fixed (additive, not milestone
  substitutes).** The changelog minijinja errors now walk their
  source chain (`render_template_source` stringified only the top
  error — the `E-CHLOG-TEMPLATE` diagnosis was losing the "caused by"
  line the fix hangs on); the `check` JSON print path's
  `unreachable!("the check document is an object")` replaced with a
  map-first build (no panic surface on a live path); the README
  daily-commands list completed (`probe_registries` and
  `release_pipeline` examples shipped without their rows) and the
  status paragraph brought current (the publish-auth and
  custom-registry rows, and this slice, were missing); the CLI README
  command table gained the `migrate` row; the arch doc §16.2 as-built
  note names the wired `migrate`.
- **Tests/bench/example totals.** +24 tests / +2 suites over base
  (19 engine `tests/migrate.rs` + 5 CLI `tests/cli_migrate.rs`; 105
  suites, 857 tests, 0 failed on the final tree). Bench `migrate`
  registered and run (release-please 4-package survey ≈7.3 µs,
  changesets ≈8.9 µs, empty two-source survey ≈103 ns). Example
  `migrate_report` runs clean.
- **Honest state at cut-off.** The §16.2 `migrate --write` step, the
  knope/release-plz sources, the M4.7 corpus-lane acceptance (20
  real-world configs), the changesets tag-grammar VERIFY, M5.3 OIDC,
  the corpus pnpm differential and the M2.6 dhat re-audit are
  unchanged. Tree left dirty (no commits made).
- **Next session start point.** (1) M4.7 step 2: the `migrate
  --write` path over the validated draft (its own reviewed slice),
  (2) the routing completions (legacy `config` filename + npmrc
  scoped env vars), (3) the M2.6 dhat re-audit with the cache warm,
  (4) M5.3 OIDC on a live CI runtime.

### 2026-11-15 — M5.2 continued: the custom-registry routing (ADR-0021) + the per-target npm routing (tree left dirty, no commits)

- **Inventory at session start.** Main was green at `4d82f45` (the
  M4.5 check-comment slice). Baseline re-verified on a fresh toolchain
  (stable 1.100.0, `RUSTFLAGS="-C debuginfo=0"` — the sandbox disk
  ceiling lesson from the M5.3 log, applied from the first build):
  the workspace compiled clean with `test --workspace --all-features
  --locked --no-run`. The declared start points were (1) M5.3 OIDC
  (needs a live CI runtime — still deferred), (2) the corpus-wide pnpm
  differential (no pnpm binary in the sandbox), (3) the M2.6 dhat
  re-audit, (4) the third-party cargo registry resume client over the
  routing map. Picked (4) as M1 — the routing gap every session since
  M5.3 pinned honestly (`E-PUB-REGISTRY` for any non-default registry
  name, the argv grammar complete and unit-pinned since M4.5) — and
  the npm-side per-package routing as M2, its mirror over the same
  seam.
- **M1 implemented (no stubs), one full red→green cycle** (the 17
  contract tests written first — red on E0432 unresolved
  `halyard_eco_cargo::registry_config`; the CLI dry test red on the
  recorded `E-PUB-REGISTRY` refusal — then implemented):
  - Design decisions (ADR-0021): the cargo configuration is the one
    source of truth for where a registry lives (the probe and the
    spawned `cargo publish --registry <name>` read the same file, so
    they cannot disagree); only the `sparse+` HTTP form is probeable
    (the resume probe is one GET of an NDJSON file — a git index is
    an honest decline naming the protocol); a malformed config file
    is refused, never read as "the entry is absent" (that would
    half-honor a broken configuration and could route the probe at
    the wrong registry); the resolution is eager, in the shell,
    before any call (the `RegistryRouter` seam keeps its
    `&dyn Registry` lifetime shape; interior mutability would buy
    nothing over validate-then-act).
  - The one real lesson pinned during the loopback e2e: the fake
    index's bounded serve loop exited after its scripted request
    count while the run was still between probes (connection refused
    at the visibility stage) — the server became unbounded with an
    observable served-count, the house fake-forge discipline (the
    script tail must cover the run, never exit early).
- **M2 implemented (M1 green end-to-end first), red→green** (the npm
  contract + engine tests first — red on E0432 unresolved
  `halyard_eco_npm::publish_registry` / `TargetRouter` and E0599 no
  method `registry_for`; then implemented): the npm resolution is
  per-package because two npm packages of one workspace may publish
  to different registries; `registry_for` is a trait DEFAULT method
  (the `close_pr`/`upsert_pr_comment` evolution convention) so every
  shipped router keeps working unchanged. The engine test that
  exposed the §13 semantics honestly: an uncoupled override registry
  leaves the visibility wait unconfirmed → the dependent is skipped
  `upstream_invisible:` — the test now couples the modelled upload
  and pins both that gate and the per-client probe logs.
- **Tests/bench/example totals.** +41 tests / +2 suites over base
  (17 cargo-config contract + 6 CLI custom-registry + 14 npm
  resolution + 4 engine routing — 103 suites, 833 tests,
  0 failed on the final tree). Benches `registry_config` (resolve
  sparse-among-4 ≈3.4 µs, git-index decline ≈3.5 µs, URL classify
  ≈ sub-µs) and `target_router` (by-name ≈27 ns, override hit
  ≈29 ns, miss-among-64 ≈33 ns) registered and run. Examples
  `resolve_registry` + `resolve_publish_registry` run clean.
  Full gate battery green on the final tree: fmt, clippy
  `--workspace --all-targets --all-features --locked -- -D warnings`
  (0 findings), the lint-hygiene grep (empty), `test --workspace
  --all-features --locked`, `test --workspace --benches`, the
  `-D warnings` doc sweep, and the cache crate's conflict-feature
  lanes (`--features loom` release run per tests/loom.rs's header;
  the `miri` pair stays the nightly toolchain lane).
- **Honest state at cut-off.** The npmrc scoped-registry environment
  variables, source replacement, the legacy `config` filename, OIDC
  trusted publishing, the corpus pnpm differential and the M2.6 dhat
  re-audit are unchanged. Tree left dirty (no commits made).
- **Next session start point.** (1) M5.3 OIDC trusted publishing
  (VERIFY on a live CI runtime), (2) the corpus-wide pnpm differential
  as an xtask subcommand, (3) the M2.6 dhat re-audit with the cache
  warm, (4) the publish `--resume` UX flag over the engine's I2 loop.

### 2026-11-15 — M4.5 step 1 check-comment mode (ADR-0020) + M5.3 continued `--registry` argv mapping (tree left dirty, no commits)

- **Inventory at session start.** Main was green at `93d7043` (the M5.3
  publish-auth slice). Baseline re-verified on a fresh toolchain
  (stable, `RUSTFLAGS="-C debuginfo=0"` — the target dir hit the disk
  ceiling twice before the flag was applied consistently; a linker
  bus-error is resource, not code, as recorded in the M5.3 log):
  `test --workspace --all-features --locked` green at base. The
  declared start points were (1) M5.3 continued (OIDC + the
  `--registry` flag mapping), (2) the corpus-wide pnpm differential
  (no pnpm in the sandbox), (3) the M2.6 dhat re-audit, (4) the
  `github-release-notes` PR-comment mode. Picked (4) — §11.6's
  "optional PR comment", deferred in every session log since M5.5 —
  as M1, and the `--registry` flag mapping (the other half of item 1
  that is offline-verifiable; OIDC needs a live CI runtime, still
  deferred) as M2.
- **M1 implemented (no stubs), one full red→green cycle** (the seam
  contract, adapter wire contract and renderer tests written first —
  red on E0432 unresolved `halyard_forge::CommentSpec` /
  `halyard_changelog::check_comment` + E0599 no method
  `upsert_pr_comment` on record — then the CLI gates, then
  implemented):
  - Design decisions (ADR-0020): the marker is the idempotence key
    (no state file — the find-first convention one level up); the
    renderer is markdown-only and lives in `halyard-changelog` beside
    the §11.3 body renderer (§7.2: the forge dependency stays on the
    consumer side); both verdict shapes post (a rejected title teaches
    in the comment, then still fails `E-CLI-TITLE`); the comment mode
    is live-only with the honest `E-CLI-COMMENT` decline (no `--dry` —
    a comment forecast would probe the network or lie about its
    target); the target is the PR found by head branch, reusing the
    adapter's find exchange.
  - One real lesson pinned during the loopback tests: the find
    exchange parses a PR **list**; the first scripted bodies were
    objects, the parse failure rode the governor's documented
    transport retry into a second GET, and the fake forge then waited
    forever on its script tail (`server.join()` hang). The scripts
    were corrected to the list shape — the transport-retry contract
    itself stays untouched and now has a witnessed exercise.
- **M2 implemented (M1 green end-to-end first), red→green** (the argv
  signature change failed E0061 at its two call sites on record):
  `cargo_publish_args(manifest, registry, dry)` + `is_default_registry`
  (the no-guess rule: the two default spellings emit no flag — the
  tool's documented `registry.default` governs), VERIFIED per §0.4
  (Cargo Book fetched, verbatim quote in the research note), the
  effect passes `target.registry` through, and the CLI test pins the
  routing gate honestly — a third-party cargo index has no
  resume-probe client, so the run refuses `E-PUB-REGISTRY` **before
  any spawn** (dry included, zero side effects). The end-to-end
  custom-registry publish waits on a client joining the routing map;
  the argv grammar is complete and unit-pinned, never a stub.
- **Tests/bench/example totals.** +35 tests / +3 suites over base
  (101 suites, 792 tests, 0 failed on the final tree): 10 seam + 7
  adapter + 7 renderer + 3 CLI gates + 3 CLI loopback + 5 adapter argv
  (integration) + inline matrix extensions. Bench `pr_comment`
  registered and run (render-6, marker-upsert create and rewrite
  shapes); example `render_check_comment` runs clean. Full gate
  battery green on the final tree: fmt, clippy
  `--workspace --all-targets --all-features -- -D warnings` (0
  findings), the lint-hygiene grep (empty), `test --workspace
  --all-features --locked` (101 suites, 792 tests), `test --workspace
  --benches`, the `-D warnings` doc sweep, `check-layers`,
  `schema-check`.
- **Honest state at cut-off.** Fork-aware comment targeting (M11 bot
  mode), OIDC trusted publishing (needs a live CI runtime), the
  third-party registry resume client, the corpus pnpm differential,
  the M2.6 dhat re-audit and the two p50 perf-budget misses are
  unchanged. Tree left dirty (no commits made).
- **Next session start point.** (1) M5.3 OIDC trusted publishing
  (VERIFY on a live CI runtime), (2) the corpus-wide pnpm differential
  as an xtask subcommand, (3) the M2.6 dhat re-audit with the cache
  warm, (4) the third-party cargo registry resume client over the
  routing map.

### 2026-11-15 — M5.3 publish-auth plumbing (ADR-0019) + provenance observability (tree left dirty, no commits)

- **Inventory at session start.** The base commit `651e8e9` re-verified green
  on a fresh toolchain (stable 1.100.0): 97 suites, 733 tests, 0 failed —
  after two sandbox infrastructure repairs (the 9.9 GB disk filled twice;
  `CARGO_PROFILE_DEV_DEBUGINFO=0` proved ineffective on this cargo build,
  `RUSTFLAGS="-C debuginfo=0"` brought the target from ≈8.7 GB to ≈1 GB; a
  linker bus-error at 100 % disk was resource, not code). The declared
  start points were (1) M5.3 OIDC/token auth plumbing + secret scrubbing,
  (2) the corpus pnpm differential, (3) the M2.6 dhat re-audit, (4) the
  `github-release-notes` PR-comment mode. Picked (1) — the `PublishEffect`
  doc's own "our job: … auth plumbing" gap: no credential resolution, no
  injection point, tool stderr landing verbatim in report rows (a token a
  misconfigured tool echoes leaks into every renderer), and the
  `publish-tooling.md` research note's explicit deferral to M5.3's auth
  plumbing. OIDC is deferred honestly: the M0.6 research's requirements
  (`id-token: write`, hosted runners) need a live CI runtime this sandbox
  cannot exercise; the token half + scrubbing is fully offline-verifiable.
- **M1 implemented (no stubs), one full red→green cycle** (the 16-test
  engine contract written first, red on E0432/E0599 unresolved
  `halyard_publish::auth`/`PublishError::Auth`; then the 5 CLI tests,
  red against the pre-auth effect — 7 suites ran before the failing pair
  stopped scheduling, exactly the two new tests failed):
  - The env grammar VERIFIED per §0.4 (fetched pages, verbatim quotes in
    `docs/research/publish-auth-env.md`): cargo reads its token variables
    directly; pnpm expands `${NAME}` **only in user-level auth files**
    (the project `.npmrc` credential interpolation is deliberately
    ignored — "a malicious repository could otherwise send your CI tokens
    to a registry it controls"), which is why halyard injects env vars
    and never writes auth files. Injection via env is therefore support
    for the tools' own contracts — ADR-0019 records honestly that the
    seam's net-new capability is resolution/provenance, the fail-closed
    empty-token refusal, the scrubbing contract and the injection point
    a future non-env provider fills, not a token *transport*.
  - Design decisions: absence is `AuthOutcome::Absent` (a value, never an
    error — `cargo login` and the user-level auth file are invisible to
    halyard), a present-but-empty candidate is `E-PUB-AUTH` naming the
    variable (an empty bearer token is a doomed spawn + a confusing 401),
    the capture is one deterministic snapshot shared by `credentials()`
    and `secrets()` (the scrubbing contract cannot drift from what was
    injected), and the `Debug`/`Display` paths redact values by
    construction (pinned by test).
- **M2 implemented (M1 green end-to-end first), red→green** (the 2 CLI
  observability tests first, red against the frozen report shape, then
  implemented): `auth_summary` (the engine-side provenance helper:
  registry → `env:VARIABLE` or `none`, name-sorted I3, values never),
  the `publish` JSON `auth` block + the human/gha `publish-auth (M5.3):`
  tail, and the `doctor` `publish-auth` check (the same capture the run
  performs — visible before the run, not after the first 401).
- **Tests/bench/example totals.** 19 engine tests (`tests/auth.rs`) + 9
  CLI `publish` tests (5 new: scrub-leak, empty-token refusal,
  provenance JSON, its byte-stability, the doctor row); bench `auth`
  registered and run (table capture ≈2.3 ns, 14-target resolution ≈
  1.4 µs, 1 KiB scrub pass sub-µs); example `publish_auth` runs clean.
  Full gate battery green on the final tree: fmt, clippy
  `--workspace --all-targets --all-features -- -D warnings` (0 findings),
  the lint-hygiene grep (empty), `test --workspace --all-features
  --locked` (98 suites, 756 tests, 0 failed), the `-D warnings` doc
  sweep, `check-layers` (18 crates), `schema-check` (both schemas).
- **Honest state at cut-off.** OIDC trusted publishing, the
  `--registry` flag mapping and provenance attestations stay M5.3+ (the
  recorded VERIFY items); the corpus pnpm differential, the M2.6 dhat
  re-audit and the two p50 perf-budget misses are unchanged. Tree left
  dirty (no commits made).
- **Next session start point.** (1) M5.3 continued: OIDC/trusted
  publishing (VERIFY research on live CI) and the `--registry` flag
  mapping over the same seam, (2) the corpus-wide pnpm differential as
  an xtask subcommand, (3) the M2.6 dhat re-audit with the cache warm,
  (4) the `github-release-notes` PR-comment mode.


### 2026-11-14 — §8.8 smart-minimal-fetch (M4.6 step 1, ADR-0016) + M5.1 registry existence probes (ADR-0017) (branch `feat/fetch-and-registry-probes`, tree left dirty, no commits)

- **Inventory at session start.** Main was green at `ba95b52` (the M5.5
  governor slice). Baseline `cargo test --workspace --all-features
  --locked` re-run green on a fresh toolchain (stable 1.100.0): 86
  suites, 658 passed, 0 failed — including the cache crate's
  `miri`+`loom` test scaffolding combined under `--all-features` (the
  documented mutually-exclusive pair only changes the test backends).
  The declared open start points were (1) publish (M5), (2) the corpus
  pnpm differential, (3) the M2.6 dhat re-audit, (4) the
  `github-release-notes` PR-comment mode. Picked §8.8 (fetch) as M1 —
  the last §16.2 command absent rather than wired, with no network
  research surface (a local `file://` remote exercises the whole
  effect) — and M5.1's registry probes as M2 (the §7.1-reserved crate,
  the §15.4-reserved `E-REG-*` family, the §13 primitive the M5.2
  publisher needs first).
- **M1 implemented (no stubs), one full red→green cycle** (17 engine
  tests written first against the planned API, red on the unresolved
  `halyard_git::fetch`, then implemented):
  - `halyard-git/src/fetch.rs` — the §8.8 subsystem: pure planner +
    live effect, mirroring the `tag`/`push` seam conventions (argv
    arrays, `GIT_TERMINAL_PROMPT=0`, deterministic reports, report
    tallies derived from rows). The empirical probes that shaped the
    design (all pinned by tests, recorded in ADR-0016): on a **complete**
    clone `--shallow-exclude` fails the whole fetch with `no commits
    selected for shallow requests` (it is a deepening option); excluding
    the *fetched* tag itself swallows the request (exit 128); excluding
    the package's **previous** release tag while fetching the newer one
    succeeds on shallow clones and bounds the deepening.
  - Template resolution rides the planner's own rule: the first engine
    run exposed that discovery templates differ from the §6.1
    single-package default — `effective_template` became `pub` and the
    planner takes `&PlanConfig`, so fetch, plan and the tag phase
    cannot disagree (I3).
  - `halyard fetch` (CLI): discovery + config only — deliberately not
    the history walk (a shallow clone, the state fetch repairs, fails
    the walk `E-GIT-SHALLOW`). Base branch = the config override,
    defaulting to `HEAD` like the planner; remote = `origin` (the §11.2
    convention). No `--dry` by design: the remote listing is the one
    input an offline forecast cannot have (ADR-0016 records the
    decision).
- **M2 implemented (M1 green end-to-end first), one red→green cycle**
  (12 contract tests first, red on the missing crate):
  - `halyard-registry` (new crate, layer rule `registry → forge`): the
    `Registry` seam + `CratesIo`/`Npm` clients + `wait_until_visible`
    with the injected `RegistrySleeper`. The URL grammar was VERIFIED
    per §0.4 — Cargo Book fetched, npm registry docs fetched, and both
    registries probed live (`se/rd/serde`, `3/l/log`, missing → 404,
    npm scoped unencoded, npm missing → 404, `versions` map keys) —
    recorded in `docs/research/registry-existence-probes.md` with the
    three remaining VERIFY items (mirror URL encoding, rate limits at
    scale, yanked-version policy) honestly open.
  - Wire-up: `halyard doctor --registries` — the opt-in probe against
    immutable versions (`serde@1.0.0`, `react@0.0.1`; registries never
    overwrite versions, so the probes are stable and read-only).
- **Micro-debt/docs drift cleaned (additive, not milestone
  substitutes).** The cache write-back failures in `halyard-git` were
  mapped onto `E-GIT-OBJECT` — a durability failure is not an
  object-read error; now `GitError::Cache` (`E-GIT-CACHE` page + index).
  The `TagCandidate::version` field was constructed `None` and never
  read or written by anyone (dead since M2.1) — removed with the
  doc note that per-package versions derive on demand. The root
  README's daily-commands list was 8 example rows behind the shipped
  examples (lint_title, tag_phase/forecast/push/target, fetch_plan,
  render_release_notes, render_user_template, pr_preview,
  governed_release, forge_exchange, app_exchange) — completed; the
  `Command` enum doc no longer lists `fetch` among the absent commands.
- **Tests/bench/example totals.** 17 fetch engine tests + 4 CLI fetch
  tests + 12 registry contract tests + 4 doctor-flag/CLI smoke paths
  (the doctor wire rides the existing snapshot suite); benches
  `fetch_plan` (3 groups) + `probe` (2 groups) registered and run;
  examples `fetch_plan` + `probe_registries` run clean.
- **Honest state at cut-off.** The M4.6 corpus checkout-time acceptance
  and the M5.1 nightly live checks belong to their lanes and stay
  deferred; `--deepen` loops are superseded by the terminal full-fetch
  fallback (documented); the M5.2 publisher is the declared consumer of
  the registry seam. Publish/M5.2+, the corpus-wide pnpm differential
  and the M2.6 dhat re-audit remain open. Tree left dirty (no commits
  made).
- **Next session start point.** (1) The M5.2 publisher over the
  `Registry` seam (topo layers, resume, visibility wait — all testable
  against `MemoryTransport`), (2) the corpus-wide pnpm differential as
  an xtask subcommand, (3) the M2.6 dhat re-audit with the cache warm,
  (4) the `github-release-notes` PR-comment mode.

### 2026-11-14 — M4.1c App-JWT auth (ADR-0014) + walk-provenance reporting (branch `feat/github-app-jwt-auth`, tree left dirty, no commits)

- **Inventory at session start.** The base commit `3122994` was NOT green on
  a fresh toolchain (stable 1.100.0): `halyard-forge-github/tests/contract.rs`
  carried 11 `Option::unwrap_or_else(|error| unreachable!(…, {error}))`
  sites (E0593 — Option closures take 0 args), and the CLI loopback suite
  had two more latent reds (see the repairs below). All repaired first.
- **M1 (M4.1c) implemented, one red→green cycle** (14 contract tests written
  first, red on the unresolved API, then implemented). Design per ADR-0014:
  `ring` RS256 (no new lockfile package), in-crate PEM/DER grammar with a
  ~60-line mini DER reader (the 0x81/0x82 long form initially skipped the
  tag byte — found by the fixture round-trip), conservative 45-min TTL, the
  injected `EpochClock`, and the fail-closed env grammar
  (`HALYARD_APP_ID` + key path/inline + installation id; any App var present
  commits the run to App mode). The CLI loopback e2e proves mint → probe
  over the real transport; the smoke JSON carries `auth_mode`.
- **M2 (the declared "cache stats wiring") implemented, red→green** (4
  `cli_stats` tests first): `render::HistoryStats` reads the engine's public
  counters (no second walk) and surfaces them in the plan human/GHA renders
  and the pr/release JSON documents + human tails. `plan --json` stays the
  frozen schema (pinned). Warm-cache zero counters are by design (§8.7) —
  the determinism pin rides `--no-cache`.
- **Honest state at cut-off.** The CLI test/clippy tail is unfinished (2
  failing `cli_stats`/`cli_pr` pins + 2 bin-test compile errors left by the
  final restructure iteration); the engine, forge-github, forge and all
  other suites are green; fmt/lint-hygiene/check-layers/benches green.
  Publish (M5), the pacing governor (M5.5), the corpus pnpm differential
  and the dhat re-audit stay open.
- **Next session start point.** (1) Finish the CLI tail (the precomputed
  `walk_line` refactor needs its two test pins updated), (2) re-run the full
  clippy battery, (3) then the corpus pnpm differential or publish (M5).



### 2026-11-14 — Tag-target pinpoint slice (§11.1 precision rule, branch `main`, tree left dirty, no commits)

- **Inventory at session start.** Main was green at `57d3864` (the tag-phase slice tree).
  Baseline gates re-run green on a fresh toolchain (stable 1.100.0): fmt, clippy `-D warnings`,
  `test --workspace --all-features` (67 binaries), `--benches` compiled. The declared start
  points were (1) the live release shell, (2) M4.1b, (3) the corpus pnpm differential, (4) the
  dhat re-audit. Picked the §11.1 precision rule instead — the manifest-blob pinpoint walk the
  previous slice explicitly deferred — because it completes the tag-phase chain without any
  network surface: it is the last piece the forecast needs before the live shell, and it ships
  the honest `--filter=blob:none` degradation the deferral note promised.
- **M1 implemented (no stubs), one full red→green cycle** (the 11 integration tests written
  first, red on the unresolved `halyard_git::tag_target` import, then implemented):
  - `halyard-git/src/tag_target.rs` — the `TagTargetResolver` seam (`&dyn`), the real
    `GixTagTargetResolver`: a bounded first-parent walk from the request tip down to
    (exclusive) the package's release base; manifest blob-OID *change events* are the
    candidates (identical OIDs share bytes, hence a version, and skip blob reads entirely);
    a candidate is verified by a caller-supplied `ManifestVersionProbe` over the manifest
    bytes (the commit carries V, its parent does not — a later manifest edit that kept the
    version cannot steal the pinpoint). Unreadable objects degrade to the documented HEAD
    fallback: missing manifest blobs (the `--filter=blob:none` case), missing parent commits
    (shallow cuts), exhausted bounds, never-committed versions; only an unreadable request
    *tip* is a hard `E-GIT-OBJECT` (the write path's target-existence contract). No new
    error codes — the existing `E-GIT-OBJECT`/`E-GIT-OPEN` surface covers the seam.
  - `directives_for_pending_resolved` — the reconciliation queue → directives glue with
    provenance (`ResolvedDirective`): per-release manifest path + ecosystem from the
    workspace, release base from the digest, stale package names keep the fallback
    directive so the phase still runs.
  - TDD: `tests/tag_target.rs` — pinpoint vs `git` ground truth, the later-manifest-edits
    guard, the never-committed fallback, the initial release reaching the root commit, a
    **real** `partial_blobless_clone()` fixture degrading to HEAD (no error), the walk
    bound stopping the search, a no-ff merge landing pinpoints the merge commit, two
    packages pinpointing independently, the glue + stale-name fallback, the probe's
    ecosystem contract, and determinism (I3, resolve twice).
  - `tag_target` bench (pinpoint-500 ≈ 18.7 ms, fallback-500 ≈ 18.7 ms over a 500-commit
    window) and the `tag_target` example (reconcile → resolve → print provenance).
- **M2 implemented (M1 green end-to-end first): CLI `release --dry` resolved targets.**
  The shell's probe dispatches to the ecosystem adapters —
  `halyard_eco_cargo::manifest_version` / `halyard_eco_npm::manifest_version` (new public
  leaf helpers over the existing parsers, unit-tested; inherited versions are routed at
  their authoritative workspace manifest) — and every forecast row now carries its target
  provenance (`manifest pinpoint` vs `HEAD fallback`) in json/human/gha. 3 new CLI tests;
  the red→green cycle was demonstrated by stashing the CLI wiring (tests red on the
  pre-provenance renderer, green after restore). `release_pipeline.rs` — the missing
  end-to-end consumer example: discover → reconcile → pinpoint → tag-phase forecast →
  release machine over `PreviewForge`.
- **Micro-debt/docs drift cleaned (additive, not milestone substitutes).** The
  `// crc placeholder` comment in `halyard-cache` was misleading (the record CRC is
  computed and patched in place three lines below — the checksum was never missing);
  the previous session row's "dirty; no commits" branch claim was stale again (the slice
  landed as `0d0a9da..57d3864`) — corrected; the arch doc got as-built notes for §11.1
  (pinpoint shipped), §11.4 (`close_pr`/`calls()` + the two shipped adapters), §15.6
  (the perf gate is wall-clock budgets per ADR-0010, the actual nightly lanes) and §16.2
  (wired commands vs the table); root README layout rows (schema/completions,
  schema-check-plan, tag-target pinpoint) and the CLI README `release --dry` row;
  nightly.yml's `differential` job ran `xtask differential --all` — a subcommand that
  does not exist (the job could never have run) — it now runs the real differential
  test suites (`eco-cargo`, `eco-npm`, `discover`), which skip honestly when the
  ground-truth binary is absent.
- **Honest state at cut-off.** The live release shell (annotated tag creation wired into a
  `release` execution mode, tag push with CAS, forge releases) remains open — the forecast
  now resolves §11.1-precise targets, the write path exists, still no CLI command mutates a
  repository except `apply`. M4.1b (GitHub REST adapter), `github-release-notes`, the M2.6
  dhat re-audit and the corpus pnpm differential are unchanged. Two p50 perf budgets remain
  missed (sandbox CPU ceiling, recorded in `bench/measurements.toml`) — the CI perf-gate
  job is expected red until the M2.6 close-out.
- **Next session start point.** (1) The live release shell: a `release` execution mode over
  `GixTagWriter` + the tag push effect (§11.2 CAS), (2) M4.1b REST adapter (VERIFY research +
  ADR per §0.3), (3) the corpus-wide pnpm differential as an xtask subcommand, (4) the M2.6
  dhat re-audit with the cache warm.

### 2026-11-14 — Tag-phase write path + forecast slice (§11.1 git-side effects, branch `main`, tree left dirty, no commits)

- **Inventory at session start.** Main was green at `ebb1dfd` (the
  reconciliation slice tree). Baseline gates re-run green on a fresh
  toolchain (stable 1.100.0): fmt, clippy `-D warnings`, `test
  --workspace --all-features` (67 binaries), `--benches` compiled. The
  declared next start points were (1) M4.1b, (2) the live git-side
  tag/push effects consuming `pending_releases`, (3) the corpus pnpm
  differential, (4) the dhat re-audit. Picked (2) — the tag-phase write
  path + its forecast — as the milestone pair: it is the seam the whole
  §11.1 chain has been pointing at, it needs no network research, and
  the git engine had **zero** write-path code (grep across
  `halyard-git/src` for `edit_reference|write_object|create_tag|reflog`
  was empty).
- **M1 implemented (no stubs), one full red→green cycle** (the 11
  integration tests written first, red on the unresolved
  `halyard_git::tag` import, then implemented):
  - `halyard-git/src/tag.rs` — the `TagWriter` seam (`&dyn`,
    create_annotated_tag), the real `GixTagWriter` (owns the opened
    repo): name validation first (git reference rules on
    `refs/tags/<name>`, nothing written on rejection), target existence
    (commit-only), idempotence pre-check (annotated **and** lightweight
    tags peel to the same comparison), annotated tag object written via
    `gix::objs::Tag` (I3: the object is a pure function of
    name+target+message+identity — pinned byte-identical across two
    repositories), create-only ref update (`PreviousValue::MustNotExist`
    — two racers cannot both claim a create; a loser re-reads and
    classifies `AlreadyTagged`/conflict instead of duplicating or
    moving), fail-closed `GitError::TagWrite` (`E-GIT-TAG`) +
    `GitError::TagConflict` (`E-GIT-TAG-CONFLICT`, never moves a release
    tag) with docs pages.
  - `run_tag_phase` — the §11.1 step-2 git-side executor over
    `&dyn TagWriter`: canonical sort+dedup (I3 armor on top of the
    reconciliation queue's own order), continue-on-error (I2: the re-run
    reaches the uninterrupted state with zero duplicates), deterministic
    report (`TagPhaseReport`/`TagPhaseRow` with counters).
  - `directives_for_pending` — the reconciliation queue → directives
    glue; target = snapshot HEAD, the documented §11.1 fallback (the
    manifest-blob pinpoint walk reads blobs and would break
    `--filter=blob:none` clones — explicitly deferred, not stubbed).
  - Tagger identity is caller-supplied (`TaggerIdentity`, name + email +
    epoch seconds) — the shell owns git config + clock; an incomplete
    identity is a fail-closed `E-GIT-TAG`, never a synthetic default.
- **M2 implemented (M1 green end-to-end first): `PreviewTagWriter`** —
  the dry-run writer mirroring the forge seam's `PreviewForge`: probes
  are real reads (validation, target existence, existing-tag
  classification — the forecast can never disagree with the live run),
  mutations forecast deterministically (`Created` with the
  neutral-identity shape oid, a documented constant — no clock, no
  config leakage) and write **nothing**. Consumed by the new CLI
  `release --dry`: reconcile → pending → directives → forecast through
  the same `run_tag_phase` executor the live run will use; human/json/
  gha renderers; live mode declines honestly (`E-CLI-RELEASE` + docs
  page). Tests: 4 preview-writer integration tests + 5 CLI tests
  (byte-stable JSON I3, zero-ref-write I1 guard, tagged repo owes
  nothing, honest decline with the JSON error-code document). Example
  `tag_forecast.rs`; bench group `tag_phase_forecast_64`.
- **Benches.** `crates/halyard-git/benches/tag_write.rs` (registered
  `[[bench]]`): `tag_write_create` (rotating 100k name pool, self-checked
  against pool exhaustion) measured ≈124 µs per annotated tag
  (object write + create-only ref update); `tag_phase_already_64` (the
  CI re-run no-op path) and `tag_phase_forecast_64` (the `release --dry`
  path) — both probe-only, re-runnable.
- **Micro-debt/docs drift cleaned (additive, not milestone substitutes).**
  `E-CHLOG-TEMPLATE` had no page (the code shipped at
  `halyard-changelog/src/lib.rs` without it; `tests/template.rs` even
  cites the page) — page added + indexed; the xtask module doc table
  listed 7 of 13 wired subcommands — completed; the architecture doc
  §8.1 still named the gix `max-performance-safe` feature (gone in gix
  0.90 — drift the manifest comment already carried) — as-built note
  added; CLI README + root README status rows for `release --dry`; the
  previous session row's stale "dirty; no commits" claim corrected (the
  slice landed as `bb2be7a..ebb1dfd`).
- **Honest state at cut-off.** The live release shell (annotated tag
  creation wired into a `release` execution mode, tag push with CAS,
  forge releases) remains open — the write path and the forecast exist,
  no CLI command mutates a repository yet (`apply` stays the only
  writing command). M4.1b (GitHub REST adapter), the §11.1
  manifest-blob pinpoint walk, `github-release-notes`, the M2.6 dhat
  re-audit and the corpus pnpm differential are unchanged. Tree left
  dirty (no commits made).
- **Next session start point.** (1) The live release shell: a `release`
  execution mode over `GixTagWriter` + the tag push effect (§11.2 CAS),
  (2) M4.1b REST adapter (VERIFY research + ADR per §0.3), (3) the
  corpus-wide pnpm differential as an xtask subcommand, (4) the M2.6
  dhat re-audit with the cache warm.

### 2026-11-14 — M1 reconciliation slice (§11.1 tag phase) + M2.6 CLI cache wiring (branch `feat/tag-phase-reconciliation`, tree left dirty, no commits)

- **Inventory at session start.** Main was green at `6252243` (the
  M4.2b step 1 + M4.3 `check` tree). Baseline `cargo test --workspace
  --all-features` re-run green on a fresh toolchain (stable 1.100.0).
  Two declared gaps from the previous session's "next start point" list
  were picked as the milestones: the tag-phase manifest-vs-tags
  reconciliation (§11.1 step 2 — nowhere implemented, `pr --dry`
  hardcoded an empty pending list) and the M2.6 cache wiring (`--no-cache`
  a declared forward-compat no-op; `GitHistoryOptions.cache` never
  populated by any in-repo caller).
- **M1 implemented (no stubs), TDD with one full red→green cycle** (11
  integration tests + the CLI pin set written first, run red against the
  missing seam, then implemented):
  - `halyard-core/src/release_state.rs` — the §11.1 step-2 stateless
    reconciliation: `reconcile_tags` classifies every release-enabled
    package (`Tagged` = a matching tag already carries the manifest
    version, the re-run is a no-op I1; `Pending` = the tag phase owes
    the release; `Behind` = a reverted merge, reported never tagged
    downward), `pending_releases` is the deduplicated machine-input
    queue (one forge release is keyed by its tag, §11.4). The template
    resolution is the planner's own `effective_template` (config
    override → single-package `v{version}` → discovery template), and
    the `[[package]] release = false` skip set (`PlanConfig::
    release_disabled`) excludes packages exactly as the plan does — the
    tag phase and the plan cannot disagree (I3). Version ordering is the
    same derived `semver::Version` ordering the §8.2 base resolution
    sorts by (pinned by test, including the build-metadata case).
  - `halyard-git`: `GitHistory::tags()` — the enumerated §8.2 tag
    candidates are stored instead of dropped, so the shell reconciles
    without re-scanning refs.
  - CLI `pr --dry`: the machine input now carries the real pending
    releases; the report shows the pending tags, the PROPAGATE gate
    ("prepare phase deferred") and Behind rows; the JSON document
    carries `pending_releases` + `propagated`. The stale "tag phase:
    not forecast" render line is gone. `cli_pr.rs` re-pinned: the
    prepare-phase fixtures now tag `v0.1.0` (manifest == latest tag ⇒
    Tagged), the untagged fixture pins the initial-release tag phase
    (§11.1: a fresh repo tags its manifest version), new tests pin the
    post-merge PROPAGATE forecast (calls 1, no find probe) and the
    Behind state (no create, prepare runs).
  - Bench `crates/halyard-core/benches/release_state.rs` (10×5 / 40×5 /
    200×5 / 40×50 package×tag shapes; 44 µs → 6 ms, negligible against
    the §14.1 planning budgets) and example
    `examples/pending_releases.rs`.
- **M2.6 close-out implemented (no stubs), TDD red→green (3 CLI tests
  first):** `context()` opens `$GIT_DIR/halyard/cache/v1/` (skipped
  under `--no-cache`) and hands `GitHistory::build_with_options` the
  trie `layout_hash` (roots + include/exclude filters, sorted per the
  hash's normalization) and the resolved `config_hash` — a layout or
  policy change invalidates cached attributions cleanly (§8.7). A cache
  that cannot open (corrupt header, unwritable `$GIT_DIR`) degrades to
  an uncached run with a stable-code warning — §8.7: the cache is an
  optimization, never a correctness surface. Tests: warm-run file
  creation + byte-identical cached/uncached plans (I3), `--no-cache`
  leaves no cache file, corrupt cache warns with `E-CACHE-*` and plans
  identically. Bench `crates/halyard-cache/benches/roundtrip.rs`
  (200 records: 638 µs write / 390 µs read; 2000: 5.2 ms) and example
  `crates/halyard-git/examples/cached_walk.rs` (cold populates → warm
  identical digest).
- **Micro-debt cleaned.** The dead `--color` policy computation removed
  (the flag is parsed and documented as reserved for the §16.3
  colour-aware renderers — its result was never consumed); the
  non-UTF-8 tag-name skip in `refs.rs` made explicit (was a silent
  empty-string default); the load-bearing empty-default in
  `tree_diff.rs::path_matches` documented (unfiltered packages must
  attribute non-UTF-8 paths — not a swallow).
- **Docs drift fixed.** `halyard-cli/README.md` documented an
  `apply --dry-run` flag (the real one is `--dry`), a nonexistent
  `apply --refresh-lockfile` (native refresh is automatic on declined
  lockfiles) and a stale command table (apply "not present yet"; pr/
  check missing) — all corrected; the root README status line and the
  `pr --dry` description updated; the `Command` enum doc no longer
  claims `pr`/`check` are absent.
- **Tests/bench/example totals.** 11 reconciliation integration tests +
  2 inline + 10 re-pinned/new CLI pr tests + 3 CLI cache tests; benches
  `release_state` + `roundtrip` registered and run; 2 new examples
  (`pending_releases`, `cached_walk`). Full workspace sweep green
  (67 test binaries), `cargo bench` quick runs recorded for both new
  benches.
- **Honest state at cut-off.** M4.1b (GitHub REST adapter), the live
  `pr`/`release` shell effects (git-side tag/push), the
  `github-release-notes` style, the dhat re-audit against the §14.1
  budgets and the corpus-wide pnpm differential remain open. The tag
  phase's git-side effects (annotated tag creation) remain the live
  slice's work — the forecast reports what a live run would owe, it
  does not create tags. Tree left dirty (no commits made).
- **Next session start point.** (1) M4.1b REST adapter (VERIFY-tagged
  research first, ADR per §0.3), (2) the live `release` shell over the
  git-side tag/push effects consuming `pending_releases`, (3) the
  corpus-wide pnpm differential as an xtask subcommand, (4) the M2.6
  perf-budget close-out (dhat re-audit against §14.1 with the cache
  warm).

### 2026-11-13 — M4.2b step 1 (`pr --dry` forecast) + M4.3 `check` title lint (branch `main`, tree left dirty, no commits)

- **Inventory at session start.** Main was green at `1d11bc8` (the
  M4.2a/M4.3a tree had landed as conventional per-file commits plus the
  docs-fix tail). Baseline `cargo test --workspace --all-features`
  re-run green on a fresh toolchain (stable 1.100.0) before any delta.
  Docs drift found: the CLI crate description still named only the
  M1.10 command set (`apply`/`check`/`pr` missing) — fixed as part of
  this slice.
- **M4.2b step 1 implemented (no stubs), TDD with one full red→green
  cycle** (9 `tests/preview.rs` contract tests written first, run red
  against the missing adapter, then implemented):
  - `halyard-forge/src/preview.rs` — the dry-run `Forge` adapter behind
    `pr --dry`: `find_release_pr` → always `None` (offline mode cannot
    observe the remote, I5), `upsert_pr` → a `preview://`-marked PR with
    number `0` (a real forge can produce neither shape), `create_release`
    → `created: true` + nothing stored, `close_pr` → the §11.4
    not-found-is-a-value `Ok(false)`, `commit_via_api` → the documented
    seam-default decline. Every call — mutations included — lands in the
    §11.5 call log, so the dry run's budget forecast is the same
    observable the live run will report.
  - CLI `pr --dry`: plan → `render_release_body` (§11.3) →
    `run_release_machine` over `PreviewForge` with `pending_releases: []`
    (the tag-phase manifest-vs-tags reconciliation is the live run's
    shell work — the report states this explicitly). Effects, the §11.5
    call forecast and the body render through the human/json/gha paths;
    the JSON document carries `mode/title/branch/pending_plan/effects/
    calls/body`. Determinism pinned byte-identical across runs (I3).
    Live mode declines fail-closed with the new `E-CLI-PR` code + docs
    page instead of pretending to write.
  - `pr_preview` bench (3 machine pass shapes over the adapter) and the
    `pr_preview` example (open forecast / close probe / tag-phase
    PROPAGATE, with the call log printed).
- **M4.3 `check` implemented in the same session (M1 green end-to-end →
  M2 authorized), TDD red→green (10 engine tests first):**
  `halyard-conventional/src/title_lint.rs` — the §11.6 title-lint seam.
  The parser leaf hosts the grammar + verdict types; the type→bump
  policy is injected via the `TitleBumpRule` trait, so the leaf stays
  standalone (ADR-0002 layer rules hold) while the shell binds
  `PlanConfig::commit_types`. The engine mirrors `planner.rs` exactly:
  the kind reaches the rule as written (no case normalization), breaking
  titles are major per §10.1's fixed rule, only the subject line is
  judged (a trailing `BREAKING CHANGE` line cannot leak), and the
  did-you-mean hint (edit-distance ≤ 2, no deps) fires only for kinds
  outside the rule's known set — a known-but-unmapped kind is policy,
  not a typo. `halyard check --pr-title` predicts, per pending package,
  the conveyed bump after the §10.2 pre-major remap
  (`p1 0.1.0 → patch (0.x policy: minor → patch)`); a typo'd type fails
  with `E-CLI-TITLE` (new docs page); `check --exit-code` gates CI with
  §15.4 exit code 3 when a release is pending. `run()` now returns the
  `ExitCode` so commands can carry non-failure codes.
- **Micro-debt cleaned.** The CLI `--no-cache` help text now states the
  truth precisely (the M2.5 cache seam exists; the CLI pipeline does not
  wire it until M2.6 — accepted as a forward-compat no-op); the
  native-lockfile-refresh member list no longer falls back to an empty
  string on an unresolvable symbol — it fails closed with the
  `E-LOCK-NATIVE` code (an empty members list would pass any refresh).
- **Tests/bench/example totals.** 9 preview contract tests + 12
  title-lint engine tests (10 integration + 2 inline) + 2 inline CLI
  check tests + 9 CLI check integration tests + 6 CLI pr integration
  tests; benches `title_lint` + `pr_preview` registered and run; 2 new
  examples (`lint_title`, `pr_preview`); `check-layers` unaffected
  (the CLI may depend on any workspace crate; forge/changelog gained no
  new runtime deps).
- **Honest state at cut-off.** M4.1b (GitHub REST adapter), the live
  `pr`/`release` shell effects, the tag-phase reconciliation in
  `pr --dry`, `github-release-notes` and M2.6 close-out remain open.
  Tree left dirty on `main` (no commits made).
- **Next session start point.** (1) M4.1b REST adapter (VERIFY-tagged
  research first, ADR per §0.3), (2) the live `pr`/`release` shell over
  git effects, (3) the corpus-wide pnpm differential as an xtask
  subcommand, (4) M2.6 perf-budget close-out (wire the cache into the
  CLI pipeline and retire the `--no-cache` no-op).

### 2026-11-13 — M4.2a Release-PR state machine + M4.3a PR body renderer (branch `main`, tree left dirty, no commits)

- **Inventory at session start.** The forge seam (M4.1a, ADR-0011) was
  green on main with 14 commits landed after the last PROGRESS row
  (docs drift: README status said "M3 apply slice"; the M4.1a row was
  missing — both repaired this session). Baseline `cargo test
  --workspace --all-features` re-run green on a fresh toolchain
  (stable 1.100.0); full workspace clippy 1.100 clean apart from the
  forge benches' `format_push_string`/`write_with_newline` debt and a
  `doc_markdown` in the release_lifecycle example (all fixed as
  micro-debt; the M4.2a/M4.3a work below is otherwise the only delta).
- **M4.2a implemented (no stubs), TDD with one full red→green cycle**
  (12 machine tests + 4 `close_pr` contract pins written first, run red
  against missing API/fail-closed bodies, then implemented):
  - `halyard-forge/src/state.rs` — the §11.1 machine:
    `run_release_machine(&dyn Forge, &MachineInput) -> MachineReport`.
    Tag phase first (one `create_release` per pending release — the
    adapter's GET-by-tag-first idempotence distinguishes
    `ReleaseCreated`/`ReleaseExisted`), PROPAGATE gating (pending ⇒
    prepare skipped, §11.1 step 2), prepare phase (find →
    `PrOpened`/`PrUpdated`/`PrUnchanged` — an identical title+body
    spends the one find probe and nothing else — or `PrClosed` under an
    empty plan). `MachineReport.calls` counts this pass's forge calls
    from the adapter's own log (§11.5: the budget is observable), input
    validation fails closed before any call, and every failure aborts
    the pass with the adapter's error verbatim.
  - `Forge::close_pr` seam (§11.1 step 3): default honest decline
    (`E-FORGE-UNSUPPORTED`, mirroring `commit_via_api`/§7.3);
    not-found is a value (`Ok(false)`), the re-close is a no-op (I1).
  - `MemoryForge` now models open/closed PR state: `find_release_pr`
    returns open PRs only, a closed PR is never rewritten in place
    (upsert-after-close opens a fresh one — real forges cannot reuse
    the number), `close_pr` closes exactly once. 4 contract pins.
  - §15.2 failure-injection matrix: auth/rate-limit/conflict injected
    at the tag phase and the prepare phase, plus the close path — each
    re-run reaches the uninterrupted baseline state with zero duplicate
    PRs/releases (I2).
  - Bench `state_machine` (5 pass shapes, criterion) and example
    `release_machine` (six passes: propagate → open → no-op → update →
    close → re-close, with the call log printed).
  - `halyard-test-support` re-exports the machine types (the E2E lane
    imports fixtures + machine from one crate).
- **M4.3a implemented in the same session (M1 green end-to-end → M2
  authorized), TDD red→green (10 tests first):**
  `halyard-changelog/src/pr_body.rs` — `render_release_body(&Plan)`
  renders the §11.3 skeleton (heading, `Package|Current|Next|Bump|Why`
  table with a deterministic Why map over the reason chain, §16.3
  Skipped line, verbatim changelog preview, override footer) and closes
  with the embedded plan: canonical JSON (ADR-0005) with the
  recomputed `plan_hash` filled in, DEFLATE-compressed, base64-encoded
  (`flate2` + `base64`, the only new deps, layer rule widened
  consciously). `extract_embedded_plan` is the I8 inverse: decode →
  decompress → re-parse → re-hash, rejecting missing markers, bad
  base64/deflate/JSON, self-inconsistent documents and marker/plan hash
  mismatches fail-closed (`E-CHLOG-PRBODY`, new error variant + docs
  page + README index row). Renderer determinism (I3) is what turns the
  machine's identical-body pass into a zero-mutating-call no-op — the
  `release_machine` example composes the two.
  Golden snapshot `pr_body__pr_body_matches_the_golden_snapshot.snap`
  reviewed and accepted.
- **Tests/bench/example totals.** 16 new forge tests (12 machine + 4
  contract), 10 new pr_body tests (9 + 1 unit), 5 bench groups, 2 new
  examples; `check-layers` ✓ (15 crates, changelog rule widened),
  `schema-check` ✓.
- **Honest state at cut-off.** M4.1b (GitHub REST adapter), M4.2b (the
  shell's git-side tag/push effects + CLI `pr`/`release` commands),
  the corpus-wide pnpm differential and M2.6 perf close-out remain
  open. The nightly `forge-smoke` job stays latent until M4.1b (its
  `forge-smoke` CLI subcommand does not exist yet — the job is opt-in
  gated, not broken). Tree left dirty on `main` (no commits made).
- **Next session start point.** (1) M4.2b: wire `run_release_machine`
  into a CLI `pr --dry` / `release` command over the git effects,
  (2) M4.1b REST adapter (VERIFY-tagged research first, ADR per §0.3),
  (3) the corpus-wide pnpm differential as an xtask subcommand,
  (4) M2.6 perf-budget close-out.

### 2026-11-13 — M3.4 pnpm-lock slice: importer patcher + native fallback (branch `feat/pnpm-lock-patcher`, base 8189fb1)

- **Inventory at session start.** The M3.2 slice was green on main.
  Docs drift found and fixed: README's daily clippy command still carried
  `-W clippy::pedantic` although pedantic+restriction enforcement lives
  in `[workspace.lints]` (a CLI `-W` overrides the curated
  `must_use_candidate`/`missing_errors_doc` allows — the root Cargo.toml
  comment documents the contract, CI matches it); README's status header
  still said "milestone M1 in progress". Micro-debt cleaned: the CLI
  `--package` plan filter hardcoded a `crates/<pkg>/Cargo.toml` layout
  and dropped npm members living elsewhere — it now resolves the
  package's manifest/root from the workspace.
- **M3.4 implemented (no stubs), TDD with one full red→green cycle** (13
  unit tests written first, run red against decline-only bodies, then
  implemented):
  - `halyard-eco-npm/src/lockfile.rs` — the restricted importer-grammar
    scanner (line/indent based, quote-aware comment stripping, absolute
    byte spans incl. inner spans for partial rewrites) and the patcher:
    §9.3 rules — plain-semver specifiers advance through the canonical
    `rewrite_requirement` (ADR-0008, so lockfile and manifest converge on
    the same text), workspace:/catalog:/file:/link: protocol specifiers
    skip (the plan does not rewrite protocol rows, M3.3 skip-and-count),
    `link:…` refs skip, `<version>(link:…)` refs move their version
    token, everything else declines (`packages:`/`snapshots:` resolution
    keys for a member, bare registry version rows, foreign-grammar or
    unrewritable specifiers, `lockfileVersion` < 6 or missing,
    multi-document lockfiles). Read-back verification re-analyzes the
    patched text as a pure skip pass. Unlike Cargo.lock, a member with
    no importer rows is a legitimate no-op (pnpm records no mandatory
    member rows) — documented difference from §9.2 rule 1.
  - `verify_native_refresh` (pnpm): before/after importer-row snapshots;
    only member importer rows may move — resolution-key, header or
    non-member row changes fail closed (`E-LOCK-NATIVE`).
  - `NpmAdapter` seam: `patch_lockfile` routes `pnpm-lock.yaml` to the
    patcher (other npm-family lockfiles decline as native-only in v1);
    `refresh_lockfile_native` dispatches on the detected lockfile kind
    via the pure `refresh_command` table (argv arrays only, I7): `pnpm
    install --lockfile-only --offline` (§9.3), `npm install
    --package-lock-only`, `yarn install --mode update-lockfile`, `bun
    install --lockfile-only` (the latter three VERIFIED against
    docs.npmjs.com / bun.com/docs/pm/lockfile / yarnpkg CLI docs).
  - Core: `LockfileVersionChange::from_release_for_ecosystem` (the
    cargo-only `from_release` delegates); the seam docs updated to the
    two-adapter reality.
  - `halyard-edit`: the lockfile stage generalized to
    `LOCKFILE_TARGETS` (Cargo.lock/cargo, pnpm-lock.yaml/npm);
    `ApplyReport.lockfiles` is a `Vec<LockfileSummary>` with an
    `ecosystem` field — all-or-none staging preserved.
  - CLI: render/JSON rows iterate the lockfile stages; the native
    fallback runs per declined row with the right adapter and the right
    verifier (eco-cargo / eco-npm), members filtered by the row's
    ecosystem.
- **Tests/bench/example/differential.** 15 unit tests (patcher matrix:
  rewrite, byte-preservation, protocol no-op, idempotence, resolution-key
  decline, registry-row decline, unrewritable/foreign specifier declines,
  drift declines, header/multi-doc declines, lonely-member no-op, empty
  changes, CRLF + quoting) + 6 adapter tests (routing, native-only
  declines, non-UTF-8, argv table, kind detection, spawn-free no-lockfile
  failure) + 3 apply_plan integration tests on a pnpm workspace fixture
  (patch + manifest rewrite, idempotent re-apply, resolution-key decline
  leaves bytes untouched) + 2 differential tests vs a real pnpm (run
  where pnpm exists; skip honestly otherwise — the sandbox has no pnpm).
  Criterion bench `lockfile_patch` (40 importers, 10 member moves ≈
  206 µs p50; `verify_native_refresh` ≈ 201 µs). Consumer example
  `examples/patch_pnpm_lockfile.rs` (2 applied, 2 skipped on the
  fixture). Error pages E-LOCK-PARSE/E-LOCK-NATIVE and the errors README
  index extended to the npm side.
- **Honest state at cut-off.** M3.4's full §17 acceptance ("pnpm
  differential on corpus") remains an xtask/nightly follow-up; npm/yarn/
  bun lockfiles are native-only in v1 by design (§9.3 Year-2 note). The
  plan-perf budgets are untouched by this slice. Tree left dirty on
  `feat/pnpm-lock-patcher` (no commits made).
- **M3.5b implemented in the same session (M1 green end-to-end → M2
  authorized), TDD red→green (tests first against the string-check
  implementation):** the `Style` registry (`parse`/`name`) replaces the
  `style != "keep-a-changelog"` check in both entry points;
  `conventional` maps the planner's section titles through the
  documented type map (breaking group first); `compact` flattens to one
  bullet list (breaking entries first). Both styles reuse the
  style-independent document machinery (marker/Unreleased insertion,
  release-section replacement), so idempotence carries over — proven by
  byte-identical re-run tests. `github-release-notes` stays unshipped
  and the E-CHLOG-STYLE message + docs page now name the shipped set.
  5 new tests (parse round-trip, compact golden, conventional golden +
  title mapping, per-style idempotence, unknown-style message);
  snapshots reviewed and accepted.
- **Next session start point.** (1) M4.1 forge adapter, (2) the
  corpus-wide pnpm differential as an xtask subcommand, (3) M2.6
  perf-budget close-out, (4) the minijinja user-template path.

### 2026-11-12 — M2 slice D: tree diff + persistent cache (branch `feat/m2-tree-diff-cache-perf`, base 9f32036f7215bb0fb590f120442648feca5a6104)

- **Inventory at session start.** Slice C state per PROGRESS.md; fresh
  sandbox (toolchain installed: stable 1.100.0 + nightly 1.102.0 with
  Miri). Preflight found **check-layers failing on main** (no rules for
  `halyard-git`/`halyard-changelog`) and **`examples/walk_history.rs`
  failing to compile** (missing `HistoryProvider` import) — both are
  Slice C close-out items fixed in this slice.
- **Implemented this session (all compiled, no stubs/`todo!()`).**
  - `halyard-core::trie` — §8.4 path trie (`build_trie`, deepest-root
    attribution data, shared global paths, `layout_hash` over roots +
    include filters), determinism tests.
  - `halyard-git::bloom` — changed-path Bloom filters read directly from
    the commit-graph's `BIDX`/`BDAT` chunks (gix 0.90 does not expose
    them); murmur3 x86_32 v2 with git's seeds (verified against git's
    `bloom.c` source: write-time insertion of every leading directory
    makes prefix negatives sound).
  - `halyard-git::tree_diff` — trie-guided lockstep walk with pruning
    outside package roots, deterministic `Vec`-backed memo LRU keyed on
    `(tree_oid_c, tree_oid_p, trie_node_id)`, include/exclude evaluation
    on collected paths, and `TreeDiffStats` counters (trees/entries
    read, memo hits, bloom skips).
  - `halyard-git::merge` — `MergePolicy`, in-window revert cancellation
    (`This reverts commit <oid>`), out-of-window/absent-target reverts
    downgraded to `fix`, merge-commit gating; unit tests for every §8.5
    rule.
  - `halyard-cache` — the §8.7 persistent cache: versioned header,
    checksummed records, append-only writes with per-operation advisory
    locking (a whole-lifetime lock self-deadlocked same-process
    multi-instance use — found by the test suite, fixed to per-op
    flock), mmap read path confined to `src/mmap.rs` (the workspace's
    single `unsafe` surface, rationale + cfg(miri) fallback), failpoint
    **and** real-process SIGKILL kill-during-write tests, corruption
    skip, Miri + loom suites, `dump` example. 15/15 tests green.
  - `halyard-conventional` — `revert_target` + `trailing_pr_number`
    (byte-scan, no regex) with golden + totality tests.
  - `GitHistory::build_with_options` — the new pipeline: paint → parse →
    merge gate → cache consult → tree diff → attribution ∩ painted
    window; commits touching no package path leave the digest (docs-only
    commits in multi-package repos).
- **Honest state at cut-off.** `halyard-git` lib tests: 8/11 green; 3
  bloom unit tests fail and one glob-path unit test hangs — triage
  needed (first suspects: the synthetic chunk-table construction in the
  tests and a possible `**` backtracking interaction to be reproduced);
  integration tests (engine/randomized/dhat) not yet re-run against the
  new attribution semantics. M2.6 (perf pass), the D0 remainder (xtask
  `schema-check-plan`/`git-diff`, fuzz targets, error-doc pages, CLI
  README/tests, changelog goldens/example, CI steps) and the Step-5 gate
  battery were **not started** — the session ended early on the git
  engine build-out. M2 is **partial**, deferred task IDs M2.6, M2.7
  (corpus not fetched) plus the close-out list above.
- **Next session start point.** (1) Fix the 4 failing/hanging unit
  tests and re-run `cargo test -p halyard-git` + the engine differential
  (update `ws_with` package roots to `""` so the trie attributes
  correctly), (2) D0 remainder, (3) M2.6 perf pass, (4) gate battery,
  then M3.1–M3.6 per protocol.
### 2026-11-13 — M3 apply slice + full gate battery (branch `main`, base 7838548)

- **Inventory at session start.** The previous session ended mid-verification:
  xtask did not compile (three helpers mis-filed under `#[cfg(test)]`),
  clippy had residual errors in the discover benches/tests, and the gate
  battery had not been run. This session drove everything to green, then
  implemented the M3 apply slice.
- **Compliance repair.** Relocated the stray `#[cfg(test)]` attributes;
  converted the discover benches/tests to the house lint style
  (`unreachable!("...")` instead of `expect`/`panic!`, `.get()`/`.first()`
  instead of indexing, `write!` instead of `push_str(&format!…)`,
  `#[cfg(test)]`-free helper fns). `cargo clippy --workspace
  --all-targets --all-features` = **0 errors**; `#![allow]/#[expect]`
  grep = **0**.
- **Gate battery (all re-run to green):**
  - `check-layers` ✓ (14 crates), `schema-check` ✓, `schema-check-plan` ✓.
  - `git-diff` ✓ — 20 synthetic commits across 3 packages, attribution
    matches `git diff-tree` ground truth.
  - **M2.7 replay** ✓ — required three real fixes, all honest-engine
    decisions, no gate loosening: (1) a tag the planner declines wholesale
    (fail-closed on a quoted exact pin, serde `v1.0.220~1`) is recorded as
    `plan-declined` and excluded from the agreement count like initial
    releases (policy documented in the generated `docs/replay.md`); (2)
    discovery no longer aborts on an anonymous root `package.json`
    (vuejs/core has one at every revision — it hosts the workspace but is
    not a member; a nameless *member* still fails closed with
    `E-DISC-MANIFEST`); (3) unknown top-level `pnpm-workspace.yaml` keys
    (pnpm v10 `allowBuilds`, `onlyBuiltDependencies`) are skipped with
    their indented block instead of erroring — they carry no member/catalog
    semantics. Result: **6/6 = 100 % agreement on conventional repos**
    (gate ≥ 95 %), byte-identical `docs/replay.md` across two runs. `fd`
    and `bat` were reclassified `conventional = false` in
    `corpus/replay.toml` on measured triage evidence (their release windows
    are dominated by non-conventional subjects, e.g. 461 of 474 at
    bat `v0.25.0`) — they are reported for triage only, and every one of
    their disagreements is the `non-conventional` category.
  - **M2.6 bench** ✓ — found and fixed a real harness bug: `bench-public`
    passed the HEAD OID returned by `genrepo::build` as the repository
    *path* to the pipeline. Release-profile measurements recorded in
    `bench/measurements.toml` (no-op-warm 2.45 ms, no-op-cold 2.45 ms,
    plan-200x40 38.00/44.94 ms, plan-1000x50k 1021.82/1044.13 ms,
    apply-40 6.07/30.33 ms, peak RSS 19 MB ≤ 300 MB). `perf-gate`
    honestly reports two misses (plan-200x40 p50 38 ms vs 25 ms,
    plan-1000x50k p50 1022 ms vs 400 ms — sandbox CPU ceiling; the
    1000-package row is the documented 2000-commit scaled variant).
    Budgets unchanged. dhat re-audit: 12 alloc/commit no-graph,
    5 with commit-graph — within the asserted bounds (≤ 14 / ≤ 8).
  - `discover-diff --corpus cargo` ✓ **4/4** (fd, ripgrep, bat, serde
    agree with `cargo metadata`): the ground truth now runs in a temp
    copy outside the halyard tree, because `cargo metadata` inside the
    corpus otherwise joins halyard's own `[workspace]`. npm differential
    skips honestly (pnpm absent from the sandbox).
  - `halyard-cli` builds; bench `--dry` works.
- **M3 implemented (apply slice, no stubs):**
  - M3.1 `halyard-eco-cargo/src/edit.rs` — byte-level patcher (toml_edit
    0.25 re-serialization drops CRLF, so the patcher replaces only the
    matched quoted literals and re-quotes with the original quote char;
    read-back re-parse verified). Dep-kind sections incl.
    `[workspace.dependencies]` and `[target.*]`; own `[package]` version
    and inherited `[workspace.package]` version; skip-and-count for `*`,
    protocol requirements, and workspace-inherited rows;
    `E-EDIT-UNREWRITABLE`/`E-EDIT-PATCHES` fail-closed (nothing mutated
    on any failure). 10 unit tests incl. formatting-preservation and
    idempotence.
  - M3.3 `halyard-eco-npm/src/edit.rs` — span-preserving JSON patch via
    new `JsonDoc::replace_span`; `catalog:`/`workspace:`/`link:`/`file:`
    classification reuses the M1.6 `classify_req`. 5 unit tests.
  - `EcosystemAdapter::edit_versions` seam added in `halyard-core`
    (object-safe; layer rules hold — core does not depend on eco crates)
    with impls on both adapters; `halyard-discover::registered_adapters()`
    exposes them as `&dyn`.
  - M3.6 `halyard-edit/src/plan_apply.rs` — `edit_set_from_plan` (1:1
    over the planner's `PlanEdit`/`PlanPatch` wire rows),
    `dry_run_preview`, `apply_plan` (fully staged in memory, then the
    existing `atomic_write`; deterministic sorted per-file summary);
    CLI `apply [--dry]` subcommand reusing the `plan` pipeline.
    Integration tests: `--dry` writes nothing; real apply rewrites the
    member manifests; a stale-span plan fails closed and writes nothing.
  - **End-to-end smoke (recorded verbatim):** `xtask gen-repo --packages 3
    --commits 5 --seed 3` → `halyard -C /tmp/m3smoke apply --dry` prints
    `dry run: 3 files, 3 patches` with quote-inclusive spans
    (`[34..41) -> "0.1.1"`); real `apply` on a copy prints
    `applied 3 edits across 3 files (0 skipped)`; `git diff --stat` shows
    exactly one changed line per member manifest
    (`-version = "0.1.0"` → `+version = "0.1.1"`); re-planning after apply
    proposes 0.1.2 because the planner anchors on the manifest version
    (`planner.rs`: `from = pkg.version`) — manifest-anchored planning is
    the documented M2 semantic, not a double-bump bug.
- **Honest state at cut-off.** Full suite **321 passed / 0 failed**
  (21 new tests this slice). Known gaps: M3.2/M3.4 lockfile patchers not
  started; the plan-200x40 and plan-1000x50k perf budgets are missed
  (recorded, not gated away); bench measurement files vary run-to-run by
  design (wall-clock samples) — only `docs/replay.md` is byte-gated;
  `cargo test --release` remains unsupported by design (§14.2
  abort-on-panic). Tree left dirty on `main` (no commits made).
- **Next session start point.** (1) M3.2 Cargo.lock patcher + the 200-
  fixture `cargo update -w --offline` differential, (2) M3.4 pnpm-lock
  span patcher + npm/yarn/bun native fallback, (3) M3.5 changelog styles
  beyond keep-a-changelog if required, (4) then M4.1 forge adapter.

### 2026-11-13 — M3.2 lockfile slice: Cargo.lock patcher + native fallback (branch `main`, no commits; tree left dirty)

- **Inventory at session start.** The M3 apply slice was green (321
  tests, clippy 0 on the 1.99 toolchain). Baseline re-run on the fresh
  sandbox's clippy 1.100 found **three gate failures the older clippy
  did not flag**: the deprecated-without-`serde` `minijinja
  Environment::new()` in `halyard-changelog`, and `clippy::panic` in
  `halyard-version`'s `tests/golden.rs`/`tests/props.rs` (the removed
  `clippy.toml` used to carry `allow-*-in-tests`; the root Cargo.toml
  comment still claimed otherwise). All fixed in this slice as
  micro-debt (`Environment::empty()` + rationale; test panics →
  `unreachable!`; comment corrected).
- **M3.2 implemented (no stubs):**
  - `halyard-core/src/lockfile.rs` — the lockfile seam:
    `LockfileVersionChange` (derived from plan releases via
    `from_release`: cargo ecosystem, real moves only), `LockfileOutcome`
    (`Patched`/`NeedsNative`), `LockfileFailure` (`E-LOCK-PARSE`) and
    `NativeRefreshError` (`E-LOCK-NATIVE`). `EcosystemAdapter` gained
    `patch_lockfile` (default = the documented `NeedsNative` fallback,
    §9.2 rule 4 — an honest decline, not a stub) and
    `refresh_lockfile_native` (default = `Unsupported`).
  - `halyard-eco-cargo/src/lockfile.rs` — the patcher: `toml_edit`
    spans over the lockfile, member rows (no `source`) and
    disambiguated `"name version [source]"` dependency entries rewritten
    byte-in place (I4), read-back re-parse verification, idempotent.
    Anything unrecognized (format header ≠ 3|4, sourced row for a
    changed name, duplicate member row, missing member row, drift) →
    `NeedsNative`; non-UTF-8/non-TOML/unusable change → `E-LOCK-PARSE`.
  - `halyard-edit/src/plan_apply.rs` — the lockfile stage in `apply_plan`:
    runs before any write (all-or-none holds), `ApplyReport.lockfile`
    carries the `LockfileSummary` (a decline is a report row with
    `native_reason`, never a guess); `EditError::LockfileParse` maps the
    hard failure. CLI render + JSON rows added.
  - Native fallback (§9.2 rule 4): `CargoAdapter::refresh_lockfile_native`
    spawns `cargo update --workspace --offline` (argv array only, I7);
    `halyard_eco_cargo::lockfile::verify_native_refresh` (pure)
    post-verifies that only workspace members moved — a registry row
    moving, a row appearing/disappearing, or a changed format header
    fails closed; the CLI shell orchestrates read-before/after +
    verification.
- **Differential (the §9.2 acceptance arbiter):**
  `halyard-eco-cargo/tests/differential.rs` builds a real cargo
  workspace (path-only deps, hermetic `CARGO_HOME`, offline) and proves
  our patch is **byte-identical** to cargo's own
  `cargo update -w --offline` rewrite, plus that the native refresh +
  verification flow moves exactly the bumped member. Skips honestly when
  no cargo binary is available. Scaling to the full 200-mutated-fixture
  matrix is an xtask/nightly follow-up.
- **Tests/bench/example.** 13 unit tests (patcher) + 2 adapter tests + 4
  `apply_plan` integration tests + 2 core seam tests + 3 differential
  tests; criterion bench `lockfile_patch` (120-package lock, 40 moves,
  ≈223 µs p50 in the bench profile); consumer example
  `examples/patch_lockfile.rs`. Error pages `E-LOCK-PARSE`/
  `E-LOCK-NATIVE` written; the missing `E-PLAN-CYCLE`/`E-PLAN-RELEASE-AS`/
  `E-PLAN-UNREWRITABLE` pages (slice-C debt) landed; the errors README
  index table was repaired (a stray blank line split it) and extended;
  the §7.3 trait sketch was updated to the as-built signatures.
- **Honest state at cut-off.** M3.4 (pnpm-lock span patcher) and the
  200-fixture differential scale-up remain open. The plan-perf budgets
  are untouched by this slice (the lockfile pass is not on the planning
  path). Tree left dirty on `main` (no commits made).
- **Next session start point.** (1) M3.4 pnpm-lock span patcher +
  npm/yarn/bun native fallback, (2) the 200-fixture lockfile
  differential as an xtask subcommand, (3) M3.5 changelog styles if
  required, (4) then M4.1 forge adapter.

### 2026-11-14 — Live release shell (git side) + release-notes pipeline (branch `feat/live-release-tag-phase`, tree left dirty, no commits)

- **Inventory at session start.** Main was green at `7cab7c9`
  (the tag-target pinpoint slice tree). Baseline gates re-run green on
  a fresh toolchain (stable 1.100.0): `test --workspace --all-features`
  (535 tests), fmt, clippy `-D warnings`. The declared start points were
  (1) the live release shell (`release` execution mode over
  `GixTagWriter` + the §11.2 tag push), (2) M4.1b, (3) the corpus pnpm
  differential, (4) the M2.6 dhat re-audit. Picked (1) as M1 — the
  whole §11.1 chain has pointed at it for three sessions — and the
  release-notes pipeline (twice deferred as "needs forge context") as
  M2: it completes the release-content chain the shell consumes.
- **M1 implemented (no stubs), one full red→green cycle** (the 11
  integration tests written first, red on the unresolved
  `halyard_git::push` import, then implemented):
  - `halyard-git/src/push.rs` — the `TagPusher` seam (`&dyn`): the
    probe-first classification (`git ls-remote --tags`, one round trip)
    splits each tag into already-on-remote (I1 no-op), divergent remote
    tag (`E-GIT-TAG-CONFLICT`, never forced) and to-push; the batch
    pushes once (`git push --porcelain <refspecs…>`) and attributes
    per-ref outcomes from the porcelain rows (`=` up-to-date,
    `!` rejected, `*` new). Subprocess discipline (I6/I7): argv arrays
    only, `GIT_TERMINAL_PROMPT=0` (fail closed, never prompt), no
    `--force`/`--delete`, cwd rooted at the opened repository (the
    ambient process cwd is not inherited — found by the first red run,
    where the subprocess resolved the *halyard* repo's own `origin`).
    `PreviewTagPusher` forecasts `WouldPush` with zero subprocesses (I5).
    The empirical wire formats are pinned by tests and benches (ADR-0012).
  - CLI `release` live mode: reconcile → pinpoint → real annotated tags
    (`GixTagWriter`; the shell owns the tagger identity — `user.name`/
    `user.email` via in-process gix config + the wall clock; incomplete
    identity fails closed per directive) → §11.2 push over the seam
    (`--no-push` skips; a missing `remote.origin.url` is an honest
    skip); forge releases reported as deferred (M4.1b), never faked;
    failures render as rows and exit 1 (I2-resumable). `--dry` gains
    the push forecast and stays fully offline. 6 new CLI tests + the
    decline test replaced (live mode no longer declines).
  - `E-GIT-PUSH` page + errors index row; `E-CLI-RELEASE` page rewritten
    to the live semantics; ADR-0012 (the effect-step seam decision);
    crate doc amended as-built (the planning read path keeps its
    no-subprocess/no-network property; the §11.1 effect steps are the
    sanctioned exceptions); CLI README + root README rows.
  - Bench `crates/halyard-git/benches/tag_push.rs` (refspecs-64,
    remote-targets-64, porcelain-64); example `examples/tag_push.rs`
    (fixture bare remote → tag → push → I1 re-run, verified).
- **M2 implemented (M1 green end-to-end first), red→green** (the 7
  changelog tests written first, red on the missing API):
  - `halyard-changelog/src/release_notes.rs` — `Style::GithubReleaseNotes`
    registered (the registry render is context-free; the two inline
    unknown-style tests re-pinned to a genuinely unknown name),
    `compare_url` (the four GitHub remote shapes → compare URL; foreign
    forges/initial releases render none — no guessed shapes),
    `render_release_notes` (plan entry preview → forge body, breaking
    first, no heading), `extract_version_section` (the bracketed/dated/
    v-prefixed heading shapes the renderer itself writes) +
    `render_release_notes_for_version` (the tag phase's `create_release`
    payload: the package changelog's own section for the released
    version; a missing section renders empty — no fabrication).
  - CLI `pr --dry`: `ReleaseSpec.notes` filled from the changelog
    section (plan-tag fallback, else empty); the JSON carries
    per-release `notes`. The reachable-state semantics were verified
    against the binary before pinning (the pending tag phase and the
    plan releases describe *different* versions by design — the
    first naive package-name matching produced the empty-notes
    finding that drove the changelog-section design).
  - Bench `crates/halyard-changelog/benches/release_notes.rs`
    (extraction over 32 sections + entry-preview render); example
    `examples/render_release_notes.rs` (both entry points + the honest
    empty case). `E-CHLOG-STYLE` page updated to the shipped set; arch
    doc §9.4 as-built note.
- **Micro-debt/docs drift fixed.** The crate-doc "no subprocess" claim
  scoped precisely (M1); the CLI README "apply is the only filesystem-
  writing command" claim corrected (`release` writes git refs); the
  stale `release --dry` rows in both READMEs; the E-CHLOG-STYLE
  unshipped-style claim.
- **Honest state at cut-off.** M4.1b (GitHub REST adapter) is the next
  milestone — the live shell's forge half stays deferred (reported in
  every live release report), the nightly `forge-smoke` job stays
  latent. Publish (M5), the minijinja user-template path, the M2.6
  dhat re-audit and the corpus-wide pnpm differential are unchanged.
  The two p50 perf-budget misses remain as recorded (sandbox CPU
  ceiling, `bench/measurements.toml`) — the CI perf-gate job is
  expected red until the M2.6 close-out.
- **Next session start point.** (1) M4.1b REST adapter (VERIFY
  research + ADR per §0.3), (2) the corpus-wide pnpm differential as
  an xtask subcommand, (3) the M2.6 dhat re-audit with the cache warm,
  (4) the minijinja user-template path.


### 2026-11-14 — GitHub REST adapter (M4.1b) + user-template path (branch `feat/github-rest-forge-adapter`, tree left dirty, no commits)

- **Inventory at session start.** Main green at `5ff350b` (the
  live-release-shell tree): 564 tests, fmt/clippy/`--all-features` all
  green on stable 1.100.0. The declared start points were (1) M4.1b
  (VERIFY research + ADR per §0.3), (2) the corpus-wide pnpm
  differential, (3) the M2.6 dhat re-audit, (4) the minijinja
  user-template path. Picked (1) as M1 and (4) as M2 — (2)/(3) need
  external binaries/hardware the session environment does not have
  (pnpm, a warm dhat profile) and stay open.
- **M1 (M4.1b) implemented, one red→green cycle** (tests written first,
  red on the unresolved adapter API): the transport seam in
  `halyard-forge/src/transport.rs` (plain-data `HttpRequest`/`HttpResponse`,
  the `HttpTransport` effect seam, the scripted `MemoryTransport` — the
  transport analogue of `MemoryForge`), `ForgeError::Transport`
  (`E-FORGE-TRANSPORT`, page + index), and the new
  `crates/halyard-forge-github` crate: `GitHubRestForge` — a full `Forge`
  impl over REST v3 (research-verified shapes, `docs/research/github-rest-forge.md`)
  with ETag conditionals on the find probe, GET-by-tag-first release
  creation, the §11.2 base derivation for `upsert_pr`, the Git-Data
  chain for `commit_via_api` (UTF-8 content entries, base64 blobs for
  binary), rate headers snapshotted into `rate_limit()`, and the §11.5
  call log — plus `UreqTransport` (blocking ureq + rustls-ring, the §19
  default decision). Layer rule widened consciously
  (`xtask/src/layers.rs`, ADR-0013). Wire-up: live `pr` over the real
  adapter, the live `release` report's forge half (ran over the adapter
  or an honest skip), and `halyard forge-smoke` — the nightly job's
  subcommand, read-only. 45 adapter/seam tests + 3 loopback tests + CLI
  e2e over a loopback mock forge (`tests/cli_forge.rs`); bench
  `forge_calls`; example `forge_exchange`.
- **M2 (user-template path) implemented, red→green**: `StyleSource`
  (the §9.4 name-or-template-path classifier, `E-CHLOG-STYLE` for
  unknown names), `render_releases_into`/`render_releases_into_template`
  (the grouped insertion entry points), the planner's zero-config
  changelog default (`<root>/CHANGELOG.md`, §6.1 — every release now
  carries its §5.8 changelog contract), and the `apply` command's
  changelog stage (per-file §9.4 insertion, atomic writes, byte-identical
  re-render reported `unchanged`). 5 seam tests + 5 CLI tests; bench
  `stage_insert`; example `render_user_template`.
- **Micro-debt/docs.** The unwired fuzz target `json_span` registered
  in `fuzz/Cargo.toml`; the dead `releases_by_version` removed;
  E-CLI-PR rewritten to the live semantics; E-FORGE-TRANSPORT page +
  index row; E-CHLOG-STYLE classifier note; ADR-0013.
- **Honest state at cut-off.** App-JWT auth (M4.1c), the pacing
  governor (M5.5), publish (M5), the corpus pnpm differential, the M2.6
  dhat re-audit and the cache stats wiring stay open. The two p50
  perf-budget misses remain as recorded.

### 2026-11-14 — M5.5 call governor (ADR-0015) + live forge wire-up (branch `feat/forge-call-governor`, tree left dirty, no commits)

- **Inventory at session start.** Main was green at `7564b8` per the
  handoff, but the baseline re-run on a fresh toolchain (stable
  1.100.0, rustfmt/clippy components installed on demand) found two
  pre-existing reds in `halyard-cli --test cli_pr`:
  `pr_dry_is_deterministic` and `pr_dry_json_carries_release_notes` —
  both compare run 1 vs run 2 over the same fixture, and the warm §8.7
  cache's zero walk counters break byte-equality. The M4.1c session
  log had declared the fix ("the determinism pin rides `--no-cache`")
  and the repair commits (`cb1a19e`/`ebda22b`) had landed the
  cli_stats/cli_forge halves, but these two cli_pr pins never got it.
  Repaired first (both runs ride `--no-cache`, the cli_stats
  convention), cli_pr green 11/11.
- **M1 (M5.5 core) implemented, no stubs, one full red→green cycle**
  (the 12 contract tests in `crates/halyard-forge/tests/governor.rs`
  written first; the red run is on record: E0432 unresolved
  `halyard_forge::governor` + E0599 no `ForgeError::Budget`, then
  green):
  - `halyard_forge/src/governor.rs` — the `Governor` `Forge`
    decorator (ADR-0015): `GovernorPolicy` (passive `default()`, the
    §11.5 production `shell_default()`), content-call pacing
    start-to-start at `min_spacing_ms` (probes and `rate_limit()`
    unpaced), `Retry-After` honored exactly per retry, transport
    backoff a pure function of the attempt (`backoff_base_ms ×
    2^attempt`, capped — **no jitter**, the documented I3 deviation:
    halyard's minimal-call design is the thundering-herd mitigation),
    the per-run budget as a forwarded-attempts ledger (injected
    failures and retries count — §11.5 "a dropped call is an
    observable one") refusing fail-closed *before* the call and
    re-checked per attempt, non-retryable errors pass through without
    a wait, exhausted retries return the last error verbatim. Time is
    injected (`GovernorClock`/`GovernorSleeper`, the ADR-0014
    convention; `system_time()` is the sanctioned production pair) and
    the event log (`Governor::events`) is deterministic data — the
    §11.5 end-of-run usage summary rides it.
  - `ForgeError::Budget` (`E-FORGE-BUDGET`) joined the taxonomy (code
    const, ctor, Display arm, inline codes-and-display pin) with its
    docs page and the errors-README index row; the E-FORGE-RATE-LIMIT
    page gained the governor as-built note; the forge crate doc's
    status paragraph amended as-built (the seam *types* stay
    clock-free; the production pair is the sanctioned effect).
  - The machine composes unchanged: `Forge::calls` keeps delegating to
    the inner adapter, so `MachineReport.calls` and the ≤2-calls
    budget assertion never notice the wrapper (pinned by the
    transparency test).
  - Bench `crates/halyard-forge/benches/governor.rs`
    (passthrough_update_path ≈894 ns, pacing_decision_16 ≈4.6 µs,
    budget_block ≈598 ns — the decorator must be free, the refusal
    must not be slow) and example `examples/governed_release.rs`
    (throttled pass → Retry-After honored ×2 → fail-closed abort →
    I2 re-run creates the release → paced prepare upsert; scripted
    time, instant and deterministic).
- **M2 (shell wire-up) implemented after M1 was green end-to-end,
  red→green** (the 4 loopback tests appended to `cli_forge.rs` first,
  red against the stashed wire-up, then green):
  - `governor_policy_from_env` — `HALYARD_FORGE_MIN_CALL_SPACING_MS` /
    `HALYARD_FORGE_MAX_CALLS` / `HALYARD_FORGE_MAX_RETRIES` over
    `shell_default()` (the App-env env layer, §16.4); empty = unset;
    a present-but-invalid value fails closed with `E-FORGE-REQUEST`
    naming the variable and the accepted shape.
  - The three live forge sites wrapped: `pr` live (the machine runs
    over the governor; the dry run stays ungoverned — a forecast
    spends no calls), the release forge half (per-package
    `create_release` calls are content creation — paced, retryable,
    budgeted) and `forge-smoke` (the probe is budgeted like any live
    call).
  - Observability: `render::GovernorSummary` (`line()`/`json()`)
    rides the live `pr` JSON document (`governor` field — the dry
    schema stays byte-stable, the key is absent), the human/gha tails
    (`governor_line`, the `walk_line` pattern) and the smoke JSON;
    `ForgeReleases::Ran` carries the summary into the release
    report's json/human/gha. `doctor` gains the `forge-governor`
    check (the resolved policy; an invalid env fails the check
    naming the variable).
  - Tests: `governor_summary_rides_the_live_pr_report` (policy
    overrides visible, op-level spend = 2 vs the adapter's 3 HTTP
    calls — the two ledgers reconcile), `governor_budget_blocks_...`
    (E-FORGE-BUDGET, the refused call never reaches the forge, spent/
    budget in the message), `governor_env_invalid_value_...`
    (fail-closed naming the variable, E-FORGE-REQUEST),
    `forge_smoke_carries_the_governor_row` (defaults + the probe as
    the run's one spend).
- **Micro-debt/docs drift fixed (additive, not milestone
  substitutes).** The CLI README's "apply ... (the only
  filesystem-writing command)" comment now names both writing
  commands (it contradicted the corrected paragraph below it); the
  root README status header no longer lags its own body (M4.1b/M4.1c,
  the live release shell and the release-notes pipeline were absent
  from it); arch doc §16.2's as-built note claimed the live
  `pr`/`release` modes "are absent rather than stubbed" (stale since
  M4.1b/live-shell — rewritten to the wired reality incl.
  `forge-smoke`, with `explain` kept in the list) and §11.1's note
  claimed forge releases "remain the M4.1b work" (the forge half now
  runs live over the adapter when credentials allow — publish alone
  remains M5); the previous session row's "CLI tail unfinished" note
  corrected (the cli_pr half was repaired and committed post-session;
  the two warm-cache pins this session finished); CLI README rows for
  `pr`/`release`/`forge-smoke` + the new governor section; the
  ADR-0015 record.
- **Honest state at cut-off.** Publish (M5), the registry clients,
  the corpus-wide pnpm differential (no pnpm in the sandbox), the
  M2.6 dhat re-audit and the two p50 perf-budget misses are unchanged.
  The governor paces only content-creating calls on the live paths;
  the pacing default (1 000 ms) costs a single live `pr` nothing (the
  upsert is the pass's first content call) and multi-package release
  forge halves ~1 s per additional package — the env knob exists for
  CI lanes that need it off. Tree left dirty (no commits made).
- **Next session start point.** (1) Publish (M5): the registry
  clients + the topological publisher consuming the governor's
  budget, (2) the corpus-wide pnpm differential as an xtask
  subcommand, (3) the M2.6 dhat re-audit with the cache warm, (4)
  `forge-smoke`'s pacing governor report in the nightly job summary.

### 2026-11-14 — M5.2 publish engine + CLI `publish` (ADR-0018, tree left dirty, no commits)

- **Inventory at session start.** Main was green at `d465be3` (the
  fetch/registry slice tree + the ship-ceremony gitignore commit).
  Baseline gates re-run green on a fresh toolchain (stable 1.100.0):
  `test --workspace --all-features --locked` (86 suites, 658 tests,
  0 failed), fmt, clippy `-D warnings`. The declared start points were
  (1) the M5.2 publisher over the `Registry` seam, (2) the corpus-wide
  pnpm differential (needs pnpm — absent from the sandbox), (3) the
  M2.6 dhat re-audit, (4) the `github-release-notes` PR-comment mode.
  Picked (1) as M1 — the declared consumer of the M5.1 seam, the
  §7.1-reserved crate slot, the §15.4-reserved `E-PUB-*` family — and
  the §16.2 `publish` command as M2. Docs drift found at base: the
  rustdoc gate was **red** (three broken intra-doc links: `refs.rs` →
  `TagTemplate::extract_version` unimported, `halyard-registry` → a
  nonexistent `crate::governor`, `halyard-forge/src/transport.rs` →
  `MemoryForge` out of scope); the xtask member-rule test list was two
  crates stale.
- **M1 implemented (no stubs), one full red→green cycle** (the 27-test
  contract matrix written first, red on the unresolved
  `halyard_publish` API — E0432 on record — then implemented; two
  expectations corrected during the cycle where the designed gating
  precedence differs from the first draft: a gated dependent never
  reaches its own probe/effect, so the registry-probe-failure row
  gates its dependents exactly like a tool failure):
  - `crates/halyard-publish` — `plan_publication` (I8 hash gate, the
    releases × workspace join, `E-PUB-PLAN` on any contradiction incl.
    duplicate release names and targets missing from `plan.order`,
    Kahn layering name-ascending over non-dev edges of the
    target-induced subgraph, `E-PUB-ORDER` with a rendered DFS cycle
    path, dev-only cycles legal, `not_targets` with reasons) and
    `run_publication` (routing resolved before any call; the resume
    probe → effect → §13 step-3 visibility wait; §13.7 outcomes as
    rows; the failed/invisible/skipped upstream gate with sorted,
    comma-joined reasons; derived tallies; bounded within-layer
    parallelism on scoped std threads with the report name-sorted
    regardless of thread completion order). `Registry`/`RegistrySleeper`
    gained `Send + Sync` supertraits (a widening, not a change — every
    shipped impl already satisfied them; recorded in ADR-0018 §5).
  - Reference fakes ship with the seam (`MemoryRegistry` staging +
    modelled index lag, `MemoryPublishEffect` per-package failure
    scripts + `successes()`, `MapRouter`) — the `MemoryForge`/
    `MemoryTransport` convention, so the CLI tests and M5.3 script the
    same behavior without a new test-support rule.
  - 30 tests + bench `publish` (layering-40 ≈ 200 µs, engine-pass-40 ≈
    84 µs) + example `publish_plan` + ADR-0018 + `E-PUB-*` pages ×4 +
    errors-index rows + the `publish → [core, registry]` layer rule
    (check-layers 18 crates ✓) + the arch §13 as-built note.
- **M2 implemented (M1 green end-to-end first), red→green** (the CLI
  tests + argv-table tests written first, red on the missing command
  and fns, then implemented):
  - argv tables VERIFIED live per §0.4 (`docs/research/publish-tooling.md`,
    both tool docs fetched, quotes recorded): `cargo publish --dry-run
    --manifest-path <m> --offline` (the book's own hermetic example) /
    `cargo publish --manifest-path <m>` live; `pnpm publish --dry-run
    --no-git-checks` dry / `pnpm publish` live (no directory flag is
    documented — the process runs at the package root; `pnpm publish`
    is the §13.4 letter, a pnpm-less npm repo fails honestly rather
    than switching tools).
  - The engine gained `PublishMode::{Live,Dry}` + `PublishOutcome::
    Forecast` + `PublishReport.forecast`: the dry forecast validates
    routing, runs the tools' dry form and never probes (I5 — the same
    reasoning that leaves `fetch` without a `--dry`).
  - CLI `publish [--dry]`: plan → publication → engine run over the
    `SpawnPublishEffect` (argv arrays; processes rooted at the package
    root; `--manifest-path` resolved against the repo root — the CLI
    tests caught the double-relative path on the first green run, a
    real bug fixed in the effect, not the tests) and the live router
    (the two §13 clients over the blocking transport). json/human/gha
    renders with per-row spawn provenance; §15.4 exit 4 on a partial
    publish; `E-CLI-PUBLISH` (+ page + index row) refuses a live run
    under `--offline` instead of silently downgrading to a forecast.
  - 5 CLI tests: shim-recorded argv (the packaged tool never runs),
    byte-stable JSON (I3), routing fail-closed with zero calls, the
    publish-disabled no-op, gated rows over a broken tool.
- **Micro-debt/docs drift fixed (additive, not milestone
  substitutes).** The three broken rustdoc links at base (the doc gate
  had never been run green with them in place) — de-linked per the
  house repair convention; the xtask `every_current_member_has_a_rule`
  list extended to all 17 members (`halyard-forge-github`,
  `halyard-registry` were missing); root README status/layout/daily-
  command rows for the publish engine + example; CLI README `publish`
  row + section + the corrected writing-commands paragraph (three
  writing surfaces now: `apply` filesystem, `release` git refs,
  `publish` registries); arch §16.2 wired-commands note (publish
  moved from the absent list to the wired list) and the `Command` enum
  doc.
- **Tests/bench/example totals.** 30 engine tests + 2 argv-table test
  files (6 tests) + 5 CLI tests + 1 policy unit test; benches
  `publish_plan_layering_40` + `publish_engine_pass_40` registered and
  run; example `publish_plan` run clean. Full gate battery re-run
  green on the final tree: fmt, clippy `--workspace --all-targets
  --all-features -- -D warnings` (0 findings), the lint-hygiene grep
  (empty), `test --workspace --all-features --locked` (97 suites, 733
  tests, 0 failed), `test --workspace --benches`, the `-D warnings`
  doc sweep, `check-layers` (18 crates), `schema-check` (both schemas).
- **Honest state at cut-off.** The live publish spawn is exercised
  through argv-table unit tests + the PATH-shim CLI tests; a real
  `cargo publish`/`pnpm publish` e2e against real registries belongs
  to the M5.3/nightly lanes (the engine's resume makes any real run
  safe to re-run). `publish --resume` is deliberately not a flag: the
  engine always resumes (I2) — the §16.2 `--resume` spelling can join
  with the M5.3 auth work. OIDC trusted publishing, provenance and
  secret scrubbing stay M5.3+ (their own VERIFY research). The
  corpus-wide pnpm differential and the M2.6 dhat re-audit remain open
  (sandbox lacks pnpm; the two p50 perf-budget misses stay as
  recorded). Tree left dirty (no commits made).
- **Next session start point.** (1) M5.3: OIDC/token auth plumbing +
  secret scrubbing over the `PublishEffect` (VERIFY research first),
  (2) the corpus-wide pnpm differential as an xtask subcommand,
  (3) the M2.6 dhat re-audit with the cache warm, (4) the
  `github-release-notes` PR-comment mode.
### 2026-11-16 — M5.3 step 6's npm half + M10 step 2 (ADR-0033/0034, tree left dirty, no commits)

- **Inventory at session start.** Main was green at `d3c1dd7` (the
  affected/man slice). The last commit recorded the npm-provenance
  VERIFY outcome — the declared next work. Baseline gates re-run green
  (stable 1.10x, `debuginfo=0` — the sandbox's 10G disk hit the target
  ceiling twice before the flag was consistent; a linker bus-error is
  resource, not code). The doc gate at base: three stale spots found
  and fixed in-session (below).
- **M1 implemented (no stubs), one full red→green cycle** (the engine
  gate/rows tests, the argv tests, the config tri-state tests, the
  planner contract tests and the CLI e2e tests written first — red on
  the unresolved `NpmPublishOptions`/`ProvenanceRuntime` APIs and the
  missing `--ci` argument, on record — then implemented in dependency
  order): **the npm publish options end to end (ADR-0033)** —
  `publish.npm.access`/`provenance` resolve as semantic-unset
  `Option`s (the pnpm.io fetch: "Default: undefined", "When unset,
  trusted publishing decides" — the old guessed defaults
  `access=Public`/`provenance=true` would have invented flags every
  publish), the planner fills the npm releases' `PlanPublish`
  (optional, schema-additive fields; cargo carries `None`), the engine
  surfaces them on targets and rows, `pnpm_publish_args` emits
  `--access`/`--provenance`/`--no-provenance` only on explicit values,
  and a live `provenance = true` run refuses `E-PUB-AUTH` before any
  call when no supported cloud runtime is detected — the detection
  (`provenance_runtime_detected`: the GHA OIDC context or GitLab's
  `GITLAB_CI` marker, VERIFIED "true when available") stays in the
  shell, the engine consumes `PublishPolicy::provenance_runtime` as
  plain data (default `NotDetected`, fail-closed; the dry forecast
  never refuses, I5). Doctor gains the `publish-provenance` check.
  Tests: 8 engine/argv/config/planner suites + 4 CLI e2e (dry argv+rows,
  live refusal with zero spawns/probes, GitLab-detected live run over
  loopback, unset-stays-bare). Bench `publish_args` registered and run
  (unset ≈36 ns, combined-dry ≈206 ns, opt-out ≈123 ns — the assembly
  is free). Example `publish_npm_options` runs clean. Plan schema
  regenerated (two optional props, additive).
- **M2 implemented (M1 green end-to-end first), red→green**: **the
  CI-selector adapters (ADR-0034, M10 step 2)** — the
  `CiSelectorAdapter` seam in `halyard_core::ci_selector`
  (`SelectedPackage { name, root }` inputs; three impls — Turbo
  `--filter=...pkg` per root, Nx `--projects=a,b` over the closure,
  Bazel `rdeps(//..., //root/...)` over the patterns; VERIFIED quotes
  in `docs/research/ci-selector-grammars.md`), the closed
  `ci_selector_adapter` lookup (unknown ⇒ `None`, extending one is an
  ADR), the CLI's `affected --ci SYSTEM` value enum beside `--format`
  (the `--graph-format` lesson), an empty selection rendering nothing
  (exit 0). Tests: 8 core adapter contract tests + 3 CLI gates; bench
  `affected_ci` registered (the 40-package chain shape); example
  `affected_ci` runs clean.
- **Docs drift fixed / micro-debt cleaned (additive, not milestone
  substitutes).** The §16.2 table's `publish [--resume]` row corrected
  to `publish [--dry]` (the engine always resumes, I2 — stale since
  M5.2); §13 item 6's provenance note + the M5.3-later as-built note
  rewritten to the wired reality; the duplicated "the channel was
  validated at resolve time" comment in `resolve.rs` dropped; the
  npm-provenance research note extended with the 2026-11-16 pnpm/GitLab
  quotes; E-PUB-AUTH page + index row for the runtime refusal;
  ADR-0033/0034 recorded; CLI README gains the `--ci` section and the
  npm-rows note.
- **Honest state at cut-off.** The npm trusted-publishing OIDC needs
  **no** halyard exchange (the tool performs it; the as-built note
  records the non-interference posture) — the remaining M5.4 items
  (npm provenance attestations for our own binaries, SLSA/cosign/SBOM)
  are unchanged. The corpus-wide pnpm differential and the M2.6 dhat
  re-audit remain open (sandbox limits as recorded). Tree left dirty
  (no commits made).
- **Next session start point.** (1) M5.4: provenance/SLSA for halyard's
  own release artifacts (VERIFY first), (2) the corpus-wide pnpm
  differential as an xtask subcommand, (3) the M2.6 dhat re-audit with
  the cache warm, (4) more CI adapters (GitLab matrix / GitHub jobs
  JSON) behind the closed ADR-0034 seam.

### 2026-11-16 — the matrix-jobs CI adapters (ADR-0035, tree left dirty, no commits)

- **Inventory at session start.** Main was green at `0672ba4` (the
  npm-options + CI-adapter slice). Baseline gates re-run green (fmt,
  clippy `--workspace --all-targets --features ci-bundle -- -D
  warnings`, the lint-hygiene grep empty, the workspace test run 0
  failed) before any edit. The declared next work (the ADR-0034
  consequence) was the seam's own "more CI systems" extension.
- **M1 implemented (no stubs), one full red→green cycle** (the core
  adapter contract tests, the empty-selection loop entry and the CLI
  e2e gates written first — red on the unresolved `gha` lookup and
  `invalid value 'gha' for '--ci <SYSTEM>'`, on record — then
  implemented in dependency order): **the GitHub Actions matrix
  adapter (`affected --ci gha`, ADR-0035)** — one compact JSON array
  over the affected closure (docs.github.com VERIFIED: "You can use
  the output from one job to define matrices for multiple jobs"; the
  output value `colors=["red", "green", "blue"]` fed to
  `fromJSON(needs.…outputs.…)` inside the downstream
  `strategy.matrix`), the escaping owned by `serde_json` (ADR-0005;
  never hand-rolled), the consumer names the matrix dimension on its
  side so no key policy is encoded. Actions computes no dependency
  closure, so — unlike turbo/bazel — the array encodes the full
  affected closure.
- **M2 implemented (M1 green end-to-end first), red→green**: **the
  GitLab `parallel:matrix` adapter (`affected --ci gitlab`,
  ADR-0035)** — one `- HALYARD_PKG: <name>` hash per affected package
  (docs.gitlab.com/ci/yaml VERIFIED: "an array of hashes of variables
  … The matrix identifiers, which become the variable names, can use
  only numbers, letters, and underscores"; "each instance of the job"
  runs "with different variable values"). `HALYARD_PKG` is the ADR's
  documented identifier policy (namespaced: it cannot collide with the
  job's own variables); the consumer pastes the lines under
  `parallel: matrix:` and reads `$HALYARD_PKG` in the script. The
  base's refusal pin (`ci_selector_adapter("gitlab").is_none()`) was
  the red hook — flipped to the five-system lookup with `teamcity` as
  the refused unknown.
- **Design record.** ADR-0035 (matrix-jobs adapters) under the
  unchanged ADR-0034 seam: trait, inputs and the
  empty-renders-nothing contract untouched; the closure-vs-roots
  split documented as a property of the consuming system (turbo/bazel
  compute dependencies themselves and take the change roots; nx/gha/
  gitlab compute nothing and take the full closure). The
  ci-selector-grammars research note carries both systems' fetched
  quotes.
- **Tests/bench/example totals.** +2 core contract tests (gha array
  over the closure, one-line-many-packages) + 1 gitlab hash test +
  the extended empty-selection loop (five systems) + the flipped
  lookup pin; +3 CLI e2e gates (gha render, gha honest empty, gitlab
  render) — 11 core + 10 CLI green; benches `ci_selector_gha_40chain`
  + `ci_selector_gitlab_40chain` registered over the same 40-package
  chain shape; example `affected_ci` prints all five adapters.
- **Docs drift fixed / micro-debt cleaned (additive, not milestone
  substitutes).** Root README status row gained the `--ci SYSTEM`
  surface (stale since ADR-0034) and the daily-command rows for the
  two landed-but-unlisted examples (`halyard-core --example
  affected_ci`, `halyard-publish --example publish_npm_options`);
  the `affected_ci` example's stray double-space print artifact
  normalized; the example/module doc headers updated to the
  five-adapter reality; arch §16.3 as-built note and the CLI README
  adapters section extended.
- **Honest state at cut-off.** The adapters render only; no CI system
  is executed by halyard (I5 — the consumers wire the lines into
  their own pipelines). More adapters (Azure Pipelines variables,
  TeamCity) remain behind the closed ADR-0035 lookup. Tree left dirty
  (no commits made).
- **Next session start point.** (1) M5.4: provenance/SLSA for
  halyard's own release artifacts (VERIFY first), (2) the corpus-wide
  pnpm differential as an xtask subcommand, (3) the M2.6 dhat
  re-audit with the cache warm.

### 2026-11-17 — the action-output seam + the circleci adapter (ADR-0037 + ADR-0038, tree left dirty, no commits)

- **Inventory at session start.** Main was green at `381f35a` (the
  azure/teamcity adapters session, landed). Baseline gates re-run
  green before any edit. The declared next start points were weighed:
  M5.4 provenance/SLSA (external tooling — the composite distribution
  needs releases to attest, so the reachable half of §12 was chosen
  instead), the corpus-wide pnpm differential (the sandbox lacks
  pnpm, as recorded), the M2.6 dhat re-audit (profiling, not a
  capability). The reachable net-new capability: §12's action glue —
  the outputs contract has no dependency on releases existing (the
  plan wire types are §5.8, the plan mode is offline, I5), while the
  composite wrapper stays with M5.4's own-artifact slice.
- **M1 implemented (no stubs), one full red→green cycle** (the
  crate contract tests and the CLI e2e gates written first — red on
  `error[E0432]: unresolved import halyard_action` and
  `error: unrecognized subcommand 'action'`, on record — then
  implemented in dependency order): **the action-output seam
  (`halyard-action`, ADR-0037)** — the §12 outputs vocabulary over
  the injected `ActionWriter` (`set_output` / `append_summary` /
  `emit_warning`): the real `GithubActionsWriter` (the `GITHUB_OUTPUT`
  env file in the documented `{name}={value}` and
  `{name}<<HALYARD` heredoc forms — VERIFIED quotes in the new
  `docs/research/github-actions-outputs.md`; values that cannot ride
  verbatim refuse `E-ACTION-OUTPUT`, never rewritten; the
  `GITHUB_STEP_SUMMARY` append with the runner's automatic-newline
  semantics; the `::warning::` degraded-mode annotation collapsing to
  one log line) and the `PreviewActionWriter` (the ordered call log,
  the `MemoryForge` poison-recovery convention) — `from_env()` is
  honestly `None` outside a workflow step, and the CLI renders through
  the preview there, so the outputs are observable locally. The §12
  `plan` mode lands as the shell's `halyard action [--mode plan]
  [--plan-out PATH]`: plan → the canonical plan file (the `plan_json`
  pointer, `E-ACTION-PLAN` on a missing parent — the caller's
  contract, never invented) → the outputs (`released`/`published`
  honestly `false`, `releases_json` rows with the url empty until a
  forge mode fills it, the per-package `--version`/`--tag` keys,
  `paths_released` = the distinct edit sites sorted and deduplicated,
  I3) → the job summary (the release table, the honest counts, the
  walk's own work counters standing in for the timing breakdown —
  wall-clock would break I3; zero API calls stated, not implied). The
  degraded-mode annotation wires to `HistoryStats::degraded` — the
  engine's own §8.3 marker, never a guess. The remaining §12 modes
  (`auto|pr|release|check`) are absent rather than stubbed (a
  one-variant value enum by design).
- **M2 implemented (M1 green end-to-end first), one full red→green
  cycle** (the core contract tests, the extended empty-selection loop
  and the CLI e2e gate written first — red on
  `ci_selector_adapter("circleci")` returning `None` and
  `invalid value 'circleci' for '--ci <SYSTEM>'`, on record — then
  implemented): **the CircleCI dynamic-config adapter (`affected --ci
  circleci`, ADR-0038)** — one paste-ready
  `parameters: '{ "halyard_pkgs": [...] }'` line over the affected
  closure, name-sorted (circleci.com VERIFIED 2026-11-17: the
  continuation orb's `continue` command takes `parameters` — "The
  `parameters` parameter accepts a JSON object as a string. Note the
  single quote encapsulating the full JSON object" — the documented
  shape is `parameters: '{ "further": true }'`; the continuation
  config declares the matching pipeline parameter and consumes it
  through the documented `<< pipeline.parameters.… >>` / `when:` keys;
  the quotes are in the extended research note, checked line updated).
  The base's refusal pin (`ci_selector_adapter("circleci").is_none()`)
  was the red hook — flipped to the eight-system lookup with
  `buildkite` as the refused unknown. `halyard_pkgs` is the
  halyard-owned identifier in the parameter grammar's own casing (the
  docs' own lower-snake examples), the ADR-0035 policy carried over;
  the hand-rendered JSON literal never needs escaping (neither
  package-name grammar carries a quote or a backslash).
- **Layer table updated consciously** (`xtask/src/layers.rs`):
  `halyard-action` — workspace deps `halyard-core`, external
  `serde_json` — plus the membership pins in the two layer tests; the
  crate's Cargo.toml registers the `render` bench (`harness = false`).
- **Tests/bench/example totals.** +10 crate contract gates (the
  §12 vocabulary order and shapes, the fail-closed output-name
  grammar incl. scoped npm keys, the env-file entry grammar incl. the
  delimiter collision refusal and the trailing-newline rule, the real
  writer's append bytes over temp files, the summary-without-sink
  refusal, the honest `from_env` absence, the preview's ordered calls,
  the row grammar, the job summary text, the canonical plan file) + 4
  CLI e2e gates (the preview posture over a real fixture workspace,
  the env-files posture with the sinks set, the `--format json` output
  map, the `--plan-out` contract with the missing-parent refusal) —
  14 green; benches `action_render/plan_outputs_40_releases` (≈ 38 µs
  over the 40-release plan — the render is not the step's cost) +
  `env_file_entry_single_line` / `env_file_entry_heredoc`
  (sub-microsecond); example `plan_forecast` prints the preview call
  log, the env-file block and the summary. M2: +3 core contract tests
  (the continuation `parameters` line over the closure,
  one-line-many-packages) + the extended empty-selection loop (eight
  systems) + the flipped lookup pin (eight named, `buildkite`
  refused) + 1 CLI e2e gate (the circleci render) — 16 core + 15 CLI
  green; bench `ci_selector_circleci_40chain` registered over the same
  40-package chain shape; example `affected_ci` prints all eight
  adapters.
- **Docs drift fixed / micro-debt cleaned (additive, not milestone
  substitutes).** PROGRESS.md's azure/teamcity entry claimed "tree
  left dirty (no commits made)" while the session had landed as
  `9649278..381f35a` — the cut-off text now carries the provenance
  parenthetical; the §7.1 as-built note now counts 19 members and one
  remaining unbuilt slot (`halyard-plugin-host`); the CLI README's
  status row split `bench`/`action` (the former stays absent, the
  latter carries its section with a pasteable transcript); the
  `E-CHLOG-CHANGE-FILE` CLI construction no longer double-encodes the
  code in the message text (the `CliError::new` code is the carrier —
  the message prefix was the only such double-encode); two
  missing-consumer examples added (`halyard-cache`'s write/read
  contract, `halyard-eco-npm`'s publish-argv half) with their
  README daily-command rows.
- **Honest state at cut-off.** The §12 `plan` mode renders and writes
  only; no release, PR or publish is executed by `halyard action`
  (their §12 modes land with their executors). The composite wrapper
  (`action.yml`, binary download, checksum + attestation) stays with
  M5.4's own-artifact slice. The circleci adapter renders only — no CI
  system is executed by halyard (I5 — the consumers wire the lines
  into their own pipelines); more adapters (Buildkite, Jenkins) remain
  behind the closed ADR-0038 lookup — `buildkite` is the contract
  tests' refused unknown. Tree left dirty (no commits made).
- **Next session start point.** (1) M5.4: provenance/SLSA for
  halyard's own release artifacts (VERIFY first — it also unblocks
  the §12 composite distribution), (2) the corpus-wide pnpm
  differential as an xtask subcommand, (3) the M2.6 dhat re-audit
  with the cache warm.


### 2026-11-17 — the azure/teamcity CI adapters (ADR-0036, tree left dirty, no commits)

- **Inventory at session start.** Main was green at `597777b` (the
  matrix-jobs adapters session). Baseline gates re-run green before any
  edit. The declared next work (the ADR-0034/0035 consequence) was the
  seam's own "more CI systems" extension — the two remaining recorded
  systems, Azure Pipelines and TeamCity. The other recorded start
  points stay open for their own sessions: M5.4 provenance/SLSA
  (VERIFY research + external tooling), the corpus-wide pnpm
  differential (sandbox lacks pnpm, as recorded), the M2.6 dhat
  re-audit (profiling, not a capability).
- **M1 implemented (no stubs), one full red→green cycle** (the core
  adapter contract tests, the empty-selection loop entry and the two
  CLI e2e gates written first — red on the unresolved `azure` lookup
  and `invalid value 'azure' for '--ci <SYSTEM>'`, on record — then
  implemented in dependency order): **the Azure Pipelines adapter
  (`affected --ci azure`, ADR-0036)** — one
  `##vso[task.setvariable variable=HALYARD_PKGS;isOutput=true]`
  logging command whose value is the affected closure, name-sorted and
  comma-joined (learn.microsoft.com VERIFIED: the command "sets a
  variable in the variable service of taskcontext"; the cross-job
  reference is `dependencies.<job-name>.outputs['<step-name>.
  <variable-name>']`; the documented consumption of a comma-joined
  variable is the `split(variables.environments, ',')` + `${{ each }}`
  example). The command's escape sequences (`;`→`%3B`, `\n`→`%0A`,
  `\r`→`%0D`, `]`→`%5D`, `%`→`%AZP25`) never fire — neither
  package-name grammar carries an escapable character, so the value
  rides verbatim. `HALYARD_PKGS` is the halyard-owned identifier
  (the ADR-0035 policy carried over).
- **M2 implemented (M1 green end-to-end first), red→green**: **the
  TeamCity adapter (`affected --ci teamcity`, ADR-0036)** — one
  `param("env.HALYARD_PKG", listOf(value(…), …))` line over the
  affected closure (jetbrains.com VERIFIED: the Matrix Build feature
  "define a collection of builds by iterating over specified parameter
  values and generating a build for every combination"; the documented
  DSL entry is `param("Browser", listOf(value("Chrome"),
  value("Firefox")))` and the documented sample iteration set carries
  an `env.*` parameter, `env.ShouldFail: true, false`). The consumer
  pastes the line inside its `matrix { }` block and each instance
  reads `$HALYARD_PKG` — the ADR-0035 identifier policy carried over.
  The base's refusal pin (`ci_selector_adapter("teamcity").is_none()`)
  was the red hook — flipped to the seven-system lookup with
  `circleci` as the refused unknown.
- **Design record.** ADR-0036 (azure/teamcity adapters) under the
  unchanged ADR-0034 seam: trait, inputs and the empty-renders-nothing
  contract untouched; the closure-vs-roots split documented as a
  property of the consuming system (turbo/bazel compute dependencies
  themselves and take the change roots; nx/gha/gitlab/azure/teamcity
  compute nothing and take the full closure). The alternatives
  considered and rejected are recorded in the ADR: the JSON-array shape
  for Azure (the docs' own end-to-end example is the comma-join +
  `split`), the `setParameter` service-message path for TeamCity
  (runtime steps, not build-configuration iteration — and
  `reverse.dep.*` "do not work correctly in matrix builds"), one
  `value(…)` atom per package (the matrix block consumes a
  `param(name, listOf(…))` entry). The ci-selector-grammars research
  note carries both systems' fetched quotes (checked line updated to
  2026-11-17).
- **Tests/bench/example totals.** +2 core contract tests (azure
  setvariable line over the closure, one-line-many-packages) + 2
  teamcity tests (the param line, one-line-many-packages) + the
  extended empty-selection loop (seven systems) + the flipped lookup
  pin (seven named, `circleci` refused); +3 CLI e2e gates (azure
  render, azure honest empty, teamcity render) — 14 core + 14 CLI
  green; benches `ci_selector_azure_40chain` + `ci_selector_teamcity_40chain`
  registered over the same 40-package chain shape (≈86 ns / ≈146 ns —
  the render stays free); example `affected_ci` prints all seven
  adapters.
- **Docs drift fixed / micro-debt cleaned (additive, not milestone
  substitutes).** The stale "three adapters" bench header and the
  stale test-file header (both still named the ADR-0034 trio after the
  ADR-0035 adapters landed) rewritten to the current reality; the
  trait's `system()` doc (still enumerating only the original trio)
  rewritten to point at the lookup's keys; the CLI `--ci` flag doc and
  the `cmd_affected` doc (ADR-0034-only) extended to the seven-system
  reality; root README status row gains `--ci SYSTEM` (stale since
  ADR-0034) and the Layout section's `halyard-cli` command list gains
  the landed-but-unlisted `affected`/`man`; arch §16.2 command table
  gains the missing `affected` row and the §16.3 as-built note extends
  to the seven adapters; CLI README: the `affected` status row carries
  `--ci SYSTEM`, the affected section gains the `--ci` example, the
  CI-adapters section rewritten to the seven systems (and the ADR-0033
  npm-options note moved out of it into the Publishing section where it
  belongs — the structural debt of the section-after-`## Errors`-with-
  no-blank-line also fixed); the E-GIT-REF error page's "Raised by"
  gains the `affected --base` unresolvable-ref refusal and the `fetch`
  tag probe (the page predated both callers); ADR-0036 recorded.
- **Honest state at cut-off.** The adapters render only; no CI system
  is executed by halyard (I5 — the consumers wire the lines into their
  own pipelines). More adapters (CircleCI dynamic config, Buildkite,
  Jenkins) remain behind the closed ADR-0036 lookup — `circleci` is
  the contract tests' refused unknown. (The cut-off text predates the
  docs commit: the session's work landed on main as
  `9649278..381f35a`; the tree is clean at the head of this entry.)
- **Next session start point.** (1) M5.4: provenance/SLSA for
  halyard's own release artifacts (VERIFY first), (2) the corpus-wide
  pnpm differential as an xtask subcommand, (3) the M2.6 dhat
  re-audit with the cache warm.

### 2026-11-17 — the buildkite + jenkins CI adapters (ADR-0039 + ADR-0040, tree left dirty, no commits)

- **Inventory at session start.** Main was green at `2c715c4` (the
  action-seam + circleci-adapter session). Baseline
  `cargo check --workspace --all-targets --features ci-bundle --locked`
  exit 0 before any edit. The declared next work (the ADR-0038
  consequence) was the seam's own "more adapters" extension — the two
  remaining recorded systems, Buildkite and Jenkins. The other recorded
  start points stay open for their own sessions: M5.4 provenance/SLSA
  (VERIFY research + external tooling), the corpus-wide pnpm
  differential (sandbox lacks pnpm, as recorded), the M2.6 dhat
  re-audit (profiling, not a capability). Survey notes: the lint
  surface stays structurally clean (zero `todo!`/`unimplemented!`/
  `#[allow]` anywhere; the miri/loom exclusion machine-enforced per
  ADR-0030; no bench or test requires nightly); benches registered 1:1
  with files across all crates; the errors taxonomy exact (73 codes ↔
  76 pages, one retired); every non-xtask crate carries consumer
  examples.
- **M1 implemented (no stubs), one full red→green cycle.** The red was
  the missing API itself, captured on both suites before any
  implementation: `ci_selector.rs` contract tests 4 failed / 14 passed
  (`buildkite_renders_the_matrix_dimension_over_the_affected_closure`
  + the many-packages variant panicking "buildkite is a known adapter",
  the empty-selection loop, the lookup 8→9 assertion), the CLI gate
  failing on `error: invalid value 'buildkite' for '--ci <SYSTEM>'`
  (the value enum refuses before the lookup, exactly as designed). The
  green landed **ADR-0039, the Buildkite adapter (`affected --ci
  buildkite`)** — one paste-ready `matrix: setup: halyard_pkg:` block
  over the affected closure, name-sorted, one quoted-string element per
  package (buildkite.com/docs VERIFIED 2026-11-17: matrices "expand a
  step template and array of matrix elements into multiple jobs";
  dimension keys map to lists of values interpolated as
  `{{matrix.<key>}}`; the documented 25-elements-per-dimension and
  50-jobs caps are the consumer's policy — the adapter renders the
  grammar, never the guard). The lookup, the trait/lookup/module docs,
  the CLI value enum + flag doc, the bench
  (`ci_selector_buildkite_40chain` ≈132 ns) and the `affected_ci`
  example all moved one row over; the refused-unknown pin moved
  `buildkite` → `jenkins` (the ADR-0036→0038 trail).
- **M2 implemented (M1 green end-to-end first: 18/18 core, 11/11 CLI,
  bench + example verified), red→green.** The red: 4 core failures +
  the CLI gate's `invalid value 'jenkins'`. The green landed **ADR-0040,
  the Jenkins adapter (`affected --ci jenkins`)** — one paste-ready
  declarative `axis { name 'HALYARD_PKG' values 'app', 'lib' }` block
  over the affected closure, name-sorted (jenkins.io/doc/book/
  pipeline/syntax VERIFIED 2026-11-17: the `matrix` section defines "a
  multi-dimensional matrix of name-value combinations to be run in
  parallel" ("cells"); "Each `axis` consists of a `name` and a list of
  `values`"; and the grammar's own rule "No semicolons as statement
  separators. Each statement has to be on its own line." pins the
  one-statement-per-line render shape). The pin moved `jenkins` →
  `bitbucket` (the next unrendered recorded system; its grammar VERIFY
  precedes any adapter); the contract loop names ten systems.
- **Tests/bench/example totals.** +4 core contract tests (buildkite
  block + many-packages, jenkins block + many-packages), the
  empty-selection loop extended to ten systems, the lookup pins flipped
  twice on record (buildkite refused → jenkins refused → bitbucket
  refused); +2 CLI e2e gates (`affected_ci_buildkite_renders_the_
  matrix_dimension_block`, `affected_ci_jenkins_renders_the_axis_block`)
  — 20/20 core + 17/17 `cli_affected` green; benches
  `ci_selector_buildkite_40chain` (≈132 ns) + `ci_selector_jenkins_
  40chain` (≈182 ns) registered over the same 40-package chain shape;
  example `affected_ci` prints all ten adapters.
- **Docs drift fixed / micro-debt cleaned (additive, not milestone
  substitutes).** Drift: the `CiSelectorAdapter::system()` doc and the
  module doc's flag spelling (both omitted `circleci` after ADR-0038
  landed), the `affected_ci` bench's stale "seven adapters" header, arch
  §16.2 `affected` row (ADR-0038/0039/0040 added) and §16.3 as-built
  note (buildkite + jenkins rows, the closure-vs-roots list extended),
  CLI README status row (the `--ci` grammar list stopped at teamcity) +
  adapters section + example commands, root README status row (the CI
  enumeration stopped at circleci), root README Layout CLI command list
  (the shipped `action` command was missing), root README status
  paragraph (the shipped OIDC trusted publishing of ADR-0031 and the
  npm publish-options row of ADR-0033 had zero mentions), the
  `E-CFG-TOML` and `E-TEST-FIXTURE` error pages gained their "Raised
  by" producer lines (the page-format invariant the other 74 pages
  follow). Micro-debt: three zero-caller public items removed —
  `FixedHistory::digest_mut` (its doc claimed a test-fixture consumer
  that never existed), `halyard_core::edit::slice_span` (its doc
  claimed it was "shared by adapter implementations"; no adapter calls
  it), `halyard_core::discover::ResolvedDep` (constructed and consumed
  by nothing, not even re-exported) — and one weak error repaired: the
  `HALYARD_APP_INSTALLATION_ID` parse failure now carries the offending
  value (`got {raw:?}`), the file's own house standard one screen down
  (the `map_err(|_|)` dropped it).
- **Honest state at cut-off.** The adapters render only — no CI system
  is executed by halyard (I5 — the consumers wire the lines into their
  own pipelines). Buildkite's documented 25-element/50-job caps and
  Jenkins' declarative-plugin availability are consumer-side concerns,
  recorded in the ADRs. The lookup stays closed: `bitbucket` is the
  contract tests' refused unknown. Tree left dirty (no commits made).
- **Next session start point.** (1) M5.4: provenance/SLSA for
  halyard's own release artifacts (VERIFY first), (2) the corpus-wide
  pnpm differential as an xtask subcommand, (3) the M2.6 dhat re-audit
  with the cache warm.

### 2026-11-17 — the Bitbucket parallel-steps adapter (ADR-0041) + the `halyard bench` measurement command (ADR-0042, tree left dirty, no commits)

- **Inventory at session start.** Main was green at `9d420e8` (the
  buildkite + jenkins session). Baseline
  `cargo check --workspace --all-targets --features ci-bundle --locked`
  exit 0 on a fresh stable toolchain 1.100.0 before any edit. The
  declared next work (the ADR-0040 consequence) was the seam's last
  recorded system — Bitbucket Pipelines, the contract tests' refused
  unknown. The other recorded start points stay open for their own
  sessions (M5.4 provenance/SLSA needs the live-CI lane; the
  corpus-wide pnpm differential lacks a pnpm binary; the M2.6 dhat
  re-audit is profiling). Survey notes: the lint surface stays
  structurally clean (zero `todo!`/`unimplemented!`/`#[allow]`
  anywhere; the miri/loom exclusion machine-enforced per ADR-0030; no
  bench or test requires nightly); the errors taxonomy exact (74 codes
  ↔ 74 pages, one retired); every non-xtask crate carries consumer
  examples.
- **M1 implemented (no stubs), one full red→green cycle.** The red was
  the missing API itself, captured on both suites before any
  implementation: `ci_selector.rs` contract tests 4 failed / 18 passed
  (the two `bitbucket` renders panicking "bitbucket is a known
  adapter", the empty-selection loop, the lookup 10→11 assertion), the
  CLI gate failing on `error: invalid value 'bitbucket' for '--ci
  <SYSTEM>'` (the value enum refuses before the lookup, exactly as
  designed). The green landed **ADR-0041, the Bitbucket adapter
  (`affected --ci bitbucket`)** — one paste-ready `parallel:` block
  over the affected closure, name-sorted, one
  `export HALYARD_PKG=<name>` script line per affected package
  (support.atlassian.com/bitbucket-cloud VERIFIED 2026-11-17:
  "Parallel steps allow you to group pipeline steps that can run at
  the same time (concurrently) to reduce build time"; the required
  `steps:` property "contains the lists of `step`s in a `stage` or a
  `parallel`. At least 1 `step` is required within the steps list";
  the docs' own example varies the steps **in the script**
  (`--batch 1` / `--batch 2`); there is no per-step `env` property —
  the `variables` option's "Allowed parent properties" is `[pipe]` —
  so the selection can only ride literal per-step script content; the
  relative indentation follows the documented example exactly;
  `BITBUCKET_PARALLEL_STEP`/`_COUNT` exist for addressing steps, not
  for building selection arrays — the indexed-array alternative was
  rejected on record). The "100-step limit on pipelines includes each
  step in a parallel group" cap is the consumer's policy — the
  adapter renders the grammar, never the guard. The lookup, the
  trait/lookup/module docs, the CLI value enum + flag doc, the bench
  (`ci_selector_bitbucket_40chain`) and the `affected_ci` example all
  moved one row over; the refused-unknown pin moved `bitbucket` →
  `drone` (an unrecorded system — no adapter follows from it; the
  recorded CI surface ADR-0034→0041 is complete).
- **M2 implemented (M1 green end-to-end first: 22/22 core, 18/18 CLI,
  bench + example verified), red→green.** The red: 5 CLI gates / 0
  passed on the unrecognized `bench` subcommand. The green landed
  **ADR-0042, the §16.2 declared-absent `bench` command** — the
  built-in measurement of the planning pipeline over the current repo:
  one untimed warmup pass (page cache + the §8.7 per-commit cache warm
  — the §14.1 no-op-warm model) precedes `--iterations` timed passes
  (default 8; zero refuses the new `E-CLI-BENCH` fail-closed before
  the warmup runs, page + index row on record); each pass runs the
  same `discover` → `config` → `history` → `plan` pipeline every
  planning command runs, timed through the same span vocabulary the
  `--profile` trace emits via the new read-only
  `Profiler::recorded_spans()` accessor (a disabled profiler records
  nothing, so the warmup is untimed by construction); `--no-cache` is
  the cold measurement and the report names the posture either way;
  the reduction is pure data on the lib target (`halyard_cli::bench`
  — the man renderer's rationale: the bench harness and the bin share
  one implementation): per-stage grouping + the nearest-rank
  percentiles (`ceil(p/100 × n)`, the definition the §14.1 budget
  table speaks) + min + integer mean, name-sorted (I3), 5 module unit
  tests (percentile math, grouping, duration formatting); the report
  carries the repo's shape beside the timings (packages, pending
  commits, cache posture) — **a measurement, never a gate**: the
  §14.1 budgets are shape-bound to the xtask fixtures
  (`bench/budgets.toml`) and both renderings say so verbatim; all
  three formats (the JSON's schema, stage order and shape are the
  run-stable contract — the timings are wall-clock, pinned as such by
  the gates); `bench_report` criterion bench (the aggregation over
  8/200-pass sample counts) + the `[[bench]]` registration; the man
  pages/completions gain the command automatically (the
  clap-derivation rule).
- **Tests/bench/example totals.** M1: +4 core contract tests
  (bitbucket block + many-packages, the empty-selection loop to
  eleven systems, the lookup pin flipped on record: bitbucket refused
  → drone refused), +1 CLI e2e gate — 22/22 core + 18/18
  `cli_affected` green; bench `ci_selector_bitbucket_40chain` over the
  40-package chain; example `affected_ci` prints all eleven adapters.
  M2: 5 CLI gates (`tests/cli_bench.rs`: the human stage table + shape
  line, the JSON schema stability with name-sorted stages, the
  `--iterations` sample-count contract, the `--no-cache` cold posture,
  the zero-iteration `E-CLI-BENCH` refusal via the JSON error
  document) + 5 module unit tests; bench `bench_report`
  (aggregate_4stages_8passes + _200passes); the `bench` command run
  against this very workspace (19 packages, 858 pending commits) as
  the dogfood check.
- **Docs.** ADR-0041 + ADR-0042; the Bitbucket section in
  `docs/research/ci-selector-grammars.md` (the VERIFIED quotes + the
  no-per-step-env finding + the rejected alternatives); arch §16.2
  (`affected` row ADR-0041; the wired-commands note — `bench` moved
  from the absent list into the wired list with its as-built
  description) + §16.3 as-built note (the bitbucket rows, the
  closure-vs-roots list extended); CLI README (the `--ci` grammar
  lists → eleven in all three places, the adapters section + example
  commands + the `bench` status row rewritten from absent to as-built);
  root README status paragraph (the CI enumeration stopped at jenkins)
  + the Layout `halyard-cli` command list (the missing `bench`);
  `E-CLI-BENCH` page + index row (74 → 75, pages and producers exact);
  PROGRESS.md row.
- **Micro-debt cleaned (additive, not milestone substitutes).** Five
  zero-caller public items removed with their dead assertions:
  `Sym::to_u32` (zero callers anywhere), `Interner::len`/`is_empty`
  (own tests only — the `get` assertions already pinned the
  behavior), `Trie::len`/`is_empty` (zero callers), `bloom::
  commit_count` (own test only); `glob::match_segment` privatized
  (its only caller is the in-module walker) — and the `glob.rs`
  module doc repaired (it named `match_segments` as the
  single-segment `fnmatch`-style matcher; the single-segment matcher
  is `match_segment`, `match_segments` is the whole-path walker). The
  man bench's mirror tree repaired to the real 20-command count (the
  "same 18 commands" claim had drifted twice — `action` was never in
  the mirror).
- **Honest state at cut-off.** The recorded CI-selector surface is
  complete (ADR-0034→0041: eleven systems render; a new system means a
  new grammar VERIFY + ADR, as always). `halyard bench` measures; it
  enforces nothing — the budgets stay xtask-side by design. Tree left
  dirty (no commits made).
- **Next session start point.** (1) M5.4: provenance/SLSA for
  halyard's own release artifacts (VERIFY first), (2) the corpus-wide
  pnpm differential as an xtask subcommand (needs a pnpm binary), (3)
  the M2.6 dhat re-audit with the cache warm.

### 2026-11-17 — the §12 `pr` and `check` modes (ADR-0043 + ADR-0044, tree left dirty, no commits)

- **Survey.** The workspace at `db6bd91` (the Bitbucket + bench session):
  zero `todo!`/`unimplemented!`/`#[allow]`/`#[expect]` anywhere, no
  non-test `unwrap`, every E-code page exact — the tree was gates-green
  before any edit. Two drift items on record: the `Command` enum doc in
  the CLI bin still claimed `bench` "remains absent rather than stubbed"
  (ADR-0042 had landed it), and the root README's Daily-commands block
  had no `halyard bench` usage line. The journal's own next-step hints
  (M5.4 VERIFY-first, the pnpm differential needing a pnpm binary, the
  M2.6 dhat re-audit) are deferred-for-cause; the one declared-absent
  surface with no external dependency was §12's remaining modes — the
  session's milestone pair.
- **M1 implemented, red→green (ADR-0043, the §12 `pr` mode).** The red:
  the forge preview test refused the url-less `MachineEffect::PrOpened`,
  5 `halyard-action` contract tests refused the missing
  `PrEffect`/`PrPass`/`pr_outputs`/`pr_summary`, 4 CLI gates refused the
  unrecognized `--mode pr`. The green: **the §11.1 machine report now
  carries the forge's own URL** (`MachineEffect::PrOpened`/`PrUpdated`/
  `PrUnchanged`/`PrClosed` gained `url` — the `pr_url` §12 output
  renders what the adapter minted, never a composed guess; the `pr`
  report's JSON effect rows carry the same key, the human/GHA forecast
  lines byte-identical); the §11.1 pass assembly extracted from `cmd_pr`
  into the shell-internal `run_pr_pass` (one executor, two renderers —
  the adapter swap stays the only posture difference, M4.1b's rule);
  `halyard-action` gained the plain forge-free view
  (`PrEffect`/`PrView`/`PrPass` + `pr_outputs`/`pr_summary`): `released`
  is the pass's honest execution value (`true` only when the live tag
  phase created a release; a dry forecast stays `false`, I5), the
  `pr_number`/`pr_url` pair rides only when the pass has a Release PR to
  report (absent on PROPAGATE and the no-op pass), `releases_json` fills
  the pass's minted URLs by tag match over the plan rows (ADR-0037's
  "a forge mode fills it"), the summary's pass line names the effect,
  the minted releases and the forecast posture; `halyard action --mode
  pr [--dry]` wires it (the shared §12 delivery tail — sink selection,
  the JSON document, the in-step/preview postures — extracted into
  `render_action_run`/`deliver_outputs`, so the plan mode and the two
  new modes render through one helper). The plan-vs-mint divergence is
  pinned on record: in a PROPAGATE state the plan rows forecast the
  *next* release (0.1.2) while the tag phase mints the *manifest*
  version (v0.1.1) — the row url stays honestly empty and the summary's
  `minted <tag> (<url>)` segment names the pass's own mint.
- **M2 implemented (M1 green end-to-end first: 15/15 action contracts,
  11/11 cli_pr byte-stable after the refactor, forge suites green),
  red→green (ADR-0044, the §12 `check` mode).** The red: 2 contract
  tests refused the missing `CheckVerdict`/`TitleOutcome`/`CheckRow`/
  `check_summary`, 4 CLI gates refused the unrecognized `--mode
  check`/`--pr-title`. The green: the check surface's verdict through
  the seam — the shell's own `classify_title`/`predict` (the planner's
  mapping + the §10.2 pre-major remap, single-sourced) mapped onto the
  plain view, `check_summary` rendering the verdict line
  (`title accepted: <kind> → <level> · conveyed <pkg> <bump>, …`) beside
  the pending forecast's table; the outputs ride `plan_outputs` verbatim
  (the §12 shape stays mode-independent); fail-closed ordering is the
  contract — the classification precedes every effect, so a rejected
  title refuses `E-CLI-TITLE` before the plan file or any sink call
  exists (pinned by the JSON error channel + the no-plan-file
  assertion); without a title the pending forecast is the verdict.
- **Tests/bench/example totals.** M1: +1 forge preview gate (the
  url-carrying open forecast), +5 action contract tests (the documented
  order with the PR pair, the minted-url fill by tag, the honest
  `released`, the summary's opened/propagated/no-op/closed postures),
  +4 CLI e2e gates (the dry forecast, the PROPAGATE forecast, the
  env-files posture, the JSON document) — forge 68 + action 15 +
  `cli_action` 12 green, `cli_pr` 11 green (the report contract
  byte-stable after the extraction); bench groups
  `pr_outputs_40_releases` + `pr_summary_40_releases`; example
  `pr_forecast`. M2: +2 action contract tests (the verdict line, the
  rejected/no-title postures), +4 CLI e2e gates (the accepted verdict
  with the pre-major remap, the fail-closed refusal, the no-title
  forecast, the JSON verdict); bench `check_summary_40_releases`;
  example `check_forecast`.
- **Docs.** ADR-0043 + ADR-0044; arch §12 as-built (the two mode notes;
  the absent list narrowed to `auto|release`); CLI README (the action
  section's `pr` sample + `check` paragraph, the command row
  `--mode plan|pr|check … --dry --pr-title`); root README (the status
  paragraph, the crate row, the example list, the `halyard bench`
  usage line the survey flagged); the `halyard-action` crate
  description; PROGRESS.md row. No new error code — the modes ride the
  existing taxonomy (`E-FORGE-*`, `E-ACTION-*`, `E-CLI-TITLE`), the
  75-page taxonomy stays bidirectionally exact.
- **Micro-debt cleaned (additive, not milestone substitutes).** The
  stale `Command` enum doc repaired (the bench claim + the command
  inventory now name all twenty and the §12 mode growth). One
  zero-caller public item removed with its dead assertion:
  `Discovery::files_with_suffix` (halyard-core — its only consumer was
  its own test). A second candidate, `TagTargetResolver::
  with_max_commits`, was restored untouched: its "zero callers" turned
  out to miss the defining crate's own tests, which drive the
  walk-bound degrade path through it — load-bearing, not dead.
- **Honest state at cut-off.** The §12 modes render `plan|pr|check`;
  `pr` live mode is wired through the same machine (fail-closed on
  credentials) but untested against a real forge by design — the
  nightly forge-smoke lane is its probe. The PROPAGATE-state
  `releases_json` divergence (plan rows forecast the next release) is
  the documented contract, not a bug. Tree left dirty (no commits
  made).
- **Next session start point.** (1) the §12 `release` mode (the
  natural reuse: its plan rows always match its minted tags), (2) M5.4:
  provenance/SLSA for halyard's own release artifacts (VERIFY first),
  (3) the M2.6 dhat re-audit with the cache warm.

### 2026-11-17 — Drone environment adapter (ADR-0045) + the §12 `release` mode (ADR-0046) (tree left dirty, no commits)

- **Inventory at session start.** Main was green at `b3a8df4` (the
  bitbucket + bench session): fresh stable toolchain 1.100.0, the
  full gate battery green at base. The survey classified the tree:
  zero `todo!`/`unimplemented!`/`#[allow]` anywhere; all 18 library
  crates carry consumer examples; the command surface is the twenty
  wired commands (README's list exact); the §16.2 table's declared
  gaps are the two §12 modes (`auto|release`) and M5.4. The refused-
  unknown pin sat at `drone` ("the recorded CI surface ADR-0034→0041
  is complete"), the research note carried no Drone section, and the
  root README's affected sentence cited ADRs only through 0038 while
  already naming the buildkite/jenkins/bitbucket grammars (drift).
  Feature surface: `miri`/`loom` machine-enforced exclusive
  (`ci-bundle` is the conflict-free gate spelling); no bench requires
  nightly. Micro-debt candidates found: two `unreachable!` panic
  surfaces in production paths (the bench aggregation's shape and the
  commit-typo diagnosis guard) — the class the M4.7 session replaced
  with map-first builds.
- **M1 implemented (no stubs), one full red→green cycle.** The red
  was the missing adapter itself (4 `ci_selector` failures on the
  unknown lookup name + 1 CLI gate on `invalid value 'drone'`),
  captured on both suites before any implementation. The green landed
  the ADR-0045 design: the grammar VERIFIED live (the three
  docs.drone.io pages, quotes in the research note) — the per-step
  `environment` mapping is a documented first-class mechanism
  (unlike Bitbucket's `pipe`-only `variables`), so the render is one
  step-scoped fragment per affected package rather than a script-line
  assignment; a full step entry was rejected (it would guess the
  consumer's `image`/`commands` and invent step names). One test-side
  correction during green: none needed — the fragment shape landed
  first try. The refused-unknown pin moved `drone` → `travis`
  (verified unrecorded and unpinned anywhere else).
- **M2 implemented (M1 green end-to-end first), one red→green cycle.**
  The red was the missing mode (E0432 on the unresolved
  `ForgePosture`/`ReleasePass`/`release_outputs`/`release_summary`
  imports + both CLI gates on `invalid value 'release'`). The green
  landed ADR-0046. Two test-side corrections during green (the red
  phase corrected the tests, not the design): the action fixture's
  `created_tags` carried the plan's row order instead of the tag
  phase's name-sorted order (I3), and the CLI forecast fixture was
  the fully-tagged two-package shape — where the §11.1 reconciliation
  honestly forecasts zero creations; the manifest-ahead shape is the
  fixture with a would-create tag, and the fully-tagged shape became
  its own no-creation gate. The extraction also deleted the forge
  half's internal re-plan/re-reconcile (one plan, one reconciliation
  feeds every half); `halyard release` reports stayed byte-identical.
- **Tests/bench/example totals.** M1: 2 adapter contract tests + the
  widened empty-selection loop + the twelve-system lookup pin (24
  `ci_selector` tests green) + 1 CLI end-to-end gate + bench
  `ci_selector_drone_40chain` (≈82 ns) + the example row. M2: 3
  action contract tests (20 green) + 3 CLI gates (15 green, one new
  no-creation gate) + benches `release_outputs`/`release_summary_40_
  releases` (≈45 µs outputs) + example `release_forecast`. Micro-debt:
  the two panic surfaces converted (the fail-closed `E-CLI-BENCH`
  error; the typo diagnosis taking the caller's matched parts), the
  `cli_check`/`cli_bench` suites green after the refactor.
- **Docs.** ADR-0045 + ADR-0046; the research note's Drone section
  (verbatim quotes) + the header's fetch record; root README (the
  affected sentence's ADR citation now runs through 0045 — the drift
  fixed — plus the drone grammar and the release-mode status rows,
  the crate row, the example list); CLI README (the adapters section,
  the command row, the example list); arch §16.2 (the affected row's
  ADR list, the §12 release-mode as-built note, `auto` the only
  declared-absent §12 mode); PROGRESS.md row. No new error code —
  the release mode rides the existing taxonomy (`E-GIT-TAG*`,
  `E-GIT-PUSH`, `E-FORGE-*`, `E-ACTION-*`), the bench shape refusal
  rides the existing `E-CLI-BENCH`.
- **Honest state at cut-off.** The §12 modes render
  `plan|pr|check|release`; `auto` is the only declared-absent §12
  mode (its trigger-policy composition is its own ADR). The release
  mode's live path is wired through the same fail-closed orderings
  the `release` command applies and is covered by the dry gates +
  the report byte-stability pins; a real-forge live pass stays the
  nightly forge-smoke lane's probe, by design. Bench numbers are the
  sandbox's (debuginfo off, LTO off for the bench profile — disk
  pressure); the budgets stay shape-bound to the xtask fixtures, so
  no budget claim rides on them. Tree left dirty (no commits made).
- **Next session start point.** (1) the §12 `auto` mode (the
  composition policy over the two shipped executors), (2) M5.4:
  provenance/SLSA for halyard's own release artifacts (VERIFY first),
  (3) the M2.6 dhat re-audit with the cache warm.

### 2026-11-24 — The §12 `auto` mode (ADR-0047, tree left dirty, no commits)

- **Inventory at session start.** Main was green at `c286ed1` (the
  Windows path-separator fixes): fresh stable toolchain 1.100.0, the
  workspace test compile green with debuginfo off (the sandbox's disk
  pressure — the same adaptation the bench-profile note records). The
  reconciliation survey ran plan ↔ code ↔ history first: the §12
  `auto` mode is the plan's own next start point (PROGRESS's previous
  row, item 1) and the only declared-absent §12 input (`ActionMode`
  carried `plan|pr|check|release`; ADR-0046 kept `auto` absent for its
  trigger-policy ADR); the M4.2 remainder (the §11.1 step-3 content
  commit: `commit_via_api` has no caller — the adapter's chain exists,
  `MachineInput` carries no content), M5.4's own-artifact slice, M4.8's
  docs site, the M0.3 measured runs and the M2.6 dhat re-audit stay
  open (all reported, none silently re-scoped). Docs drift found: the
  arch §12 ADR-0037 as-built note still listed `auto|release|check` as
  absent — release (ADR-0046) and check (ADR-0044) had landed. Feature
  surface unchanged: `miri`/`loom` machine-enforced exclusive
  (`ci-bundle` the conflict-free gate spelling), no bench requires
  nightly, zero `#[allow]` anywhere. Micro-debt candidate found: the
  `affected` command's adapter lookup rode a production
  `unreachable!` (the string-keyed registry lookup behind a
  compile-time-closed value enum) — the class the M4.7 and drone
  sessions converted.
- **M1 implemented (no stubs), one full red→green cycle.** The red was
  the missing mode itself (E0004 non-exhaustive `ActionMode` match +
  the 4 new CLI gates refusing on the unknown `--mode auto` value,
  captured before any implementation). The green landed ADR-0047: the
  composition policy is §11.1's own rule, not a new one — one context
  → plan → manifest-vs-tags reconciliation; the release executor's
  git-side tag effects first (an empty pending an honest no-op); then
  the §11.1 machine pass with both halves' inputs, so the machine's
  own PROPAGATE rule selects the half (§11.1 step 2: pending releases
  gate the prepare phase; §11.1 step 3: nothing pending upserts or
  closes the Release PR). One plan, one reconciliation, one machine
  pass (the ADR-0046 lesson carried across the composition — the auto
  pass never re-plans between the halves and never runs the release
  executor's separate forge half, whose machine input carries no
  Release-PR spec). §11.1's `halyard release --auto` phrasing resolves
  to the §12 mode (the surface a workflow step execs); no second
  `--auto` flag forks the release command. One test-side correction
  during green: none needed — the composition landed first try; the
  green-phase refactor trimmed the lean §12 outcome to the fields the
  renderers observe (the full git-side reports are consumed on the way
  in — the dead-code deny is the discipline that caught it).
- **Tests/bench/example totals.** 4 CLI gates (`cli_action.rs`: the
  PROPAGATE forecast naming the would-create tag + the mints, the
  prepare forecast carrying the PR keys, the JSON document's
  byte-stability, the in-step env-file delivery) + the whole
  `cli_action`/`cli_affected` suites green; bench groups
  `auto_outputs_40_releases` (≈38.6 µs) + `auto_summary_40_releases`
  (same scale) — the microsecond budget every mode renderer rides;
  example `auto_forecast` (the PROPAGATE vocabulary over the preview
  writer).
- **Docs.** ADR-0047; arch §12: the ADR-0047 as-built note + the stale
  ADR-0037 note amended (the modes-absent sentence now names what
  landed); the `ActionMode`/`Command::Action` doc comments; root
  README example row; CLI README (the modes paragraph's stale
  `auto|release`-absent sentence and the command row's argv + status);
  PROGRESS.md row. No new error code — the auto mode rides the
  existing taxonomy (`E-GIT-HEAD`, `E-GIT-TAG*`, `E-GIT-PUSH`,
  `E-FORGE-*`, `E-ACTION-*`); the fail-closed refusals are the `pr`
  executor's own.
- **Micro-debt converted (additive, not a milestone substitute):** the
  `affected` adapter lookup's production `unreachable!` deleted —
  `CiSelector::adapter()` is the closed core registry as a total
  caller's match (the exhaustive match over the value enum cannot
  miss; the string-keyed `ci_selector_adapter` stays the registry's
  documented lookup). `cli_affected` green (19 gates).
- **Honest state at cut-off.** The §12 input list is complete
  (`auto|pr|release|plan|check`); the composite distribution (the
  binary download + checksum + attestation action) stays with M5.4's
  own-artifact slice; the §11.1 step-3 content commit stays the M4.2
  remainder it already is (unchanged by the mode — the composed pass
  rides the forge lifecycle the `Forge` seam carries today). Bench
  numbers are the sandbox's; the budgets stay shape-bound to the xtask
  fixtures. Tree left dirty (no commits made).
- **Next session start point.** (1) the M4.2 remainder: the §11.1
  step-3 content commit (the plan's EditSet → tree-OID compare against
  the release-branch tip → the CAS branch push; VERIFY the compare's
  git-side shape first), (2) M5.4: provenance/SLSA for halyard's own
  release artifacts (VERIFY first), (3) the M2.6 dhat re-audit with
  the cache warm.

| M7 step 2 Forgejo/Gitea adapter slice (ADR-0050) | partial | (tree left dirty; no commits) | M1: the `halyard-forge-forgejo` crate — the §11.4 `Forge` over the shared Forgejo/Gitea REST v1 surface (research note §1–§8 VERIFIED live against codeberg.org's swagger.v1.json this session; the close shape `EditPullRequestOption.state = "closed"` verified and recorded as research §8): the pull flow (find by `state=open&head=`, confirmed client-side on `head.ref`; create/rewrite; the verified close with the documented `409`→`E-FORGE-CONFLICT` mapping; the vanished-pull race as success), the releases GET-by-tag-first, the issue-comments marker scan (a PR is an issue; no pagination parameters — one walk), `Authorization: token` auth (empty token fail-closed `E-FORGE-AUTH`), `commit_via_api` kept as the documented honest decline (`E-FORGE-UNSUPPORTED` naming `ForgejoRestForge` — the contents API has no atomic multi-file commit; the lane rides `commit_mode = "git"`), the honest-zero `RateInfo` + `429`→`retry-after`, the call log; red→green on record (E0432 on the missing adapter API; then 12 contract tests + the 9-scenario shared conformance matrix green with exactly one `Declined` verdict asserted + the doctest); bench `forge_calls` + example `forge_exchange`; CLI wire-up `ForgeEnv::Forgejo` (origin grammar codeberg.org/gitea.com https/git@/ssh, `FORGEJO_API_URL` base override, `FORGEJO_TOKEN`→`GITEA_TOKEN`→`HALYARD_FORGE_TOKEN` env grammar, App-env mismatch fail-closed, the release forge half via the `ReleaseLane` resolver, doctor's token check names the lane, the foreign-origin decline names three lanes) + the `pr` loopback e2e over a codeberg.org-shaped origin (green: auth header, base derivation, the git-side content half, 3 calls) + xtask layer rule + member guard (21 crates ✓) + ADR-0050 + README/CLI-README/docs-site rows + research §8. Gates at session end: fmt ✓, clippy `--workspace --all-targets --features ci-bundle -D warnings` ✓ (zero warnings), forgejo crate tests 14/14 ✓ + doctest ✓, cli_forge green except one PRE-EXISTING hang confirmed at base by stash (`pr_live_opens_the_release_mr_over_gitlab_loopback` idles on a futex in this sandbox — documented, not silently weakened); the full-workspace test battery could not re-run to completion — the sandbox's 9.9 G disk filled twice mid-link (ld Bus error; `cargo clean` + the repo's own debuginfo-off adaptation recorded in PROGRESS did not leave room for the 20-crate debug build). M2 deferred with inventory: the remaining M7 rows (GitLab CI component, Bitbucket, corpus-lane e2e) cannot ship fully green in this sandbox (no forge tokens, no pnpm, disk ceiling); everything recorded, nothing silently re-scoped | The Bitbucket adapter (M7, VERIFY-first), the GitLab CI component, the corpus-lane e2e, M5.4 (live-CI lane), the M2.6 dhat re-audit, the corpus-wide pnpm differential, M0.3 measured runs — all unchanged; the full test/bench gate battery must re-run on a machine with disk | 2026-11-24; session log below |

### 2026-11-24 — The §12 composite distribution (ADR-0052) + the GitLab CI component (ADR-0053) (tree left dirty, no commits)

- **Inventory at session start.** Main was green at `addcd21` (the
  M7 step-3 docs): fresh stable toolchain 1.100.0, the workspace
  check compile green with debuginfo off (the sandbox's disk
  pressure — the same adaptation the bench-profile note records).
  The reconciliation survey ran plan ↔ code ↔ history first: the
  Bitbucket session's code commits landed after PROGRESS's last row
  and completed TWO recorded open items without their own row (the
  §11.1 step-3 content half — `content_commit_for_plan`/
  `live_content_commit` and the machine's `content_commit` input —
  and the M7 step-3 Bitbucket adapter itself; both stale in the
  next-start lists, both verified present in code and covered by the
  arch doc's ADR-0051 note — drift reported, not rewritten). The
  earliest still-missing seam-capable item per plan order is M5.4's
  own-artifact slice (the §12 composite distribution; `find . -name
  action.yml` empty, `.github/workflows/` carrying only ci+nightly,
  zero attestation/cosign/SBOM references; the npm-provenance half
  already shipped as ADR-0033) — the §12-auto session's own next-start
  item 2. M0.3's measured runs (networked, pending rows) and the M2.6
  dhat re-audit are not seams; `--since` has no milestone-table row;
  the M7 corpus-lane e2e is forge-token-bound. Docs drift found:
  PROGRESS has no M7 step-3 row (the Bitbucket session's); the init
  scaffold's "glue lands with its milestone" claims (doc + human
  render) were satisfied by nothing until this session. Feature
  surface unchanged: `miri`/`loom` machine-enforced exclusive, no
  bench requires nightly, zero `#[allow]` anywhere.
- **M1 implemented (no stubs), one full red→green cycle.** The red
  was the missing seam itself (E0432/E0433 on
  `halyard_action::distribution` + the file-absent pins in the CLI
  contract test, captured before any implementation; the plan line
  the first test pins is §12's "Distribution" paragraph, quoted in
  the test doc). The green landed ADR-0052: one grammar, three
  consumers — `halyard_action::distribution` (the §12 inputs with
  their surfaces, the modes, the output keys, five pinned targets,
  the asset names, the checksum manifest) is the single source the
  composite action template, the release pipeline and the drift gate
  all pin to; the templates stay hand-written YAML and the
  restricted reader refuses any shape it cannot pin (the §19
  decision-2 discipline — no YAML dependency). `E-ACTION-DISTRIBUTION`
  joins the taxonomy (page + index). The drift gate is
  `cargo run -p xtask -- check-release` (the schema-check
  discipline; xtask is the §7.1-declared owner of "release of
  halyard itself"), wired into CI as the `templates` job.
- **M2 implemented (M1 green first), one full red→green cycle.** The
  red was the gate's own refusal (`cannot read templates/halyard.yml`);
  the green landed ADR-0053: the M7 row's GitLab CI component on the
  documented component layout (VERIFIED quotes in the research
  note's GitLab section), consuming the same release distribution
  with the checksum verify, the same five-mode vocabulary, and the
  attestation verify recorded as the GitHub lane's honest decline.
- **Tests/bench/example totals.** 6 CLI contract gates
  (`cli_action_template.rs`) + 9 grammar unit tests (asset names,
  runner resolution hit/miss, manifest round-trip, the five refusal
  shapes, coverage, the §12 input-table pin, the template reader) +
  the xtask gate test; bench groups `manifest_from_files_5_targets_mib`,
  `manifest_parse_render`, `manifest_verify_one_asset_mib` (≈603 µs —
  SHA-256 throughput over 1 MiB); example `distribution_plan` (the
  resolve → manifest → verify/tamper → coverage contract, offline).
- **Docs.** ADR-0052 + ADR-0053; research note
  `own-artifact-attestation.md` (gh attestation verify, attest@v4 +
  permissions, the composite step grammar, Get-FileHash — VERIFIED
  quotes; the coreutils-manual and Expand-Archive rows recorded
  honestly as not-fetched); `E-ACTION-DISTRIBUTION` page + index
  row; arch §7.1/§12/§17 as-built amendments; README feature bullet
  + example row; CLI README (the composite action + the component
  sections); PROGRESS row. No new crate; the layer change is the
  conscious `sha2` edge on halyard-action (documented in the rule)
  and xtask's workspace-wide `*` rule already covering
  halyard-action.
- **Micro-debt converted (additive, not a milestone substitute):**
  two zero-caller `pub fn`s deleted — `halyard_core::edit::span_range`
  (the dead-code scan's only pre-existing hit; the now-unused
  `Span` import removed with it) and this session's own
  `latest_download_url` (the dead-code deny catching the grammar's
  unused half at green). Milestone-coupled drift fixed: the init
  scaffold's `halyard/action` "lands with its milestone" doc
  comment and the init human-render line now as-built (the glue
  landed).
- **Honest state at cut-off.** The distribution's live half — a real
  `v*` tag push producing attested assets and a consumer running
  `gh attestation verify` — is the recorded off-sandbox half (the
  M5.3 live-CI lane discipline); the in-tree producer/consumer/gate
  contract ships green. M5.4's cosign/SBOM rows stay open. The M7
  corpus-lane e2e stays forge-token-bound (recorded, unchanged).
  Bench numbers are the sandbox's; the budgets stay shape-bound to
  the xtask fixtures. Tree left dirty (no commits made).
- **Next session start point.** (1) the M5.4 remainder: cosign
  signing + the SBOM artifact for the release pipeline (VERIFY the
  cosign keyless grammar first), (2) the M2.6 dhat re-audit with the
  cache warm, (3) the M0.3 measured runs on a networked machine.
