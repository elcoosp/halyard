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

## Session log

### 2026-11-12 — M0 bootstrap (branch `feat/m0-bootstrap`, base 54fa712c0666dc94f06db8bc1093003612278fcb)

- **M0.1 done.** Workspace (edition 2024, MSRV 1.88), `[workspace.lints]`
  (pedantic + restriction; pedantic enforced via the lints table, see the
  clippy gate note in the PR body), `rustfmt.toml`, `clippy.toml`,
  `deny.toml`, `.cargo/audit.toml`, `xtask` with working `check-layers`,
  `schema-check`, `perf-gate`, `size-gate`, ADR template, `.github/workflows/ci.yml`
  (fmt, clippy, test×3 OS, nextest, deny, audit, doc, schema-drift, perf-gate,
  size-gate, lint-hygiene) and `.github/workflows/nightly.yml` (fuzz, Miri,
  corpus replay, differential, forge smoke, mutation — armed behind
  `hashFiles` guards per ADR-0002).
- **M0.5 done.** `GitFixture` DSL in `halyard-test-support`: deterministic
  dates/identity/env isolation (ADR-0003), 20 scenarios
  (`crates/halyard-test-support/src/scenarios.rs`) incl. shallow + partial
  clones; 21 integration tests assert shape and build determinism (same
  scenario twice ⇒ same OIDs); criterion bench `fixture_build`;
  runnable example `fixture_tour`.
- **M0.7 done.** `halyard-plan/v1` (`halyard-core::plan`) and `halyard.toml`
  v1 (`halyard-config::model`) types → frozen draft-2020-12 schemas checked
  in under `schemas/`; drift test = `xtask schema-check` (+ in-crate test);
  CI job `schema-drift`. Optional deps documented in ADR-0001.
- **M0.4 done.** `xtask gen-repo` (shapes: linear/squash/merge/octopus,
  splitmix64 PRNG): same seed+params ⇒ identical HEAD (asserted in tests and
  demonstrated: seed 42 ⇒ `89889c7c…` twice).
- **M0.2 done.** `corpus/manifest.toml` pins 15 real repos by `ls-remote`
  HEAD SHA (2026-11-12, ADR-0004); `xtask corpus fetch` verifies HEAD == pin;
  smoke-tested shallow fetch of `fd` (`24d90b28d254`), idempotent re-fetch.
- **M0.6 done.** Six research notes under `docs/research/` (crates.io
  publishing, npm provenance/trusted publishing, shallow-exclude + partial
  clone matrix, Cargo.lock grammar, pnpm lockfile grammar, competitor CLI
  surfaces), each with live-source URLs or verbatim local observations and
  explicit open VERIFY items.
- **M0.3 partial.** Harness shipped and exercised (2 measured runs); the
  remaining tool runs need a GitHub token / tool installs — tracked in the
  table above and `bench/competitors/REPORT.md` (statuses honest: measured /
  pending).
- **Gates at session end:** fmt ✓, clippy (`-D warnings`, pedantic+restriction
  via `[workspace.lints]`) ✓, `test --workspace --all-features` ✓ (37 tests),
  `test --benches` ✓, `cargo deny check` ✓, `cargo audit` ✓, `xtask
  check-layers` ✓, `xtask schema-check` ✓, lint-suppression grep ✓, doc
  (`RUSTDOCFLAGS=-D warnings`) ✓.
- **Deviations requiring review:** ADR-0001 (optional schema-gen deps in
  core), ADR-0002 (crates enter per milestone), pedantic curation
  (`must_use_candidate`, `missing_errors_doc` allowed with rationale in
  `[workspace.lints.clippy]`; `-W clippy::pedantic` moved off the CLI into
  the lints table so the allow-list is effective), deny `wildcards = warn`
  (workspace-inheritance false positive, commented in deny.toml).

### 2026-11-12 — M1 slice A: foundation (branch `feat/m1-foundation`, base a196b083615702a713984d07c200da5e95654320)

- **Scope.** Slice A of the M1 protocol (M1.1, M1.2, M1.3, M1.4, M1.8) —
  the self-contained foundation: core model, conventional-commit parser,
  version algebra, edit engine, config loader. Tree compiles, all gates
  green, no stubs/`todo!()`/`#[allow]`. M2 not started (per protocol: only
  after M1 is fully green end-to-end).
