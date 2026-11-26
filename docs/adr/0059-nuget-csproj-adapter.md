# ADR-0059: the M8 NuGet cell — the `halyard-eco-nuget` adapter, SDK-style csproj planning, and the pack-slot posture

- **Status:** accepted
- **Date:** 2026-11-26
- **Milestone:** §17 YEAR 2, **M8** *"More ecosystems"* — the next
  listed cell after the shipped `Maven/Gradle` half (ADR-0058),
  *"NuGet"*, with the acceptance row *"each:
  discover/edit/publish/resume tests + replay on 2 real repos"*. The
  reconciliation survey found the cell **missing in code**
  (`grep -rni "nuget\|csproj" crates/ --include="*.rs"` returns zero
  hits; no `halyard-eco-nuget`; no research note; `git log --oneline
  -50` and `git branch -a` carry no NuGet work — only the stale
  merged-ADR branches the Go session recorded), and the last PROGRESS
  row's recorded next start point names it first ("the next M8 cell
  per plan order (NuGet)"). M1 lands the discovery + planning slice
  (the csproj scanner, the SDK-style gate, the identity chain, the
  pack-slot version strategy, the `ProjectReference` edges); M2 lands
  the release half (the byte-preserving edit seam, the apply-shell
  routing, the tag-target probe arm, the publish posture). The
  `dotnet` publish argv arm and the corpus replay stay the
  recorded-open remainders (see Consequences), like the Maven cell's
  `mvn deploy` arm.
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

M8's ecosystems are four cells in: PyPI (ADR-0056), Go (ADR-0057), the
Maven half of Maven/Gradle (ADR-0058). The adapter seam
(`halyard_core::discover::EcosystemAdapter`, §7.3) carries five
implementations — cargo (M1.5), npm (M1.6), pypi (M8 step 1), go (M8),
maven (M8) — and new built-ins require an ADR (`Ecosystem`'s
`#[non_exhaustive]` doc). This is that ADR for the sixth adapter.

NuGet differs from every landed ecosystem in one structural way: **the
version grammar is not semver either, and the version is a property,
not a dedicated coordinate row**. VERIFIED quotes in
`docs/research/nuget-grammars.md` (fetched 2026-11-26 from
`learn.microsoft.com`: the NuGet pack-and-restore MSBuild-target
reference, the package-versioning reference, the .NET project-SDK
overview, and the MSBuild customize-by-directory page):

- the SDK declaration: *"The `Project` element has an optional `Sdk`
  attribute that specifies which SDK (and version) to use"*, with the
  top-level `Sdk` element as the documented second form — a project
  file declaring neither may carry its metadata outside the file;
- the pack version: *"`PackageVersion` — Specifies the version that
  the resulting package will have. Accepts all forms of NuGet version
  string. Default is the value of `$(Version)`, that is, of the
  property `Version` in the project"*, and *"Defaults to `Version` if
  not set"* — `VersionPrefix`/`VersionSuffix` are the composite
  fallbacks (*"Setting `PackageVersion` overwrites `VersionPrefix`"*);
- the identity: `PackageId` defaults to *"the `AssemblyName` or
  directory name as the name of the package"*, `AssemblyName` to the
  project file's stem;
- the version grammar: *"The following document follows the Semantic
  Versioning 2.0.0 standard"* — but *"NuGetVersion only requires the
  major segment to be defined. All others are optional... This means
  that `1`, `1.0`, `1.0.0`, and `1.0.0.0` are all accepted and equal"*
  (leading zeroes and a zero fourth segment normalize away; build
  metadata is *"removed"* on resolve) — NuGet versions are
  semver-shaped under NuGet's own normalization, **not** the plan
  contract's semver type;
- the references: *"`<PackageReference Include="..." Version="..." />`*
  ... specify NuGet package dependencies directly within project
  files", the `Version` attribute carrying a NuGet **range** (`6.1`,
  `[6.1.0]`, floating `3.6.*`); *"Project to project references are
  considered by default as NuGet package references"* —
  `<ProjectReference Include="..\Lib\Lib.csproj"/>` names the
  referenced project file;
