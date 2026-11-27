# ADR-0062: the M8 `generic` cell — config-declared packages over regex-edited version files, the `Ecosystem::Generic` row shape, and the `DiscoverInput` config channel

- **Status:** accepted
- **Date:** 2026-11-27
- **Milestone:** §17 YEAR 2, **M8** *"More ecosystems"* — the next
  listed cell after the shipped `RubyGems` cell (ADR-0061), *"`generic`
  (regex-edited version files)"*, with the acceptance row *"each:
  discover/edit/publish/resume tests + replay on 2 real repos"*. The
  reconciliation survey found the cell **missing in code**
  (`grep -rni "Ecosystem::Generic\|eco-generic\|GenericAdapter" crates/
  --include="*.rs"` returns zero code hits; no `halyard-eco-generic`
  crate; no generic research note; the `Ecosystem` enum ends at
  `RubyGems` + the unused `Custom(Sym)`; `git log --oneline -50`
  carries no generic work; `git branch -a` shows only the stale
  merged-ADR branches earlier sessions recorded), and ADR-0061's
  recorded cell order (*"the cell order resumes at RubyGems"*) names
  `generic` next per the M8 row's own list. M1 lands the discovery +
  planning slice (the `[[generic]]` config surface, the
  config→discovery channel, the regex scanner, the planner gates); M2
  lands the release half (the semantic edit seam, the apply-shell
  routing, the path-aware tag-target probe arm, the publish posture).
  The multi-file-package form, shared version files, and the corpus
  replay stay the recorded-open remainders (see Consequences).
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

