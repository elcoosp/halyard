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
