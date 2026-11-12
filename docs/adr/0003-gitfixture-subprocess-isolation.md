# ADR-0003: `GitFixture` uses subprocess git with isolated environment

- **Status:** accepted
- **Date:** 2026-11-12
- **Milestone:** M0 (M0.5)
- **Deciders:** autonomous implementation agent

## Context

M0.5 requires a deterministic `GitFixture` DSL. Building repositories needs
real git semantics (OIDs, refs, commit-graph, shallow/partial clones) that a
pure-Rust reimplementation would not reproduce byte-for-byte. Invariant I6
forbids subprocesses in the *planning hot path*, and I7 forbids shell
interpolation; test infrastructure is neither.

## Decision

`halyard-test-support` shells out to the `git` binary:

- every invocation is an **argv array** through `std::process::Command`
  (never `sh -c`, never string interpolation);
- the environment is isolated per invocation: `GIT_CONFIG_NOSYSTEM=1` and a
  fixture-owned empty `GIT_CONFIG_GLOBAL`, so host git config cannot change
  behavior;
- identity and dates are pinned (`GIT_AUTHOR_*`/`GIT_COMMITTER_*` derived
  from a per-fixture sequence counter with pure integer calendar math), so
  identical scenario + content ⇒ identical OIDs;
- gpg signing is impossible (no keys are loaded, `commit.gpgsign` unset and
  tags are created unsigned);
- the DSL lives only in test-support; `halyard-git` (M2) will use `gix`
  in-process and must never call into the fixture.

## Consequences

- Fixtures are byte-reproducible across platforms and machines (asserted by
  the determinism tests for all 20 scenarios).
- Test suites require a `git` binary on PATH (present on all CI runner
  images; documented in README).
- Fixture build speed is bench-marked (`fixture_build`) because M2.2's
  10 000-sample differential walk consumes these scenarios.

## Alternatives considered

1. **`gix` in the fixture itself** — rejected for M0: fixture construction
   must mirror what end users' git produces exactly, including annotated tag
   objects and `packed-refs` layout; `libgit2`/`gix` parity gaps would
   silently skew the corpus. Revisit if fixture build time ever dominates
   test time.
2. **Pure-Rust object writer** — rejected: reimplementing git hashing and
   packing invites divergence from real repositories, which is precisely
   what the differential tests must not have.