M8's ecosystems are seven cells in: PyPI (ADR-0056), Go (ADR-0057),
the Maven half of Maven/Gradle (ADR-0058), NuGet (ADR-0059), Dart/pub
(ADR-0060), RubyGems (ADR-0061). Every landed adapter owns a **fixed**
manifest grammar: discovery scans the committed tree for the family's
manifest files and parses them at the VERIFIED grammar. The `generic`
cell is the row the plan names for everything else — the escape hatch
for repositories no manifest grammar covers (`VERSION` files,
`include/version.h` headers, script constants). There is no fixed
grammar to scan for, so the locator must be **user-declared**: a
version file path plus a regular expression whose named capture group
`version` spans the version text. The VERIFIED prior art
(`docs/research/generic-version-files.md`) is Knope's `versioned_files`
regex form (*"The regex pattern must include a named capture group
called version … This capture group identifies the version number to
replace."*; *"The matched string must be a valid semantic version"*);
release-please's `Generic` updater rides fixed `x-release-please-*`
annotations instead and is the alternative halyard's cell exists to
avoid. The engine is the `regex` crate (the VERIFIED group semantics:
`(?<name>exp)` named groups, groups *"indexed by opening parenthesis"*
, group 0 *"implicit … the entire match"*).

Structural differences from every landed ecosystem:

1. **Discovery is config-driven, not tree-scanned.** A `generic`
   package exists because `halyard.toml` declares it — the committed
   tree alone cannot distinguish a version file from any other text
   file. The `DiscoverInput` struct already exists as the
   config→discovery channel (`links`, §6.5) but every CLI call site
   passed `DiscoverInput::default()`, so no config ever reached the
   orchestrator. The cell's wire-up makes that channel real:
   `[[generic]]` entries flow from the resolved config into discovery,
   and the `[[link]]` entries — resolved by the loader since M4.7 but
   never fed to `detect_links` from any shell — ride the same channel.
2. **The version locator is the regex match, not a grammar-owned row.**
   The pattern must compile and carry exactly one named `version`
   capture group (unnamed groups refuse — ambiguous intent; the
   leftmost match is the version row); the captured text follows the
   ADR-0056 fail-closed discipline: only semver-parseable captures are
   canonical, every other spelling is recognized-but-non-canonical —
   warned, unpublishable, no edit site, never rewritten.
3. **No registry grammar exists — not even a name.** Every disabled
   publish posture so far recorded a truthful registry name
   (`maven-central`, `nuget.org`, `pub.dev`, `rubygems.org`) the day
   an argv/auth arm could flip it. A generic package names no registry
   at all — inventing one is precisely what the escape hatch must not
   do — so release rows carry the **no-decision shape** (`publish:
   None`, the ADR-0057 Go shape minus tag-only-ness): the tag is the
   artifact, the user's own pipeline owns publishing, and the publish
   engine's documented `no_publish_contract` skip keeps the row out of
   every forecast.

## Assumptions

- A1: one `[[generic]]` entry is one package with **one** version file
  (the file is the package's manifest row; `version_site` targets its
  capture span). Multi-file packages (Knope's pattern-array form) and
  two packages sharing one version file refuse at load — a shared file
  would aim two packages' edits at one span. Recorded, not silently
  re-scoped.
- A2: the leftmost match of the pattern in the file is **the** version
  row; further matches are consumed unmodeled with a warning. A
  multiple-match grammar is a superset (recorded-open, A1's family).
- A3: the pattern's regex-crate compilation is the load-time validation
  surface (`E-CFG-GENERIC`): a pattern that does not compile, or that
  does not carry exactly one named `version` group, is a config error,
  not a discovery error — the same M1.8 posture as the tag-template
  grammar (`E-CFG-TAG-STYLE`).
- A4: the tag template stays the group default (`v{version}` /
  `{name}-v{version}` per package count) — the plan names no generic
  tag shape, and a regex-edited version file implies a
  manifest-versioned (not tag-only) release.
- A5: the declared file must be a normalized repo-relative path that
  exists in the committed tree; a declared-but-missing file is the
  `E-DISC-GLOB` refusal (the same member-missing-from-tree posture the
  dart adapter pinned).

## Decision

1. **M1 — the discovery + planning slice.** `Ecosystem::Generic` joins
   core (`resolve` → `generic`; the `#[non_exhaustive]` extension per
   its own doc); `ManifestFormat::Text` joins the discovery error
   vocabulary (the declared file is plain text — the format any
   manifest error reports); the `GenericFileSpec` vocabulary type
   (name, file, pattern — plain data) lives beside the adapter trait it
   feeds. The `[[generic]]` config surface (`name`/`file`/`pattern`)
   joins `halyard.toml` v1 (schema regenerated; the fail-closed
   unknown-key contract untouched), validated at load with the new
   `E-CFG-GENERIC` code (compile failure, capture-group shape,
   duplicate names, duplicate files, non-normalized paths — the docs
   page joins `docs/errors/`). The `halyard-eco-generic` crate (the
   29th member) owns the scanner: compile the spec's pattern, match
   the UTF-8 file bytes, capture group `version`'s byte span + text,
   `non-canonical-version`/`no-version`/`unmodeled-match` warnings.
   `DiscoverInput` carries the specs; `collect_ctx` reads the declared
   files into the context; the orchestrator dispatches the
   spec-constructed adapter after rubygems (fixed order). The CLI
   resolves config **before** discovery in the shared pipeline
   (`context_with_base`, `cmd_fetch`, `doctor`) and builds the input
   from it — `[[link]]` entries finally reach `detect_links` the same
   way (the dead seam's repair, additive). The planner's ecosystem arm
   gives generic rows `publish: None` (no registry name to record —
   D3). The xtask layer rule and member guard extend (29 crates ✓;
   `halyard-eco-generic`: core + `semver` + `regex`; `halyard-config`
   consciously gains `regex` for the load-time validation). The
   planner gates pin the no-decision publish shape and the cargo
   control (`crates/halyard-core/tests/planner_generic.rs`). Red→green
   on record: the red was the missing seam itself (E0432 on
   `halyard_eco_generic::GenericAdapter`, E0599 on `Ecosystem::Generic`
   and `ManifestFormat::Text`; the plan line the first test pins is
   §17 M8's *"`generic` (regex-edited version files)"* cell, quoted in
   the contract tests' module docs).

2. **M2 — the release half.** The semantic edit seam (`edit_versions`:
   the spec-owning adapter re-locates the `version` capture in the
   current bytes — the patch span must equal the capture's span, a
   drifted file refuses `E-EDIT-PATCHES`/`SpanMismatch`; the captured
   bytes swap in place, every other byte verbatim (I4); the read-back
   verify re-matches before acceptance (`E-EDIT-VERIFY` posture); the
   idempotent re-run skips the carried target (I1); a file that no
   longer matches refuses). The apply shell's adapter list chains the
   config-constructed generic adapter beside the static set (the
   routing resolves through the owning package's ecosystem — no
   file-name fallback exists for arbitrary paths). The tag-target
   probe's generic arm rides a **path-aware** `ManifestVersionProbe`
   (the trait gains the manifest path — the resolver's
   `TagTargetRequest` always carried it; a config-declared pattern
   cannot be picked without it), with the shell's probe carrying the
   specs. Red→green on record: the red was the edit contract against
   the interim fail-closed seam (the go posture carried over M1) — the
   green-path edit tests failed at red, green at the semantic patcher.

3. **Docs.** The research note
   (`docs/research/generic-version-files.md`) carries the VERIFIED
   quotes (Knope's named-group requirement and semver rule; the
   release-please annotation contrast; the regex crate's group
   semantics and UTF-8 haystack posture). The README gains the feature
   bullet, the crate-table row, the adapter list entry and the example
   rows. The new `E-CFG-GENERIC` error page joins `docs/errors/`;
   discovery/edit reuse the `E-DISC-*`/`E-EDIT-*` pages (`ManifestFormat`
   gains the `Text` variant its error reports name).

## Consequences

- Positive: a repository no manifest grammar covers — the plan's own
  escape-hatch row — plans, tags and edits end-to-end offline; the
  `[[link]]` config surface stops being dead (the same channel); the
  regex locator is validated at load with line-free but precise
  diagnostics (compile error text + the named-group contract), so a
  broken pattern is a config error before discovery ever runs.
- Neutral: generic rows are the first manifest-versioned rows with
  **no** registry decision (`publish: None`); the publish engine's
  documented skip needed no change — the shape the Go cell proved.
- Recorded-open remainders (not silently re-scoped): the multi-file
  package form and pattern arrays (A1), shared version files (A1),
  multiple-match grammars (A2), the corpus-lane replay on 2 real repos
  (the M8 cell's acceptance-row half, network-bound, the M7 lane
  discipline), and the Maven/Gradle cell's Gradle half (the plan's own
  recorded-open gate — an evaluated, VERIFIED Gradle grammar).