- conditions: *"Conditions can also be applied at the `ItemGroup`
  level and will apply to all children `PackageReference` elements"*;
- overrides: *"The property might be getting overwritten later"* and
  *"Properties that are set in `Directory.Build.props` can be
  overridden elsewhere in the project file"* — the shared
  `Directory.Build.props`/`Directory.Build.targets` imports are
  evaluation semantics (*"Microsoft.Common.props searches your
  directory structure for the `Directory.Build.props` file"*).

## Decision

### D1 — discovery at the SDK-style .csproj grammar (hand-rolled, offset-preserving)

`halyard_eco_nuget::grammar` is a hand-rolled scanner over the MSBuild
project-file XML (the ADR-0058 pom-scanner discipline: spans point
into the original bytes, whitespace and comments outside captured
values never move, invariant I4; comments/PI/DOCTYPE/CDATA consumed in
place; a comment inside a captured value taints it unplanable — I4).
The scanner captures:

- the SDK declaration (the `Sdk` attribute on `<Project>` or the
  top-level `<Sdk Name>` element) — the SDK-style gate. A `.csproj`
  declaring neither is a classic (non-SDK) project: consumed unmodeled
  with no row (its metadata may live in `packages.config`/`.nuspec`;
  planning it would guess). A conditioned `<Project>` element declines
  the whole file the same way (the file may not apply);
- the `<PropertyGroup>` properties `PackageId`, `AssemblyName`,
  `PackageVersion`, `Version`, `VersionPrefix`, `VersionSuffix`,
  `IsPackable` as value spans (the later declaration wins — the
  documented override rule);
- the `<PackageReference>`/`<ProjectReference>` rows with their
  attribute values and spans (the documented attribute-shaped rows).

A conditioned `PropertyGroup`/property/`ItemGroup`/reference is
consumed unmodeled with an `ignored-section` warning (the value may
not be the pack-time one). No parser dependency joins the tree — the
§7.2 layer rule carries `halyard-core` + `semver` only.

### D2 — package identity is the documented chain; internal edges are path-proven

The identity is `<PackageId>` → `<AssemblyName>` → the manifest file's
stem (the documented default chain, evaluated per project file) — the
same posture as Maven's `artifactId` (colon-free, git-refname-safe).
Internal edges are the **path-proven** ones only: a `ProjectReference`
whose normalized `Include` (backslashes → slashes, `..` climbing,
fail-closed on rooted/escaping paths) names a discovered member's
manifest is the `DepEdge` — *"project to project references are
considered by default as NuGet package references"*. A
`PackageReference` is registry-resolved and never internal (NuGet has
no reactor rule; a package reference could resolve to a published
artifact). A `ProjectReference` to an existing-but-non-member file (a
classic project) is warned and consumed unmodeled; to a missing file
it fails closed (`E-DISC-GLOB`, the Maven `<module>` posture); through
an MSBuild expression (`$(...)`) it is evaluation semantics — warned,
unmodeled. All internal edges are normal edges: NuGet documents no
dev/test scope grammar on `PackageReference`/`ProjectReference`
(Maven's `<scope>test</scope>` mapping has no counterpart here;
inventing one would guess).

### D3 — the version strategy: the pack slot, semver-canonical, fail closed (the ADR-0056 discipline)

The version resolves `PackageVersion` → `Version` (the documented pack
resolution; the slot the plan's edit lands on). Only spellings that
parse as the plan contract's semver are canonical (planned with an
editable site); every NuGet-legal spelling the contract cannot render
— `1.0.0.0`, `1.0`, build metadata — is recognized-but-non-canonical:
warned, unpublishable, no site, never rewritten. A project resolving
through the `VersionPrefix`/`VersionSuffix` composite plans no version
(evaluation semantics a pure discovery pass does not compute — the
Maven `${...}` posture), and a project with no version property at all
is warned `no-version` (imported `Directory.Build.props` values are
not evaluated — the go/maven no-version posture, with the shared
imports warned per project when they exist in the tree).

### D4 — `IsPackable=false` keeps the version and drops the pack

`<IsPackable>false</IsPackable>` (the documented default is `true`)
means the project cannot pack: the row keeps its version (a release
tag still plans) and warns (`not-packable`). It is one input to the
row's `publishable = false` posture (D5), never a guess about pack
semantics.

### D5 — the release half: the edit seam, the apply routing, the probe arm, the publish posture

- `edit_versions` re-applies the plan's own-version byte patches
  semantically and in place (invariant I4): each patch span must land
  on the pack slot's value span (`PackageVersion` when present, else
  `Version`); the inner bytes swap for the plan's target, the
  surrounding markup/indentation never moves, and a re-parse read-back
  verifies the project now carries the target before the patch is
  accepted (`E-EDIT-PATCHES`/`E-EDIT-VERIFY` otherwise). A patch aimed
  at the `Version` span while `PackageVersion` owns the slot refuses
  (pack consumes `PackageVersion`); a project with no slot refuses.
