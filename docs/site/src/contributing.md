# Contributing

Halyard is developed as a single Cargo workspace with a gate battery that
runs locally exactly as it runs in CI.

## Development workflow

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --features ci-bundle -- -D warnings
cargo test --workspace --features ci-bundle
cargo test --workspace --benches
cargo run -p xtask -- check-layers      # crate dependency rules
cargo run -p xtask -- schema-check      # frozen JSON schemas
cargo run -p xtask -- check-docs-site   # this book's structure and links
```

The clippy gate is pedantic with the restriction set denied workspace-wide
(`unwrap`, `expect`, `panic`, indexing) outside tests; lint suppressions are
forbidden outright. The `ci-bundle` feature is the conflict-free spelling of
"all stable features": the cache crate's `miri`/`loom` pair is
machine-enforced exclusive, so `--all-features` is not a valid gate
invocation by design.

## The decision and milestone protocol

- Architectural decisions are recorded as ADRs in the repository's
  `docs/adr/` directory, starting from the
  [template](../../../docs/adr/0000-adr-template.md). Any change to an
  invariant, a schema, or a budget requires one.
- The architecture document —
  [docs/HALYARD_ARCHITECTURE.md](../../../docs/HALYARD_ARCHITECTURE.md) —
  is the master plan: the data model, the subsystem designs, the milestone
  breakdown, and the error taxonomy.
- [PROGRESS.md](../../../PROGRESS.md) is the append-only milestone log: the
  next session resumes from its last row.
- Error codes need a page: a new `E-*` variant ships with its
  `docs/errors/E-*.md` page and an index row, and this book's
  [error table](reference/errors.md) must stay in set-equality with that
  index (the `check-docs-site` gate enforces it).

## This documentation site

The site is an mdBook tree at `docs/site` (ADR-0048):

- render it with `mdbook build docs/site`;
- the structural and link gate is `cargo run -p xtask -- check-docs-site` —
  it validates the summary, page reachability, relative links and anchors,
  include directives, and the error-table mirror, and it runs in CI as its
  own job;
- pages are hand-authored summaries of the canonical in-repository
  documents; when a canonical document changes, the corresponding page is
  updated in the same change (the error table is generated from the index
  for exactly this reason).

## CI

The CI workflow in the repository (`.github/workflows/ci.yml`) runs, among
the rest of the battery: fmt, the pedantic clippy gate, the test matrix
across Linux/macOS/Windows, the loom lane for the cache crate, cargo-deny
and cargo-audit, the rustdoc gate with warnings denied, the schema-drift and
perf/size gates, the lint-hygiene grep, and the docs-site link gate.
