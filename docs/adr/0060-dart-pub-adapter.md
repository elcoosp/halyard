# ADR-0060: the M8 Dart/pub cell — the `halyard-eco-dart` adapter, pub-workspace planning, and the pub.dev disabled-publish posture

- **Status:** accepted
- **Date:** 2026-11-27
- **Milestone:** §17 YEAR 2, **M8** *"More ecosystems"* — the next
  listed cell after the shipped `NuGet` half (ADR-0059), *"Dart/pub"*,
  with the acceptance row *"each: discover/edit/publish/resume tests +
  replay on 2 real repos"*. The reconciliation survey found the cell
  **missing in code** (`grep -rni "pubspec\|Ecosystem::Dart" crates/
  --include="*.rs"` returns zero hits; no `halyard-eco-dart`; no dart
  research note; `git log --oneline -50` and `git branch -a` carry no
  Dart work — only the stale merged-ADR branches the earlier sessions
  recorded), and the last PROGRESS row's recorded next start point
  names it first ("the next M8 cell per plan order (Dart/pub)"). M1
  lands the discovery + planning slice (the pubspec scanner, the
  pub-workspace walk, the version strategy, the two-proven internal
  edges, the planner gates); M2 lands the release half (the semantic
  edit seam, the apply-shell routing, the tag-target probe arm, the
  publish posture). The `dart pub publish` argv/auth arm and the corpus
  replay stay the recorded-open remainders (see Consequences), like the
  Maven cell's `mvn deploy` arm.
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

