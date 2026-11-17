# ADR-0042: the `halyard bench` measurement command

- **Status:** accepted
- **Date:** 2026-11-17
- **Milestone:** §16.2's declared-absent command — "run built-in
  micro/macro benchmarks on the current repo"; lands as the
  user-facing measurement surface beside the maintainer-side xtask
  harness
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

§16.2 reserved a `bench` command ("run built-in micro/macro benchmarks
on the current repo") that stayed absent rather than stubbed through
every slice so far. The measurement machinery that exists is
maintainer-side: the xtask harness measures the synthetic fixtures of
`bench/budgets.toml` (the 40-package chain, the 200×40 plan shape) and
`xtask perf-gate` enforces the §14.1 budget table in CI. What does not
exist is the *user-facing* surface: a developer with a real repo has no
command that answers "what does halyard's planning pipeline cost on
**this** workspace?" — the question the §14 model rows (warm/cold
no-op, plan with N commits over M crates) are shaped after.

## Decision

- **The command times the real pipeline, not a fixture** — one pass
  runs exactly what every planning command runs: the shared `Context`
  pipeline (`discover` → `config` → `history`) plus the pure planner
  (`plan`), over the current repo's real discovery, real git history
  and real config. The measured shape is reported beside the timings
  (packages, pending commits, cache posture), so a number is never
  separated from the shape that produced it.
- **One untimed warmup precedes the timed passes** — the §14.1
  no-op-warm model: the warmup pass fills the page cache and the §8.7
  per-commit cache, and the timed passes measure the warmed steady
  state. `--no-cache` turns the run into the cold measurement; the
  report names the posture either way, so the two runs are never
  compared blind.
- **The stage vocabulary rides the `--profile` trace** — each pass is
  timed through the same span names the Chrome trace emits
  (`discover`, `config`, `history`, `plan`), via a new read-only
  `Profiler::recorded_spans()` accessor; a bench row and a trace span
  with the same name measure the same phase. The untimed warmup is a
  disabled profiler, which records nothing by construction.
- **The reduction is pure, on the lib target** — grouping the samples
  per stage and the nearest-rank percentiles (the `ceil(p/100 × n)`
  definition the §14 budget table speaks) live in
  `halyard_cli::bench` as plain-data functions over plain-data samples,
  unit-tested without a repo, and benched in isolation
  (`bench_report`: the aggregation over the sample counts a real pass
  loop produces). The lib-target placement is the man renderer's
  rationale: the bench harness and the bin share one implementation.
- **A measurement, never a gate** — the §14.1 budgets in
  `bench/budgets.toml` are shape-bound to the synthetic fixtures the
  xtask harness measures; a repo-shaped bench borrows none of them.
  The human/gha renderings say so explicitly ("measurement, not a
  gate"). Budget enforcement stays `xtask perf-gate`'s job.
- **Fail-closed on a basis-less request**: `--iterations 0` refuses
  with the new `E-CLI-BENCH` before the warmup runs (a measurement
  needs at least one timed pass; the default is 8).
- **All three formats**, per the §16.3 rendering rule: the human
  table, the stable JSON document (the timings are wall-clock — the
  schema, the name-sorted stage order and the shape are the run-stable
  contract), and the GHA job-summary table.
- **Alternatives considered and rejected**: running the criterion
  suites from the CLI — criterion is the maintainer's dev harness and
  its fixtures are synthetic, not the user's repo; printing the
  §14.1 budget table next to repo timings — a repo whose shape matches
  no fixture would read the comparison as a verdict it cannot carry;
  a `--json`-only surface — the §16.3 principle renders all three
  formats for every report.

## Consequences

- `halyard bench [--iterations N]` is the §16.2 row's as-built form:
  built-in, over the current repo, the pipeline stages as the micro
  measurements and the whole pass as the macro one.
- The error taxonomy grows by exactly one code (`E-CLI-BENCH`), page +
  index row on record (the bidirectional-exactness invariant holds).
- The man pages and completions gain the command automatically (the
  clap-derivation rule); the man bench's mirror tree carries the real
  command count (20 — the mirror had also drifted by one, missing
  `action`, repaired here).
- `Profiler::recorded_spans()` is the profiler's second consumer: the
  trace stays the `--profile` surface, the spans become the bench's
  raw material. Nothing writes a trace from `bench` (the global
  `--profile` behavior is unchanged).
