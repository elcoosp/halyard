# The layered workspace

Halyard is a layered Rust workspace organized around one pattern: a
**functional core** (pure computation, no I/O, deterministic — byte-stable
output for the same inputs) and an **imperative shell** (the CLI, the git
write paths, the network adapters) that owns every effect. The layering is
not aspirational: `cargo run -p xtask -- check-layers` parses every crate's
manifest and enforces the dependency rules on every run.

## The crates

| Crate | Role |
|---|---|
| `halyard-core` | pure core types, plan contract, plan hash (no I/O) |
| `halyard-config` | `halyard.toml` loader + layered resolution + foreign-config migration |
| `halyard-conventional` | conventional-commit parser (winnow, zero-copy) |
| `halyard-version` | bump algebra, 0.x policy, requirement rewriter |
| `halyard-edit` | byte-range patches, atomic apply, verify-before-write |
| `halyard-discover` | topology + manifests → workspace |
| `halyard-eco-cargo` | Cargo adapter: discovery, manifest + lockfile patchers |
| `halyard-eco-npm` | npm/pnpm adapter: discovery, JSON edit, pnpm-lock patcher |
| `halyard-git` | gix engine: refs, history walk, tag phase, push, smart fetch |
| `halyard-cache` | persistent mmap per-commit cache |
| `halyard-changelog` | changelog styles, Release PR body, change-file grammar |
| `halyard-forge` | `Forge` seam + `MemoryForge` + `PreviewForge` + release state machine |
| `halyard-forge-github` | GitHub REST adapter + App-JWT auth |
| `halyard-registry` | crates.io sparse index + npm packument probes |
| `halyard-publish` | publish engine: plan validation, topological layers, resume, auth |
| `halyard-action` | GitHub Actions outputs over `GITHUB_OUTPUT`/`GITHUB_STEP_SUMMARY` |
| `halyard-cli` | the `halyard` shell |
| `halyard-test-support` | fixtures: the declarative git-repo DSL |
| `xtask` | workspace tooling and gates |

## The seams

The adapters above the core are driven through narrow, object-safe traits, so
the real network adapters and the preview adapters are interchangeable:

- `Forge` — create/upsert the Release PR, create releases, API commits,
  comments; implemented by the GitHub REST adapter, the deterministic
  `MemoryForge` reference, and the `PreviewForge` dry-run forecast.
- `Registry` — existence probes against the crates.io sparse index and npm
  packuments, with the HTTP transport injected (no client lives in the crate).
- `PublishEffect` — the spawned publish tools, injected so the engine is
  testable without ever shelling out in tests.
- `PublishAuth` — credential resolution with a scrubbing contract; the env
  grammar and the OIDC exchange are two implementations of the same seam.
- `ActionWriter` — GitHub Actions outputs and job summaries; the real writer
  rides the runner's env files, the preview writer records a call log.
- `TagWriter` / `TagPusher` — the git-side tag and push effects, with
  preview counterparts that classify exactly as the live run would.

Ecosystem adapters implement a shared `EcosystemAdapter` trait (discovery,
version edits, lockfile patching), which is what lets one plan span both
Cargo and npm packages.

## Enforcement and hygiene

The workspace denies panics outside tests (`unwrap`, `expect`, `panic!`,
indexing), forbids unsafe code except the cache crate, requires every public
item to be documented, and keeps the `miri`/`loom` feature pair of the cache
crate machine-enforced exclusive (a combined build is a `compile_error!`).
The full invariant list, the data model, and the performance budgets live in
the architecture document in the repository:
[docs/HALYARD_ARCHITECTURE.md](../../../../docs/HALYARD_ARCHITECTURE.md).
