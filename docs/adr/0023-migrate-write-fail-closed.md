# ADR-0023: the migration write step is fail-closed create-only (M4.7 step 2, §16.2)

- **Status:** accepted
- **Date:** 2026-11-15
- **Milestone:** M4.7 step 2 — `halyard migrate --write`: the validated
  draft that step 1 rides on the report lands at `halyard.toml` (the
  §16.2 write half)
- **Deciders:** autonomous implementation agent (per §0 rule 3; the
  decision rules ride ADR-0022's report-first contract)

## Context

ADR-0022 made step 1 report-first on purpose: the draft is a validated
*string* the human reviews, and the §16.2 write half was left to its own
reviewed slice. The open question for that slice was not the file I/O —
the durability protocol already exists (the edit engine's §9.1 temp
sibling → fsync → rename) — but the decision rules: which draft lands
when the report carries one, two or zero configured tools, and what
happens to an existing `halyard.toml`.

Constraints that shaped the design:

1. **What the human reviewed is exactly what lands.** The draft on the
   report is already strict-loader-valid (ADR-0022); re-serializing it
   for the write would open a drift window between the reviewed report
   and the written file. The write plan carries the report's draft
   *verbatim*.
2. **The migration never guesses.** Two configured foreign tools draft
   two different `halyard.toml` shapes (release-please pins the
   pre-major levels and the tag style; changesets pins the base branch,
   npm access and per-package skips). Picking one silently discards the
   other's mappings — exactly the "half-understood config" failure
   ADR-0022 refuses. Two candidates are a fail-closed refusal naming
   every candidate, not a pick.
3. **An existing config is never clobbered.** A repo that already has a
   `halyard.toml` is not in the migration's target state; merging a
   draft into an existing config is human work with judgment (which
   rows win?). The write is create-only: an existing target is a
   refusal (`E-CFG-MIGRATE`) naming the path and the rule, and the
   file is byte-identical after the refusal.
4. **Absence is a value (the ADR-0022 rule carries over).** `--write`
   with no foreign configuration is an honest
   `NothingConfigured` outcome — exit 0, nothing written, the report
   says so — never an error and never a synthesized empty config.
5. **The engine stays pure; the shell owns the I/O.** The seam
   (`DraftSink`: `exists` + atomic create-only `write`) is object-safe
   and injected like `TagWriter`/`PublishEffect`; the production impl
   (`AtomicDraftSink`) walks the §9.1 protocol; the engine crate ships
   the deterministic reference fake (`MemoryDraftSink`) tests drive.

## Decision

- `halyard-config::migrate` gains the write step: `DRAFT_TARGET`
  (`halyard.toml`), the `DraftSink` seam, `AtomicDraftSink` (temp
  sibling → fsync → rename, create-only, refuses a missing parent
  directory instead of mkdir-ing on the user's behalf), `MemoryDraftSink`
  (models pre-existing files, records writes in order), the pure
  `plan_draft_write(&MigrationReport)` (zero candidates → `None`, one →
  its draft verbatim, many → `E-CFG-MIGRATE` naming both) and
  `run_draft_write(plan, &dyn DraftSink)` (the exists gate reads before
  any write; the outcome is a plain-data `DraftWriteReport` with a
  snake-case `DraftWriteOutcome`).
- `halyard migrate --write` (CLI): the plan runs over the real sink
  after the survey; the JSON gains a `write` block **only when the flag
  ran** (the frozen report-only shape is untouched — byte-stable), the
  human header names what landed (path + bytes + tool) and the GHA shape
  carries a `::notice`. Refusals surface as `E-CFG-MIGRATE` errors;
  exit 0 with `NothingConfigured` when nothing is configured.
- The re-run story is the I1-shaped one: a second `--write` at a
  migrated repo refuses (the target exists), which *is* the honest
  "already migrated" signal — the report-only mode stays available for
  re-inspection.

## Consequences

- The §16.2 `migrate` row is complete (report + write); the knope /
  release-plz sources remain the open migrate items (new
  `MigrationSource` impls, no write-path changes).
- `E-CFG-MIGRATE` gains three as-built causes (existing target,
  multiple candidates, unwritable target); the docs page and the
  `ConfigError::Migrate` display carry them.
- The bench pins the write decision cost (`migrate_write` group: plan +
  memory-sink run) — the plan must stay a trivial scan over the report,
  the heavy strict-loader validation already happened in the survey.
- The write target is fixed at the repo root: the migration never
  mkdirs and never writes anywhere else; a future `--path` flag would
  be a deliberate surface change, not a default.
