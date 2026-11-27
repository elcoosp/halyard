# ADR-0061: the M8 RubyGems cell — the `halyard-eco-rubygems` adapter, Gem::Version planning, and the rubygems.org disabled-publish posture

- **Status:** accepted
- **Date:** 2026-11-27
- **Milestone:** §17 YEAR 2, **M8** *"More ecosystems"* — the next
  listed cell after the shipped `Dart/pub` half (ADR-0060), *"RubyGems"*,
  with the acceptance row *"each: discover/edit/publish/resume tests +
  replay on 2 real repos"*. The reconciliation survey found the cell
  **missing in code** (`grep -rni "gemspec\|rubygems" crates/
  --include="*.rs"` returns zero code hits — only the plan text; no
  `halyard-eco-rubygems`; no rubygems research note; `git log --oneline
  -50` and `git branch -a` carry no RubyGems work — only the stale
  merged-ADR branches the earlier sessions recorded), and the last
  PROGRESS row's recorded next start point names it first ("the next M8
  cell per plan order (RubyGems)"). The Maven/Gradle cell's Gradle half
  stays earlier in the row's list but is the plan's own recorded-open
  gate (*"an evaluated, VERIFIED grammar gates it"* — no Gradle grammar
  note exists in-tree and evaluating Groovy/Kotlin build code requires
  running Gradle), so the cell order resumes at RubyGems. M1 lands the
  discovery + planning slice (the gemspec scanner, the
  literal-assignment grammar, the version strategy, the name-proven
  internal edges, the planner gates); M2 lands the release half (the
  semantic edit seam, the apply-shell routing, the tag-target probe
  arm, the publish posture). The `gem build`/`gem push` argv arm and
  the corpus replay stay the recorded-open remainders (see
  Consequences), like the Dart cell's `dart pub publish` arm.
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

M8's ecosystems are six cells in: PyPI (ADR-0056), Go (ADR-0057), the
Maven half of Maven/Gradle (ADR-0058), NuGet (ADR-0059), Dart/pub
(ADR-0060). The adapter seam (`halyard_core::discover::EcosystemAdapter`,
§7.3) carries seven implementations — cargo (M1.5), npm (M1.6), pypi
(M8 step 1), go (M8), maven (M8), nuget (M8), dart (M8) — and new
built-ins require an ADR (`Ecosystem`'s `#[non_exhaustive]` doc). This
is that ADR for the eighth adapter.

RubyGems differs from every landed ecosystem in three structural ways:

1. **The manifest is executable Ruby.** The VERIFIED specification
   reference (`docs/research/rubygems-grammars.md`, fetched 2026-11-27
   from `guides.rubygems.org/specification-reference/`) defines the
   gemspec as a `Gem::Specification.new do |s| ... end` block whose
   attributes are assignments — arbitrary Ruby in the general case.
   The same page pins the fence the scanner rides: *"Specifications
   must be deterministic, as in the example above"* (conditionally
   defined attributes are invalid). The adapter therefore plans the
   **literal-assignment grammar**: only a `name`/`version` attribute
   assigned a plain string literal (single- or double-quoted, a
   trailing `.freeze` tolerated as the no-op idiom) captures with an
   honest byte span; computed values (`Shared::VERSION`),
   interpolations (`"1.#{n}"`), heredocs and conditionally guarded
   rows are recognized-but-non-canonical — warned, unpublishable, no
   edit site, never rewritten. The scanner is hand-rolled and
   quote-aware (comments, `;` statements, `do`/`{`/statement-keyword
   nesting, heredoc bodies consumed unmodeled) and refuses fail-closed
   with a line number on anything it cannot attribute (an unterminated
   string, an unclosed block, a second `Gem::Specification.new` block,
   a non-literal identity).

2. **No workspace grammar exists.** A `.gemspec` claims nothing beyond
   itself: every `*.gemspec` in the committed tree is a standalone
   package row (the gem is the package; the manifest is the gemspec
   file), and the internal edges are the **name-proven** ones — an
   `add_dependency` whose gem name matches another discovered gemspec
   (when both gems ship from one repository, the requirement targets
   the same artifact the repo publishes, so the edge drives the
   cascade; the Maven adapter's coordinate-proven shape, at gem-name
   granularity). Every requirement string (`>= 1.1.4`, the pessimistic
   `~> 13.0`) is a foreign grammar and rides verbatim (`ReqSpec::Other`,
   ADR-0006) — RubyGems requirements are not semver and are never
   rewritten.

