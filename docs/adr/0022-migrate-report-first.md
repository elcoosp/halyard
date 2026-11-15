# ADR-0022: the migration report is report-first (M4.7 step 1, §16.2)

- **Status:** accepted
- **Date:** 2026-11-15
- **Milestone:** M4.7 step 1 — `halyard migrate` (the §16.2 command
  row): the release-please and changesets configurations are surveyed
  against the `halyard.toml` v1 surface — mapped rows, the semantic
  differences, and a validated draft `halyard.toml` riding the report
- **Deciders:** autonomous implementation agent (per §0 rule 3; the
  foreign grammars ride the VERIFIED quotes recorded in
  `docs/research/migrate-config-grammars.md`)

## Context

The roadmap's M4.7 acceptance ("read configs, emit report") and the
§16.2 row ("import from release-please/changesets/knope/release-plz
configs, write `halyard.toml` + report of semantic differences") name
the same command in two halves: read+report, then write. Every session
since the plan was written deferred both halves. The migration is the
last §16.2 command absent from the wired set that needs no network and
no new dependency — the natural next slice after the ADR-0021 routing
session.

Constraints that shaped the design:

1. **The report precedes the write.** A migration that silently
   rewrote `halyard.toml` would be the one command in halyard that
   edits a file the user has not asked to change; the §16.2 write step
   is its own slice behind its own review. Step 1 emits the draft as a
   report *string* — validated through the strict loader
   (`load_from_str`) before it is surfaced, so the draft is exactly
   what `halyard.toml` v1 accepts — and writes nothing.
2. **Absence is a value.** `halyard migrate` runs on any directory —
   including one with no foreign tool and no `halyard.toml` yet (that
   is the migration's target state) — so a missing foreign config is
   `configured: false` in the report, never an error. A configured
   file that does not parse fails closed (`E-CFG-MIGRATE`, new
   `ConfigError::Migrate` variant + page): a half-understood foreign
   config is never mapped, because a mis-mapped tag template or
   pre-major pin would silently change what releases.
3. **Preserve the foreign tool's *effective* behavior, defaults
   included.** release-please's documented pre-1.0 defaults
   (`bump-minor-pre-major: false`, `bump-patch-for-minor-pre-major:
   false`) differ from halyard's §10.2 defaults (breaking → minor,
   feat → patch). The survey pins `versioning.pre_major` explicitly in
   both directions and the row notes say which side is the foreign
   default — a migrated repo behaves the way the old tool behaved,
   which is the only non-surprising migration.
4. **Semantic differences are rows, not errors.** The v1 surface
   cannot express release-please's `changelog-path`, `plugins`,
   `prerelease`, `extra-files`, or changesets' per-subset
   `linked`/`fixed` groups. Dropping them silently would strand the
   user's setup; refusing the whole migration would strand the user's
   migration. Each such key is an `unmapped` row with a concrete
   reason and, where one exists, the closest v1 pointer (e.g.
   `exclude-paths` → review `[[package]].exclude`).
5. **The seam is the extension point, not a closed enum.**
   `MigrationSource` (tool name + declared config paths + survey over
   the discovered files) has one impl per shipped tool; knope /
   release-plz land as new impls (§16.2 names them for later slices).
   The shell does the I/O — the engine stays pure (functional core /
   imperative shell, §4) and `migrate(&files, sources)` runs any slice
   of sources, which is how tests exercise custom sources.

## Decision

- `halyard-config::migrate` (the config crate already owns the v1
  model the draft is built from; no new dependency, no new crate):
  the `MigrationSource` seam, the `ForeignFiles` input (a sorted
  path→content map), the plain-data report types (`MigrationReport` /
  `ToolSurvey` / `MappingRow` / `UnmappedRow` / `PackageSurvey` —
  `Serialize` for the CLI's stable JSON), and `default_sources()`.
- release-please maps: the pre-major switches (pinned, defaults
  noted), the tag switches (`include-component-in-tag` /
  `include-v-in-tag` / `tag-separator` → one `repo.tag_style`
  validated through the core's `TagTemplate` grammar, per-package
  disagreement reported), per-package `package-name`/`component` and
  the manifest's last-release versions as survey notes. changesets
  maps: `access` → `publish.npm.access`, `baseBranch` →
  `repo.base_branch`, `ignore` → `[[package]] release = false`
  overrides, `updateInternalDependencies` → `versioning.cascade`
  ("always", the closest v1 shape, noted). Everything else is an
  unmapped row with its reason.
- `halyard migrate` (CLI): reads each source's declared paths in the
  working directory (missing = absence, unreadable = fail-closed),
  runs the survey, renders human/json/gha. The JSON document is the
  engine type's own serialization (the engine type is the single
  source of the field names). Exit 0 with the report even when rows
  report differences — the report is data, the review is the human's.
- The changesets git-tag grammar was **not** verifiable (the docs page
  quoting the `name@version` shape no longer exists at `main` — see
  the research note); the survey carries an honest advisory row for
  tag naming and maps no changesets-derived template.

## Consequences

- The wired command set gains `migrate` (report-only; the §16.2 write
  step and the knope/release-plz sources are the next migrate slices).
- New error page `E-CFG-MIGRATE`; the `ConfigError` taxonomy grows one
  variant (additive).
- The benches pin the survey cost (release-please 4-package survey ≈
  7 µs, changesets ≈ 9 µs, empty tree ≈ 100 ns) — the strict-loader
  validation of the draft is part of the measured path.
- The M4.7 corpus-lane acceptance (20 real-world configs) stays with
  the corpus lane; this slice ships the engine + command + fixture
  suites over the VERIFIED grammars.
