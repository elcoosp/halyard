# ADR-0010: Performance tooling ceiling — valgrind unavailable, PGO
# prototype deferred (M2.6/M2.6b)

- **Status:** accepted
- **Date:** 2026-11-13
- **Milestone:** M2.6 / M2.6b
- **Deciders:** M2.6 perf-pass session (sandbox-constrained)

## Context

§14.1 fixes the scenario budgets and §14.3 names the profiling toolchain:
`hyperfine`, `divan`/`criterion`, **`iai-callgrind`** (deterministic
instruction counts for CI gating), `samply`/`perf`, **`dhat`** (zero
allocations per commit in the painter walk is a tracked metric) and
`strace -c`. The architecture doc also lists **PGO + BOLT in the release
pipeline** (§14.2, "PGO prototype" under M2.6/§17).

The M2.6 execution environment is a restricted Linux sandbox:

1. **No valgrind.** `valgrind` is not installed and cannot be installed
   here (no root package-manager access). Without valgrind there is no
   callgrind, therefore **`iai-callgrind` (deterministic
   instruction-count gating) is impossible in this environment** — the
   §14.3 "used for CI gating since wall-time is noisy" mechanism is
   unavailable for M2.6 evidence.
2. **PGO status.** `rustup component add llvm-tools-preview` **succeeds**
   in this sandbox and the toolchain ships `llvm-profdata` under
   `<sysroot>/lib/rustlib/x86_64-unknown-linux-gnu/bin/`, so a
   generate→use walkthrough is *implementable*. The prototype command
   `cargo run -p xtask -- pgo` implements the full walkthrough (build
   with `-Cprofile-generate`, run the plan scenario on a `genrepo`
   fixture, `llvm-profdata merge`, rebuild with `-Cprofile-use`). The
   full release-profile cycle was **not executed within this session's
   time budget** (two release builds + profile run); M2.6b (PGO) is
   therefore **deferred**, with the walkthrough command available and the
   tooling verified present.

## Decision

1. **Accepted M2.6 evidence** in this environment, until a valgrind- or
   CI-integrated toolchain is available:
   - the criterion benches already under `crates/*/benches/`
     (micro-benchmarks),
   - the `dhat` allocation audit
     (`crates/halyard-git/tests/dhat_walk.rs`: audited per-popped-commit
     allocation ceilings — ≤ 14 without a commit-graph, ≤ 8 with one —
     asserted so regressions fail the suite),
   - the wall-clock harness `cargo run -p xtask -- bench-public` (30
     in-process runs per §14.1 scenario, p50/p99, VmHWM peak RSS,
     deterministic TOML output gated by `xtask perf-gate`).
2. **iai-callgrind instruction-count gating is deferred** until a
   valgrind-capable runner exists; `perf-gate` wall-clock gating
   (§15.6, p50 > 5 % regression fails) is the interim CI gate.
3. **M2.6b (PGO) is deferred**: `xtask pgo` exists as the walkthrough
   prototype and verifies `llvm-tools-preview`/`llvm-profdata`
   availability; wiring PGO into the actual release pipeline (and BOLT)
   stays with M6.x release engineering.
4. Budget honesty: `bench/budgets.toml` keeps the fixed §14.1 values; a
   scenario measured at a larger shape than §14.1 (the 1000-package /
   50 000-commit row generated as 1000 packages × 2000 commits this
   session) is recorded as a **scaled variant** in
   `bench/measurements.toml` with the actual shape stated, never by
   loosening the budget.

## Consequences

- Easier: M2.6 evidence is fully reproducible offline (`bench-public`,
  `perf-gate`, `dhat_walk`, criterion benches); no sandbox-specific CI
  dependencies.
- Harder: wall-clock numbers are noisier than instruction counts; the
  5 %-regression gate may need re-run averaging before failing a PR.
- Revisit when: a valgrind-capable CI runner exists (revisit decision 2)
  or the M6 release pipeline lands (revisit decision 3); both only change
  *evidence*, not the §14.1 budget values.

## Alternatives considered

1. **Ship wall-time-only gating without documenting the ceiling** — lost:
   §14.3 explicitly prescribes deterministic instruction counts; hiding
   their absence would misrepresent M2.6 evidence.
2. **Loosen the 1000-package §14.1 budget to fit the scaled variant** —
   lost: §14.1 numbers are the contract; an honest "scaled variant" note
   preserves comparability while keeping the target fixed.
3. **Emulate callgrind with `perf stat` instruction counts** — lost:
   `perf` hardware counters are unavailable to unprivileged sandbox
   processes here, and counts would still be hardware-dependent, not
   deterministic.