- **M1.1 done.** `halyard-core` gains `id` (PkgId/Sym/Interner), `bitset`
  (SmallBitset, inline 128 + spill, logical equality), `package`
  (Ecosystem/RelPath/DepEdge/ReqSpec/ManifestSite/TagTemplate/PathFilter/
  Group/Workspace), `reason` (Oid/Reason/ReasonChain/VersionResolution),
  `history` (RelevantCommit/HistoryDigest), `canonical` (canonical JSON +
  blake3 `plan_hash`). Proptest on the bitset (model = BTreeSet), insta
  snapshot on the plan's canonical JSON, determinism double-runs.
  ADR-0005: `serde_json` joins core's runtime graph for the plan hash;
  `schemars` stays feature-gated.
- **M1.2 done.** `halyard-conventional`: winnow-based subject grammar +
  hand-scanned scopes/trailers; spec 1.0.0 examples as goldens; totality
  proptest (never panics on arbitrary bytes); `(#NNN)` byte-scan extractor
  (§8.5); `fixup!`/`squash!` detection; insta snapshot of parsed output.
  Differential vs `conventional-commits-parser` (99.99% over 1M subjects)
  deferred with the nightly differential lane — needs the npm corpus tool.
- **M1.3 done.** `halyard-version`: `apply_bump` with Cargo-compatible 0.x
  policy, `CommitBumpMapping`, prerelease channels (counter derived from
  existing versions), and the appendix-A.2 rewriter (operator + precision
  preserving; widen policy; `E-EDIT-UNREWRITABLE` for closed bounds and
  unparseable reqs). Goldens = every A.2 row + operator/precision variants;
  proptests: rewritten reqs admit the new version, operator style preserved,
  bump monotone.
- **M1.4 done.** `halyard-edit`: `Patch`/`EditSet` (sorted, non-overlapping,
  validated), `apply` (ascending rewrite; untouched bytes verbatim — I4),
  `detect_line_endings`, atomic `apply_to_file[_verified]`/`atomic_write`
  (temp sibling → fsync → rename; Windows `ERROR_ACCESS_DENIED` retry;
  POSIX mode preservation). Proptests: byte-range definition, batched ==
  sequential (length-preserving), identity noop; atomic tests cover
  verify-failure-leaves-file-untouched, CRLF/BOM on disk, mode survival.
  Criterion benches: bitset ops (core), conventional parse corpus, edit
  apply (single + 40 files).
- **M1.8 done.** `halyard-config` loader + resolver: strict unknown-key
  walk over the frozen v1 key table with Levenshtein did-you-mean (≤ 2,
  lexicographic tie-break), layered resolution (Defaults < File <
  PackageMetadata < Env < Cli), documented `HALYARD_*` set
  (`BASE_BRANCH`/`TAG_STYLE`/`CASCADE`/`POLICY`), `config_hash` = blake3
  over canonical JSON. Diagnostics snapshot-tested (insta); loader tests
  deterministic.
- **Docs.** ADR-0005; 8 new error pages (E-CFG-UNKNOWN-KEY, E-CFG-TAG-STYLE,
  E-CFG-TOML, E-CFG-ENV, E-EDIT-PATCHES, E-EDIT-VERIFY, E-EDIT-IO,
  E-EDIT-UNREWRITABLE) + index update; README layout/commands refresh;
  `schemas/halyard-plan-v1.json` regenerated (description text only — the
  plan.rs doc pointer to `plan_hash` changed; wire format untouched).
- **Fuzz.** `fuzz/` cargo-fuzz crate with two targets (`parse_conventional`,
  `tag_template`); nightly CI lane arms itself via the existing
  `hashFiles('fuzz/**')` guard. Local short runs recorded in the PR body.
- **Deferred (explicit).** M1.5/M1.6 (eco adapters + differential vs
  `cargo metadata` / `pnpm m ls --json`), M1.7 (CSR/topo/SCC + polyglot
  links), M1.9 (planner + naive gix history provider), M1.10 (CLI
  skeleton). Suggested next session: slice B (M1.5, M1.6, M1.7) then
  slice C (M1.9, M1.10) — same protocol, slices per TASK §2.2.
