# ADR-0057: the M8 Go-modules cell — the `halyard-eco-gomod` adapter, tag-only planning, and the subdir tag shapes

- **Status:** accepted
- **Date:** 2026-11-26
- **Milestone:** §17 YEAR 2, **M8** *"More ecosystems"* — the second
  listed cell, *"Go modules (tag-only, `go.work`, subdir tags)"*, with
  the acceptance row *"each: discover/edit/publish/resume tests + replay
  on 2 real repos"*. The reconciliation survey found the cell **missing
  in code** (no `go.mod`/`go.work`/`gomod` symbols anywhere in
  `crates/`; PROGRESS records no Go work; the prior session's recorded
  next start point names this cell), and the plan's first unclaimed M8
  cell after the PyPI one. M1 lands the discovery + planning slice (the
  adapter, the tag-only version seam, the subdir tag templates); M2
  lands the release-phase half (the tag-only publish decision, the
  tag-target probe arm, the apply-shell routing). The corpus-lane
  replay on 2 real repos stays the recorded-open remainder (forge/
  corpus lanes are token-bound, the M7 discipline), and the GOPROXY
  existence-probe arm stays unstarted (a tag-only ecosystem has no
  registry publish to resume).
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

M8's ecosystems are one cell in: PyPI (ADR-0056). The adapter seam
(`halyard_core::discover::EcosystemAdapter`, §7.3) carries three
implementations — cargo (M1.5), npm (M1.6), pypi (M8 step 1) — and the
plan's own extension path is documented in the code: *"new built-ins
require an ADR"* (`Ecosystem`'s `#[non_exhaustive]` doc). This ADR is
that ADR for the fourth adapter.

Go differs from every landed ecosystem in one structural way: **the
manifest carries no version**. VERIFIED quotes in
`docs/research/go-module-grammars.md` (fetched 2026-11-26 from
`go.dev/ref/mod` and `go.dev/doc/modules/publishing`):

- *"A go.mod file must contain exactly one module directive"* — the
  directive names the path, never a version;