- `halyard-edit`'s manifest map routes any `*.csproj` → the `nuget`
  adapter (the `pom.xml` routing precedent), and the tag-target
  pinpoint walk's manifest probe gains the explicit `nuget` arm
  (`halyard_eco_nuget::manifest_version` — the semver-canonical
  pairing text, `None` for the non-canonical spellings, which keep the
  documented HEAD fallback).
- A NuGet release row carries a publish decision with the engine's
  disabled shape: `publishable = false` at discovery (halyard owns no
  NuGet publish grammar — there is no `dotnet` argv arm, no nuget.org
  probe and no auth arm; inventing one would violate the VERIFY rule —
  the Maven D5 posture), so `plan_publication` records the member as a
  `publish_disabled` skip and no tool is ever spawned. The planner's
  ecosystem→registry mapping gains the `Nuget` arm (`nuget.org`, the
  ecosystem's canonical registry) so the decision field is truthful
  the day the argv arm lands — flipping `publishable` is the whole
  change.

### D6 — layering and wire-up

- New crate `halyard-eco-nuget` (layer rule in `xtask/src/layers.rs`:
  workspace deps `halyard-core` only; external `semver` — the csproj
  grammar is hand-rolled, no new parser dependency).
- `halyard-discover` gains the adapter in the fixed dispatch order
  (after maven) and the tree walk reads every `*.csproj`.
- `Ecosystem::Nuget` joins core (`resolve` → `nuget`,
  `is_tag_only` → `false`); `ManifestFormat::Csproj` names the grammar
  in discovery errors; no plan-schema change (the frozen wire format
  of ADR-0055 carries NuGet rows unchanged — the ecosystem string is
  data).

## Consequences

- Sixth adapter live end-to-end: discovery → workspace rows → plan
  (canonical versions, cascade) → release rows with byte-preserving
  csproj edits and an honest publish posture; the conformance
  discipline (unit + contract + example + bench + CLI e2e) matches the
  cargo/npm/pypi/gomod/maven pattern.
- The M8 NuGet cell is **partial** by design: the discovery/plan/edit
  halves ship in-tree green; the `dotnet pack`/`dotnet nuget push`
  argv arm, the nuget.org flat-container probe, the auth arm and the
  corpus replay on 2 real repos stay recorded-open (PROGRESS row), like
  the PyPI/Go/Maven cells' remainders. The cell's
  non-`.csproj` project types (`vbproj`/`fsproj`) are warned-unmodeled
  shapes (recorded-open), and CPM (`Directory.Packages.props`) rows
  ride verbatim.
- The stability contract (ADR-0055) is untouched: the plan schema
  carries no new keys, and no new error code was needed (discovery/edit
  reuse the `E-DISC-*`/`E-EDIT-*` pages; `ManifestFormat` gains the
  `Csproj` variant its error reports name).
- The manifest-family refusal texts (`halyard init`'s no-manifests
  message, `E-DISC-NO-PACKAGE`'s display and its docs page) name the
  csproj grammar now that a nuget-only repository is plannable (docs
  drift fixed with the seam, the ADR-0057/0058 convention).