- **Gates at session end:** fmt ✓, clippy (`-D warnings`, pedantic +
  restriction via the lints table) ✓, lint-suppression grep ✓ (zero hits),
  `test --workspace --all-features` ✓ all suites green (≈130 unit/property/
  snapshot tests; 167 green result lines incl. bench-mode runs),
  `test --benches` ✓, deny ✓, audit ✓, `check-layers` (7 crates) ✓,
  `schema-check` ✓, `perf-gate` ✓ (vacuous until M2.6), `size-gate` ✓
  (vacuous until M1.10), `cargo doc -D warnings` ✓.

### 2026-11-12 — M1 slice C: planner + git engine + CLI (branch `feat/m1-planner-m2-history`, base fcc622d642be8507a5f46295e8fcee273aad88b1)

- **M1.9 (planner v1) implemented.** `halyard-core::planner` — pure
  `plan(&Snapshot, &PlanConfig) -> Result<Plan, PlanError>` (ADR-0007:
  `PlanConfig` in core, `ResolvedConfig::plan_config()` maps it). Direct
  bumps + `Release-As` trailers + group/lockstep normalization + cascade
  fixpoint (bounded by |V|, monotone) + appendix-A.2 requirement rewrites
  + edits from the new `version_site`/`raw` discovery fields + changelog
  previews + plan_hash. Goldens: 5 insta snapshots under
  `crates/halyard-core/tests/snapshots/`; proptest
  (`planner_props.rs`, 64 cases) covers version monotonicity, tag
  round-trip, hash determinism, cycle refusal.
- **M2.1 + M2.2 (git engine) implemented.** New `crates/halyard-git`
  (gix 0.90, `default-features = false, features = ["sha1"]`;
  `max-performance-safe` no longer exists — drift noted). Refs
  enumeration + tag-template matching + highest-version-reachable base
  resolution with O(1) generation rejection; painter walk per §8.3 with
  exact exhaustiveness-based termination, degraded marker, initial bound.
  Differential: `tests/engine.rs` (9/9 green — bases vs
  `git merge-base --is-ancestor`, walk vs per-package `git rev-list HEAD
  ^base_i` union over 10 fixture repos incl. merge/octopus/squash/revert/
  packed-refs/annotated/side-branch-tag/multi-package/scoped-npm) +
  `tests/randomized.rs` (60 pseudo-random DAG repos, all matching).
  dhat audit: 12 blocks/commit (no commit-graph), 5/commit (commit-graph),
  ceilings pinned in `tests/dhat_walk.rs`; M2.6 owns the drive to ≤ 1.
  Bench `benches/walk.rs` (1000-commit synthetic), example
  `examples/walk_history.rs`.
- **M1.10 (CLI) implemented, NOT verified end-to-end.** New
  `crates/halyard-cli` (`halyard` binary, `#![forbid(unsafe_code)]`):
  plan/explain/status/graph/doctor/schema/completions, `--format
  human|json|gha`, `--color/-C/--config/--offline/--no-cache/--profile`,
  miette-rendered errors, Chrome-trace profiler. Compiles clean; the
  insta snapshot battery, `doctor` case snapshots, the profile-trace
  parse test and the strace purity gate did NOT run in-session.
- **M3.5 (stretch, partial).** `crates/halyard-changelog`:
  keep-a-changelog renderer over the Plan, idempotent (re-run replaces
  only `[Unreleased]`/marker span, byte-identical on re-run), breaking
  first, hidden types honoured, `E-CHLOG-*` codes. Other styles deferred
  (M3.5b).
- **ADRs:** 0007 (PlanConfig in core), 0008 (bump/rewrite algebra
  canonical home moves to core; halyard-version re-exports unchanged),
  0009 (halyard-git depends on halyard-conventional — leaf, cycle-free).
- **Slice-B latent bug fixed:** `RelPath::new("")` rejected the
  documented repo-root empty path (its own test asserted `is_some`);
  discovery of root packages was broken on main. Fixed in
  `crates/halyard-core/src/package.rs`.
- **Deferred to slice D (next session):** full gate battery
  (fmt/clippy/test/benches/deny/audit/doc/schema-check/discover-diff),
  new xtask subcommands (`schema-check-plan`, `git-diff` — planned, not
  written), CLI snapshot tests + strace gate, `docs/errors/E-PLAN-*` +
  `E-GIT-*` + `E-CHLOG-*` pages + index update, 10 000-sample
  `git describe`/rev-list campaign on the corpus, M2.3 tree diff,
  M2.4 merge policy, M2.5 cache, M2.6 perf pass, M2.7 replay.
