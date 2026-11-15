# ADR-0024: the knope and release-plz migration sources (M4.7 continued, §16.2)

- **Status:** accepted
- **Date:** 2026-11-15
- **Milestone:** M4.7 continued — `halyard migrate` surveys the §16.2
  row's remaining two tools: the knope and release-plz configurations
  join the release-please/changesets sources behind the same
  `MigrationSource` seam (ADR-0022), the same fail-closed survey
  contract and the same validated-draft shape
- **Deciders:** autonomous implementation agent (per §0 rule 3; the
  foreign grammars ride the VERIFIED quotes recorded in
  `docs/research/migrate-config-grammars.md`)

## Context

ADR-0022 shipped the report-first survey with two of the four §16.2
tools; every session log since pinned "the knope/release-plz sources"
as the declared next start point. The seam needs no changes — the two
sources are new `MigrationSource` impls over the discovered
`ForeignFiles` — but the two grammars raise mapping questions the first
two tools did not: knope documents *tag rules* (not configurable tag
templates), and release-plz documents *Tera templates* (not appendix-A.3
templates).

Constraints that shaped the design:

1. **Preserve the foreign tool's effective behavior** (ADR-0022): a
   mapped pin must reproduce what the foreign tool does, or the report
   must say what it cannot. For knope, the Release step's documented
   tag rules *are* the effective behavior: one package tags
   `v{version}`, multiple packages tag `{package_name}/v{version}` —
   both shapes compose into exactly one appendix-A.3 template per
   shape, so the mapping is a translation of a documented rule, not a
   guess.
2. **A foreign template language is never half-translated.**
   release-plz's `git_tag_name` is a Tera template. The translation
   rule is placeholder substitution over the two documented
   placeholders (`{{ package }}` → `{name}`, `{{ version }}` →
   `{version}`); any other Tera variable, block or whitespace variant
   leaves the template untranslatable, and the survey reports it as an
   unmapped row naming the placeholders the v1 grammar accepts. A
   silent partial translation would pin a tag style halyard renders
   differently than release-plz did.
3. **The workspace default is data the survey cannot observe.**
   release-plz's `git_tag_name` default depends on the repo shape
   (single crate vs workspace). The survey sees only the config files,
   so the default row maps the documented *workspace* default and its
   note names the single-crate rule halyard's own `auto` applies — the
   effective behavior is preserved either way without inventing facts
   the survey cannot know.
4. **Publishing stays opt-in (invariant I7, ADR-0022's changesets
   convention).** release-plz publishes by default (`publish = true` is
   the documented default); the survey reports `publish` as an unmapped
   row carrying the I7 reason and the draft never sets
   `publish.enabled`.
5. **The two knope package shapes are exclusive.** knope documents two
   layouts (one `[package]` or named `[packages."<name>"]`
   sub-tables); a config carrying both is broken, so it refuses
   `E-CFG-MIGRATE` instead of picking.
6. **The registry stays the extension point.** Both sources land in
   `default_sources()`; `migrate()` still runs any slice of sources, so
   a downstream caller can survey one tool in isolation.

## Consequences

- `halyard migrate` now surveys all four §16.2 tools; the command's
  JSON/human/GHA renderers needed no changes (the report is data).
- The `write` step is untouched: one configured tool with a non-empty
  draft still lands verbatim; knope/release-plz surveys with nothing
  mappable are not write candidates (an empty draft is a no-op, the
  ADR-0023 rule).
- The research note gains the two grammars' quotes and the mapping
  consequences; the corpus-lane acceptance (real-world configs) stays
  the open VERIFY item for both, as for the first two tools.
