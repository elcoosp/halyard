# Concepts

These pages explain how Halyard is built and what its outputs guarantee.
Each one stays short on purpose: the authoritative, full-detail documents
live in the repository (`docs/HALYARD_ARCHITECTURE.md` for the architecture
and master plan, `PROGRESS.md` for the milestone log), and the pages link to
them where a topic deepens.

1. [The layered workspace](architecture.md) — the functional-core /
   imperative-shell split, the crate layout, and the dependency rules that
   `xtask check-layers` enforces.
2. [The plan contract](plan.md) — what `halyard plan` computes, the
   `halyard-plan/v1` document, and the blake3 `plan_hash` that lets CI and
   the release step verify nothing drifted.
3. [Dry-run and preview everywhere](dry-run.md) — the preview adapters
   behind `--dry`, and why refusals fail closed instead of guessing.
4. [Channels and maintenance trains](channels.md) — prerelease channels,
   branch-bound channels, per-package overrides, and maintenance-line
   backports.
