# HALYARD progress

Append-only record: the next agent (or human) resumes from the last row.
Status values: `not started` / `partial` / `done` / `blocked`.

| Milestone | Status | Branch | Tasks done | Tasks deferred | Notes |
| --------- | ------ | ------ | ---------- | -------------- | ----- |
| M0 | done | feat/m0-bootstrap | M0.1, M0.2, M0.4, M0.5, M0.6, M0.7 | — | 2026-11-12; see below for scope notes on M0.3 |
| M0.3 | partial | feat/m0-bootstrap | harness (`xtask bench-competitors`, tools/claims/README, REPORT.md generated) | measured runs for release-please (needs forge API + token), release-plz & sampo (not installed, argv unobserved) | 2026-11-12; cocogitto 7.0.0 + knope 0.23.2 measured on the `fd` fixture; findings in `docs/research/competitor-cli-surfaces.md` |
| M1 | not started | — | — | — | depends on M0 done; next session starts at M1.1 (core types, interner, SmallBitset) |

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