3. **The version grammar is Gem::Version, not semver.** The VERIFIED
   `lib/rubygems/version.rb` pattern
   (`'[0-9]+(?>\.[0-9a-zA-Z]+)*(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?'`)
   accepts spellings the plan contract cannot render. The adapter
   applies the ADR-0056 discipline: only semver-parseable spellings are
   canonical (`1.2.3`, the dash-suffix form `1.2.3-rc.1`, letter-segment
   prereleases like `1.2.3.pre.1`); every other Gem::Version spelling
   (`1.2`, the four-segment `1.2.3.1`) is recognized-but-non-canonical
   — warned, unpublishable, no edit site, never rewritten. The
   documented canonical form (*"Versions shall be represented by three
   non-negative integers, separated by periods (e.g. 3.1.4)."*) is what
   the planner writes.

## Assumptions

- A1: the specification reference's required-attribute list (*"Required
  gemspec attributes: authors=, files, name, summary, version"*) means a
  gemspec without a `name` row is malformed — the adapter refuses
  (`E-DISC-MANIFEST`) instead of inventing an identity, mirroring the
  Dart adapter's missing-`name` posture.
- A2: one `Gem::Specification.new` block per `.gemspec` file is the
  supported subset; multi-spec files refuse fail-closed (a second block
  cannot share the file's single-row identity) — recorded, not silently
  re-scoped.
- A3: the semver contract's rendering of a target version (`1.3.0`,
  `1.3.0-rc.1`) is always Gem::Version-legal (the dash suffix is the
  pattern's own alternation), so the edit seam writes the plan's target
  text verbatim — no spelling transform exists between the plan
  contract and the manifest, unlike PyPI's PEP 440 pairing.
- A4: the tag template stays the group default (`{name}-v{version}`) —
  the plan names no gem-specific tag shape, and RubyGems projects
  follow the same `v{version}` convention the other manifest-versioned
  ecosystems ride.

## Decision

1. **M1 — the discovery + planning slice.** `Ecosystem::RubyGems` joins
   core (`resolve` → `rubygems`); `ManifestFormat::Gemspec` joins the
   discovery error vocabulary; the hand-rolled scanner
   (`halyard-eco-rubygems/src/grammar.rs`) owns the
   literal-assignment grammar with line-numbered refusals and
   `conditional-attribute`/`non-canonical-version`/`unmodeled-dep`
   warnings; `RubyGemsAdapter::discover` walks every `*.gemspec` in the
   committed tree (sorted walk order, I3) into standalone rows with
   name-proven internal edges; the planner's registry arm names
   `rubygems.org` with the engine's disabled shape (`publishable =
   false` at discovery); the orchestrator dispatch and the tree walk
   extend (`.gemspec` joins the manifest walk); the xtask layer rule
   and member guard extend (28 crates ✓); the planner gates pin the
   disabled publish shape and the cargo control
   (`crates/halyard-core/tests/planner_rubygems.rs`). Red→green on
   record: the red was the missing seam itself (E0432 on
   `halyard_eco_rubygems::RubyGemsAdapter`, E0599 on
   `Ecosystem::RubyGems` and `ManifestFormat::Gemspec`; the plan line
   the first test pins is §17 M8's *"RubyGems"* cell, quoted in the
   contract tests' module docs).

2. **M2 — the release half.** The semantic edit seam
   (`edit_versions`: the patch span must equal the version literal's
   inner-byte span — quotes, `.freeze` and trailing comments never
   join it; the value's inner bytes swap right-to-left; the read-back
   verify re-parses before acceptance; the idempotent re-run skips the
   carried target I1; a slotless gemspec refuses
   `E-EDIT-PATCHES`); the apply shell's manifest map routes
   `*.gemspec` → `rubygems`; the tag-target probe's explicit
   `rubygems` arm (`manifest_version` pairs the semver-canonical
   spellings; a non-parseable Gem::Version keeps the documented HEAD
   fallback); and the publish posture — `publishable = false` at
   discovery, the planner's registry arm naming `rubygems.org`, the
   engine recording the `publish_disabled` skip, no tool spawned (the
   documented flow pushes a *built* `.gem` — `gem push
   squid-utils-0.1.0.gem` — and its credential surface is the local
   `~/.gem/credentials` file maintained by `gem signin`; the publishing
   guide names no token environment variable the auth seam's env
   capture could carry). Red→green on record: the red was the edit
   contract against the interim fail-closed seam (the go posture
   carried over M1) — the four green-path edit tests failed at red,
   green at the semantic patcher.

3. **Docs.** The research note (`docs/research/rubygems-grammars.md`)
   carries the VERIFIED quotes (the specification reference's block
   shape, determinism rule, required attributes and dependency
   methods; `version.rb`'s VERSION_PATTERN and the three-integer
   canonical form; the name-your-gem rules; the publishing guide's
   `gem push` argv and credentials-file surface). The README gains the
   feature bullet, the crate-table row, the adapter list entry and the
   example rows. No new error code (discovery/edit reuse the
   `E-DISC-*`/`E-EDIT-*` pages; `ManifestFormat` gains the `Gemspec`
   variant its error reports name); `E-DISC-NO-PACKAGE`'s Display and
   page and `halyard init`'s no-manifests refusal name the `.gemspec`
   family now that a rubygems-only repository is plannable.

## Consequences

- Positive: rubygems repositories — one of the year-two M8 row's
  named ecosystems — plan, tag, edit and forecast end-to-end offline;
  the gem-name cascade drives monorepo gem releases; the scanner's
  fail-closed refusals name line numbers, so a gemspec outside the
  literal-assignment grammar is diagnosable, never half-planned.
- Neutral: the planner's registry arm (`rubygems.org`) is truthful the
  day the argv arm flips it — the same shape the Maven/NuGet/Dart
  cells shipped.
- Recorded-open remainders (not silently re-scoped): the `gem build` +
  `gem push` argv arm (the build-artifact step is native-tool plumbing
  the publish engine does not carry yet — a `.gem` must be built from
  the edited gemspec before the push), the rubygems.org registry
  probe arm (the M5.1 probes' discipline), the corpus-lane replay on 2
  real repos (the M8 cell's acceptance-row half, network-bound, the M7
  lane discipline), the multi-spec `.gemspec` subset (A2), and the
  Maven/Gradle cell's Gradle half (the plan's own recorded-open gate —
  an evaluated, VERIFIED Gradle grammar).
