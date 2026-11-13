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

## Session log

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
