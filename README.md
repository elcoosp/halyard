# HALYARD

The fastest, most correct, most pleasant polyglot release engine in the
world. Zero-config for Cargo + pnpm + single-package repos, extensible to
every ecosystem, with a *Release PR* UX better than release-please and a
runtime measured in milliseconds.

**Status: milestone M1 in progress — foundation slice landed (M1.1–M1.4,
M1.8).** The architecture and multi-year master plan lives in
[`docs/HALYARD_ARCHITECTURE.md`](docs/HALYARD_ARCHITECTURE.md); progress is
tracked in [`PROGRESS.md`](PROGRESS.md).

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

## What exists after the M1 foundation slice

- **Core model (M1.1)** — `halyard-core`: `PkgId`/`Sym`/`Interner`,
  `SmallBitset` (inline ≤ 128 packages, heap spill), `Workspace` /
  `Package` / `DepEdge` / groups, `TagTemplate` (appendix A.3),
  `ReasonChain`, `RelevantCommit` / `HistoryDigest`, canonical JSON +
  `plan_hash` (blake3, ADR-0005); proptest on the bitset, snapshot-pinned
  plan hash.
- **Conventional-commit parser (M1.2)** — `halyard-conventional`:
  spec-compliant (1.0.0) grammar over bytes via `winnow`, scopes with
  escapes, `BREAKING CHANGE`/`BREAKING-CHANGE` footers, git trailers,
  `(#NNN)` PR extraction, `fixup!`/`squash!` detection; golden tests on the
  spec examples, totality proptest, fuzz targets under `fuzz/`.
- **Version algebra (M1.3)** — `halyard-version`: bump application with the
  Cargo-compatible 0.x policy, commit-type mapping, prerelease channels
  (counter from existing tags), and the appendix-A.2 requirement rewriter
  (operator + precision preserving, widen policy, `E-EDIT-UNREWRITABLE`).
- **Edit engine (M1.4)** — `halyard-edit`: `EditSet` byte-range patches,
  atomic apply (temp → fsync → rename, Windows retry), verify-before-write,
  CRLF/BOM preservation (invariant I4), roundtrip proptests.
- **Config loader (M1.8)** — `halyard-config`: strict unknown-key detection
  with did-you-mean, layered resolution (defaults < file < package metadata
  < env `HALYARD_*` < CLI), `config_hash` over canonical JSON; snapshot-
  tested diagnostics.

## Layout

```
crates/
  halyard-core           pure core types + plan contract + plan hash (no I/O)
  halyard-config         halyard.toml model + loader + layered resolution
  halyard-conventional   conventional-commit parser (winnow, zero-copy scan)
  halyard-edit           EditSet byte patches + atomic apply
  halyard-test-support   GitFixture DSL + scenarios (test infra)
  halyard-version        bump algebra + 0.x policy + requirement rewriter
  xtask                  check-layers, schema-check, perf-gate, size-gate,
                         corpus, gen-repo, bench-competitors
schemas/                 frozen JSON Schemas (M0.7)
corpus/manifest.toml     pinned real-world corpus (M0.2)
bench/                   budgets (§14.1) + competitor harness (M0.3)
fuzz/                    cargo-fuzz targets (conventional parser, tag templates)
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
cargo run -p halyard-core --example workspace_demo
cargo run -p halyard-conventional --example parse_commit
cargo run -p halyard-version --example bump_algebra
cargo run -p halyard-edit --example apply_patch
cargo run -p halyard-config --example resolve_layers
```

Crate layering is enforced (`xtask check-layers`): `halyard-core` depends on
nothing but `semver`/`smallvec`/`bstr`/`blake3`/`serde`/`serde_json`
(ADR-0005) plus the feature-gated schema dep of ADR-0001; the parser and
algebra crates take `core` at most; adapters depend on `core` only; nothing
depends on the CLI.
