# HALYARD

The fastest, most correct, most pleasant polyglot release engine in the
world. Zero-config for Cargo + pnpm + single-package repos, extensible to
every ecosystem, with a *Release PR* UX better than release-please and a
runtime measured in milliseconds.

**Status: milestone M0 (bootstrap).** The architecture and multi-year master
plan lives in [`docs/HALYARD_ARCHITECTURE.md`](docs/HALYARD_ARCHITECTURE.md);
progress is tracked in [`PROGRESS.md`](PROGRESS.md).

## What exists after M0

- **Workspace scaffold (§7.1/§7.2/§15.3)** — layered crates, pedantic+
  restriction lints, rustfmt/clippy/deny/audit configs, CI + nightly
  workflows (`.github/workflows/`).
- **`halyard-plan/v1` + `halyard.toml` v1 schemas (M0.7)** — frozen JSON
  Schemas generated from the Rust types in `halyard-core` / `halyard-config`;
  drift fails CI (`cargo run -p xtask -- schema-check`).
- **`GitFixture` DSL + 20 scenarios (M0.5)** — deterministic git repository
  builder in `halyard-test-support` (linear, merge, octopus, squash, revert,
  shallow, partial, commit-graph, packed refs, worktrees, unicode, …).
- **Synthetic repo generator (M0.4)** — `xtask gen-repo` (same seed ⇒ same
  OIDs).
- **Pinned corpus (M0.2)** — `corpus/manifest.toml` (15 repos pinned by SHA)
  + `xtask corpus fetch` (verified reproducible).
- **Competitor harness (M0.3)** — `xtask bench-competitors`; recipes contain
  only observed argv, unmeasured claims stay `pending`
  (`bench/competitors/REPORT.md`).

## Layout

```
crates/
  halyard-core           pure core types + plan contract (no I/O)
  halyard-config         halyard.toml model
  halyard-test-support   GitFixture DSL + scenarios (test infra)
  xtask                  check-layers, schema-check, perf-gate, size-gate,
                         corpus, gen-repo, bench-competitors
schemas/                 frozen JSON Schemas (M0.7)
corpus/manifest.toml     pinned real-world corpus (M0.2)
bench/                   budgets (§14.1) + competitor harness (M0.3)
docs/                    architecture, ADRs, research notes, error pages
```

## Daily commands

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings -W clippy::pedantic
cargo test --workspace --all-features
cargo test --workspace --benches
cargo run -p xtask -- check-layers     # §7.2 layer rules
cargo run -p xtask -- schema-check     # M0.7 schema drift
cargo run -p xtask -- perf-gate        # §14.1 budgets (populated from M2.6)
cargo run -p xtask -- size-gate        # §14.1 binary size (armed from M1.10)
cargo run -p halyard-test-support --example fixture_tour
```

Crate layering is enforced (`xtask check-layers`): `halyard-core` depends on
nothing but `semver`/`smallvec`/`bstr`/`blake3`/`serde` (+ the feature-gated
schema deps of ADR-0001); adapters depend on `core` only; nothing depends on
the CLI.
