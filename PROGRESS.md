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

## Session log

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