M8's ecosystems are five cells in: PyPI (ADR-0056), Go (ADR-0057), the
Maven half of Maven/Gradle (ADR-0058), NuGet (ADR-0059). The adapter
seam (`halyard_core::discover::EcosystemAdapter`, §7.3) carries six
implementations — cargo (M1.5), npm (M1.6), pypi (M8 step 1), go (M8),
maven (M8), nuget (M8) — and new built-ins require an ADR
(`Ecosystem`'s `#[non_exhaustive]` doc). This is that ADR for the
seventh adapter.

Dart/pub differs from every landed ecosystem in two structural ways:

1. **The workspace grammar is name-proven, not only path-proven.** The
   VERIFIED pub-workspaces reference
   (`docs/research/dart-pub-grammars.md`, fetched 2026-11-27 from
   `dart.dev/tools/pub`) documents that workspace packages resolve to
   each other *"regardless of the source"*: a dependency **naming** a
   member is an internal edge even when its declared source is hosted.
   The adapter ships both proofs — the name match (workspace-scoped)
   and the `path:` resolution ("considered relative to the directory
   containing your pubspec") — because the doc quote makes the name
   match authoritative, not a heuristic.
2. **The publish posture is disabled for the absence of an auth
   grammar, not an argv grammar.** `dart pub publish` is the documented
   command (the publishing page), but the pub environment-variables
   page documents only `PUB_CACHE` and `PUB_HOSTED_URL` — **no token
   variable**. pub.dev publishing rides the pub client's own OAuth2
   credentials flow; there is no env-var secret the
   `halyard-publish` auth seam could carry. Maven (ADR-0058 D5) and
   NuGet (ADR-0059) took the same disabled shape for unverified argv
   grammars; Dart's is the stricter case — the argv is verified but
   the credential surface is not automatable through the documented
   environment.

The version grammar needs no comparator work: *"A version number is
three numbers separated by dots … It can also optionally have a build
(+1, +2, +hotfix.oopsie) or prerelease (-dev.4, -alpha.12, -beta.7,
-rc.5) suffix. When you select a version, follow semantic versioning."*
— the pub grammar **is** the plan contract's own semver type, so the
ADR-0056 discipline reduces to one fail-closed set: spellings the
semver crate rejects (two-component `1.2`, quoted escapes, null rows)
are warned, unpublishable, no edit site, never rewritten.

## Assumptions

- Workspace discovery is **root-anchored**, like every landed
  ecosystem: the repository-root `pubspec.yaml` is the posture
  selector. Without it, every `pubspec.yaml` in the tree is a
  standalone package (the orphan-manifest posture — pub treats them as
  independent resolutions; the melos-style shape plans).
- The stray-file refusal is scoped to pubspecs **between the root and a
  claimed member** (an ancestor directory of a member), per the
  reference's own tree example (the stray `packages/pubspec.yaml`
  errors; a sibling `docs/pubspec.yaml` resolves independently and is
  consumed unmodeled).
- The glob form ships the documented shape only — exactly one `*` as a
  whole path segment (`packages/*`, Dart 3.11+); `**`, mid-segment
  stars and other patterns refuse (`E-DISC-MANIFEST`) instead of
  guessing glob semantics.

## Decision

- `halyard-eco-dart` joins the workspace as the 27th member (§7.2: core
  + `semver`, no other deps). `Ecosystem::Dart` resolves to `"dart"`;
  `ManifestFormat::Pubspec` joins the manifest-syntax set.
- **Scanner** (`grammar.rs`): a hand-rolled, offset-preserving
  `pubspec.yaml` subset scanner (the pnpm YAML discipline carried over:
  block maps/lists, single/double quotes, `#` comments, CRLF, BOM skip;
  tabs rejected; flow style, block scalars, anchors/aliases, multi-doc
  markers and quoted escapes refuse with line numbers). Spans point
  into the original bytes (I4) — the `version` value's inner span is
  the edit site; quotes and trailing comments never join it.
- **Discovery** (`lib.rs`): the root pubspec with `workspace` entries
  is the aggregator — members walked recursively (nested workspaces are
  the documented shape), each gated on `resolution: workspace`, each
  claimed exactly once; the root itself is a row (the documented
  `dart pub workspace list` prints `_ ./`). The package identity is the
  `name` field (*"Required for every package"* — `[a-z0-9_]`, so the
  shared appendix-A.3 tag grammar renders legal git refnames). Missing
  member pubspecs refuse (`E-DISC-GLOB`, the Maven `<module>` posture).
- **Version strategy**: canonical semver pairs with an editable site;
  everything else is warned (`non-canonical-version` — pub's own
  grammar is three numbers) and never rewritten; a versionless pubspec
  is the documented implicitly-0.0.0 local-only posture
  (`no-version`), `publish_to: none` is the documented
  *"prevent publication anywhere"* opt-out (`publish-disabled`).
- **Internal edges**: the name match (workspace-scoped) and the
  `path:` resolution (everywhere); `ReqSpec::Other` verbatim per
  ADR-0006 — pub constraints (`^2.3.0`, ranges) are never interpreted.
  A path dep landing on a non-member warns (`external-path-dep`) and
  stays external; a path dep outside the tree warns and stays external
  (pub allows arbitrary filesystem paths — the package is never
  refused over one).
- **`dependency_overrides`** is consumed unmodeled with a warning (the
  section pins local sources; the constraint there is not a published
  requirement).
- **Edit seam** (`edit.rs`): M1 carried the interim fail-closed seam
  (the Go/NuGet M1 posture); M2 replaces it with the semantic patcher —
  the patch span must equal the `version` value's span, the inner bytes
  swap right-to-left, the read-back re-parse must carry the target
  (`E-EDIT-VERIFY` posture), the idempotent re-run skips (I1), a
  slotless pubspec refuses (`E-EDIT-PATCHES`). A patch aimed at a
  dependency constraint's span refuses — requirement rewrites never
  touch dart rows.
- **Wire-up**: the orchestrator's fixed dispatch and tree walk read
  every `pubspec.yaml`; the planner's registry arm names `pub.dev` with
  the disabled shape; the apply manifest map routes `pubspec.yaml` →
  `dart`; the tag-target probe's explicit `dart` arm pairs canonical
  spellings (two-component spellings keep the documented HEAD
  fallback); the xtask layer rule + member guard carry the 27-crate
  workspace.
- **Publish posture**: `publishable = false` at discovery (the engine's
  disabled shape); no publish-effect arm, no registry probe arm, no
  auth arm — the planner's registry name is truthful the day an auth
  grammar exists. The single root `pubspec.lock` is pub-generated and
  pub-owned (*"Delete any other existing pubspec.lock"*), so the
  adapter rides the trait's documented `NeedsNative`/`Unsupported`
  declines (the PyPI cell's uv.lock shape).

## Consequences

- **Positive**: a pub workspace plans end-to-end offline — rows, bumps,
  changelogs, tags, the Release-PR flow, the `apply` rewrite, the
  tag-target pinpoint — with the same gates as the other six
  ecosystems; `halyard init`'s refusal and the `E-DISC-NO-PACKAGE`
  page name the seventh family.
- **Recorded-open remainders** (never silently re-scoped):
  the `dart pub publish` argv arm (verified grammar, no automatable
  auth — blocked on a documented credential surface), the pub.dev
  existence-probe arm, the `dart pub get` native lockfile refresh, the
  glob superset beyond the single-`*` form, `pubspec_overrides.yaml`
  handling, and the corpus-lane replay on 2 real repos (the
  acceptance-row half, network-bound, the M7 lane discipline).
- The remaining M8 cells (RubyGems, `generic`, Docker image tags) and
  the Maven cell's Gradle half stay next per plan order.