- versions are the repository's semantic version tags (`git tag
  v0.1.0` is the documented publish flow), and a module in a
  subdirectory tags `{subdir}/vX.Y.Z`;
- *"Starting with major version 2, module paths must have a major
  version suffix like /v2 that matches the major version"*;
- workspaces ride `go.work`: *"A `use` adds a module on disk to the set
  of main modules in a workspace. Its argument is a relative path to
  the directory containing the module's go.mod file"*; *"When no
  go.work file exists, the workspace consists of the single module
  containing the current directory."*

## Decision

### D1 — tag-only planning (the version seam moves to the release base tag)

A Go package discovers with `version: None`, `RawVersionSource::Tag`
(the new variant, mirroring `VersionSource::Tag` in the core package
model), **no** version site, and `publishable = false` — and the
planner, which before skipped every version-less package
(`materialize_versions`' `continue`), plans tag-only packages from the
release base tag: `from` = the §8.2 base tag's version parsed through
the package's own tag template (`base_version_of`), or the zero version
for an initial release. The ecosystem gate is
`Ecosystem::is_tag_only()` — no other ecosystem's planning shape
changes (pinned by the manifest-versioned control test in
`halyard-core/tests/planner_tag_only.rs`).

The skip vocabulary stays honest: a version-less **tag-only** package
with no commits and no base skips as `no_changes` (its missing manifest
version is the grammar, not a defect); the `no_version` reason remains
for manifest-versioned ecosystems.

### D2 — the major version suffix rule, enforced at planning

`halyard_core::package::major_version_suffix(name)` (the FINAL `/vN`,
N ≥ 2, no leading zero, no dots — the reference's own path rules) backs
a planner guard: a tag-only package whose target's major is below its
declared `/vN` major is **refused and reported** (`major_version_suffix`
in the plan's `skipped` rows) — never silently re-scoped, never a
guessed `v1.x` tag for a module whose path declares v2. The suffix rule
lives in core (next to `is_tag_only`) because the planner reads it; the
adapter re-exports the same function so one implementation owns the
rule.

### D3 — the tag templates ride the shared grammar, no new syntax

Subdir tags are not a Go-specific template language: the adapter
derives each package's `tag_template` from its repo-relative directory
(`v{version}` at the root, `{dir}/v{version}` elsewhere) inside the
existing appendix-A.3 grammar, so the §8.2 base-tag matcher,
`TagTemplate::extract_version`, the fetch planner and the tag phase
work unchanged.

### D4 — discovery scope: `go.work` `use` directives, the single-module posture, workspace-proven internal edges

- A root `go.work` discovers its `use` directories (normalized:
  `.`/`./` → the root, `./api` → `api`; a path escaping the tree or an
  absolute path is a fail-closed `E-DISC-GLOB`). Every named member
  must carry a go.mod in the committed tree (a `use` directive is a
  requirement, not a candidate root — the npm explicit-member posture).
  The root module is a member only when `use .` names it (the
  documented grammar says nothing about an implicit root member — none
  is invented).
- No `go.work`: the root `go.mod` is the single package (§6.1
  single-package mode); nested go.mods nothing claims are skipped (the
  orphan-manifest posture).
- The go.mod grammar (`module`, `go`, `require`, `replace`, `exclude`,
  `retract`; Go 1.24's `tool`/`ignore` recognized-and-skipped) parses
  with a hand-rolled, offset-preserving lexer (`halyard_eco_gomod::
  grammar` — comments blanked in place so spans point into the original
  bytes). `exclude`/`retract` constrain go's own resolution, not
  halyard's release planning, and are consumed unmodeled.
- Internal edges are the workspace-proven ones: a require naming a
  `use` member's module path, and a require named by a local-path
  `replace` (go.work's or the module's own). Everything else stays
  registry-resolved (not internal) — the cargo differential discipline.
  Requirement text rides `ReqSpec::Other` verbatim (ADR-0006): the
  semver rewriter never rewrites a Go version row (go tooling owns
  them), and the cascade still bumps dependents.

### D5 — the tag-only edit seam fails closed; the publish decision is `None`

- `edit_versions` has no legitimate input (no version row exists, and
  requirement rewrites never touch Go rows): any patch fails closed
  `E-EDIT-PATCHES` with the untouched bytes; an empty patch set returns
  the verbatim bytes (the sibling adapters' posture for an empty pass).
  `halyard-edit`'s manifest map routes `go.mod` → the `go` adapter so a
  contract violation hits the honest decline, not an unknown-manifest
  error.
- A Go release row carries `publish: None` — the plan contract's
  documented shape for "no registry publishing decision" (plan.rs's
  `PlanRelease::publish` doc widened for the tag-only case). There is
  no `go` registry name, no publish-effect arm and no auth arm: the
  publish engine already skips `None` rows, and inventing a registry
  string would be dead grammar.

### D6 — layering and wire-up

- New crate `halyard-eco-gomod` (layer rule in `xtask/src/layers.rs`:
  workspace deps `halyard-core` only; external `semver` — no new
  parser dependency; the Go line grammar is hand-rolled).
- `halyard-discover` gains the adapter in the fixed dispatch order
  (after pypi) and the tree walk reads `go.mod` (root + nested) and the
  root `go.work`.
- The tag-target pinpoint walk's manifest probe gains the explicit
  `go` arm returning `None` (`halyard_eco_gomod::manifest_version` —
  the documented HEAD fallback, now by the adapter's own contract
  rather than the fall-through).

## Consequences

- Fourth adapter live end-to-end: discovery → workspace rows → plan
  (tag-only from-versions, subdir tags, the /vN guard) → release rows
  with no publish decision; the conformance discipline (unit + contract
  + example + bench) matches the cargo/npm/pypi pattern.
- The M8 Go cell is **partial** by design: the discover/plan and
  release-phase halves ship in-tree green; the corpus-lane replay on 2
  real repos and the GOPROXY probe arm stay recorded-open (PROGRESS
  row), like the PyPI cell's remainders.
- The stability contract (ADR-0055) is untouched: the plan schema
  carries no new keys (Go rows carry no `publish` decision at all), and
  no new error code was needed (discovery/edit reuse the
  `E-DISC-*`/`E-EDIT-*` pages; `ManifestFormat` gains the `Gomod`
  variant its error reports name).
- The manifest-family refusal texts (`halyard init`'s no-manifests
  message, `E-DISC-NO-PACKAGE`'s display and its docs page) name the go
  grammar now that a go-only repository is plannable (docs drift fixed
  with the seam).
