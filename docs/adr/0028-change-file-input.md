# ADR-0028: change-file input (M3.5 completion, §9.4)

- **Status:** accepted
- **Date:** 2026-11-16
- **Milestone:** M3.5 completion — the change-file half of the roadmap
  row ("Changelog engine + styles + change-file support") that
  ADR-0025's sessions left open
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

The §9.4 input model had exactly one path into the planner: the
conventional-commit stream. Two real-world repos have no usable signal
there — squash-merge histories whose titles carry no conventional
grammar, and releases whose *content* decision (what to call this
release) is human, not mechanical. The architecture reserved the escape
hatch all along: optional **change files** `.halyard/changes/*.md`
(§9.4), with the documented precedence "`Release-As` footer >
change-file explicit bump > conventional commits > cascade > none" and
the notes "merged into notes, consumed on release". The wire format was
already pre-wired for it — `Reason::Changeset` /
`PlanReason::Changeset` existed with renderers in `pr_body` and `explain`
— but nothing produced the reason: no parser, no planner input, no
merge. The seam was scaffolded; the capability was missing.

Constraints that shaped the design:

1. **The planner stays pure (I3/I6).** The change files ride
   [`Snapshot`] as plain data — a new `change_files` field beside
   `refs` — materialized by the shell (the CLI scans
   `.halyard/changes/*.md`; the strict grammar is parsed in
   `halyard-changelog`, the §9.4 crate). No file I/O in `halyard-core`.
2. **Explicit replaces, never unions.** A change-file bump *replaces*
   the commit-derived level outright (both directions): `feat` commits
   under a `patch` file plan the patch. That is what "escape hatch"
   means — a stray conventional commit must not out-vote the explicit
   input. Implementation-wise the bump slot is overwritten in
   `direct_bumps` after the commit loop; the cascade then exempts the
   package for free (the monotone guard sees the replaced slot, and
   every explicit level is at least `Patch`), which also honors
   "cascade < change file". The commit reasons stay in the chain —
   they document what happened; the `Changeset` reason documents what
   won.
3. **`Release-As` stays the top of the ladder.** A forced target wins
   over any change-file bump (the guard reads the existing `forced`
   slot at materialization; nothing about forced semantics changed).
   Both reasons appear in the chain when both inputs are present: each
   documents one explicit input.
4. **The grammar is TOML-style and strict.** `packages = { name =
   "bump", ... }` between `---` delimiters — TOML inline-table syntax,
   the config crate's dialect, quoted `major`/`minor`/`patch`, bare
   names matching the npm scoped-name charset (`@scope/pkg` works).
   Anything else refuses with `E-CHLOG-CHANGE-FILE` (the
   `E-CFG-UNKNOWN-KEY` discipline applied to the change-file layer);
   a file naming an unknown package refuses with `E-PLAN-CHANGE-FILE`
   at plan time. One bad file fails the whole load — input is never
   silently dropped.
5. **Notes merge into the plan's changelog preview.** The merge point
   is `render_entry` (§9.4): note bodies append verbatim, blank-line
   separated, in path order, no invented headings (no fabrication).
   That single point feeds every downstream consumer for free — the
   §11.3 PR body preview, the CHANGELOG.md write, and the §11.1
   `create_release` payload (which extracts the section the merged PR
   wrote) — so the PR preview and the release body can never disagree.
6. **Strongest bump wins across files.** Two files naming one package
   (say `patch` and `minor`) plan the minor — the same monotone
   strongest-wins rule every other bump source obeys; no new
   conflict-error semantics. Notes from all of them merge in path
   order. The loader sorts paths, `ChangeFiles::record` preserves
   order — deterministic regardless of directory iteration order (I3).

## Decision

- `halyard-core::change_file`: the pure `ChangeFiles` input type
  (bumps + notes per package name, strongest-wins `record`), carried
  by `Snapshot::change_files`.
- `halyard-changelog::change_files`: the strict grammar
  (`parse_change_file`) and the deterministic loader
  (`load_change_files`), failing closed under
  `ChangelogError::ChangeFile` (`E-CHLOG-CHANGE-FILE`).
- `halyard-core::planner`: `direct_bumps` applies the explicit bump
  after the commit loop (overwrite + `Reason::Changeset`), `plan()`
  validates names (`PlanError::ChangeFilePackage`,
  `E-PLAN-CHANGE-FILE`), `render_entry` merges the notes.
- `halyard-cli`: scans `.halyard/changes/*.md` under the repo root in
  `plan_from_context` — the same pipeline every plan-consuming command
  (`plan`, `explain`, `apply`, `pr`, `release`) shares, so the input
  reaches all of them at once. A missing directory is the zero-config
  shape.

## Consequences

- **Consumed-on-release is a maintainer contract, not automation.**
  The plan never deletes the change file: the edit engine's effect
  vocabulary is byte patches, not file removals, and a silent delete
  is against the create-only instinct (ADR-0023). While the file is
  present, every plan keeps proposing its bump — deterministic, never
  destructive — so landing a release means landing the PR *and*
  removing the consumed files, exactly like changesets' workflow. The
  docs pages and the CLI test fixtures demonstrate the contract.
- **Squash-merge repos get a real path.** A non-conventional history
  plus one change file plans a release with the note carrying the
  description the commits cannot (`golden_change_file_plan` pins the
  wire format).
- **Group policy stays above the change file.** Fixed/Linked
  normalization reads the replaced slots, so a group member's union
  can raise an explicit bump — the §9.4 precedence list orders the
  per-package sources, not the §5.4 policy passes.
- The `snapshot`-shaped API breaks every `Snapshot` construction
  site; all eleven were updated (3 test suites, 3 benches, 2 examples,
  the xtask replay + perf harnesses, the CLI).

## Alternatives considered

- **YAML front-matter (changesets' dialect).** Rejected: it would add
  a YAML dep to a leaf crate or a hand parser of a grammar halyard
  does not own; the TOML inline table reuses the dialect the repo
  already speaks and the architecture example's `{core: minor}` shape
  survives the translation.
- **Config key to toggle the input.** Rejected: presence is opt-in
  enough (no directory = no input), and a key would need schema
  regeneration for zero expressive gain — the zero-config path stays
  untouched.
- **Deleting consumed files from the plan.** Rejected for v1 (see
  Consequences); revisit if a file-removal effect lands with the
  apply-pipeline milestones.
