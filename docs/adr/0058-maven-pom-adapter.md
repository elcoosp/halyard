# ADR-0058: the M8 Maven cell — the `halyard-eco-maven` adapter, pom.xml planning, and the parent-inheritance posture

- **Status:** accepted
- **Date:** 2026-11-26
- **Milestone:** §17 YEAR 2, **M8** *"More ecosystems"* — the next
  listed cell after PyPI (ADR-0056) and Go (ADR-0057),
  *"Maven/Gradle"*, with the acceptance row *"each:
  discover/edit/publish/resume tests + replay on 2 real repos"*. The
  reconciliation survey found the cell **missing in code**
  (`grep -rni "maven\|gradle" crates/ --include="*.rs"` returns zero
  hits; no `halyard-eco-maven`; no research note; no branch carries
  Maven work — only the stale merged-ADR branches the Go session
  recorded), and the last PROGRESS row's recorded next start point
  names it first. M1 lands the discovery + planning slice (the
  pom.xml scanner, coordinates, modules, parent inheritance, the
  semver-canonical version strategy); M2 lands the release half (the
  byte-preserving edit seam, the apply-shell routing, the tag-target
  probe arm, the publish posture). The **Gradle** half of the cell and
  the `mvn deploy` publish argv arm stay the recorded-open remainders
  (see Consequences), like the PyPI cell's `uv.lock` half.
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

M8's ecosystems are three cells in: PyPI (ADR-0056), Go (ADR-0057).
The adapter seam (`halyard_core::discover::EcosystemAdapter`, §7.3)
carries four implementations — cargo (M1.5), npm (M1.6), pypi (M8 step
1), go (M8) — and new built-ins require an ADR (`Ecosystem`'s
`#[non_exhaustive]` doc). This is that ADR for the fifth adapter.

Maven differs from every landed ecosystem in one structural way: **the
version grammar is not semver**. VERIFIED quotes in
`docs/research/maven-pom-grammars.md` (fetched 2026-11-26 from
`maven.apache.org/pom.html` and the `maven-artifact` `ComparableVersion`
javadoc):

- *"groupId:artifactId:version are all required fields (although,
  groupId and version do not need to be explicitly defined if they are
  inherited from a parent)"* — the coordinate identity;
- children inherit *"groupId, version, ... dependencies"* from the
  parent POM, while *"artifactId"* is *"not inherited"*;
- aggregation: *"A pom packaged project may aggregate the build of a
  set of projects by listing them as modules, which are relative paths
  to the directories or the POM files of those projects"*, and *"'The
  <packaging> type is required to be pom for parent and aggregation
  (multi-module) projects'"*;
- the parent lookup rule: *"Without it, Maven looks in ../pom.xml, and
  uses the POM it finds there only if its coordinates match the
  <parent> element"*;
- version comparison: `ComparableVersion` is a *"[g]eneric
  implementation of version comparison"* over *"mixing of '-' (hyphen)
  and '.' (dot) separators"* with *"well-known qualifiers"* (`snapshot`
  among them) — Maven versions are arbitrary strings under Maven's own
  comparator, **not** semantic versions.

## Decision

### D1 — discovery at the pom.xml grammar (hand-rolled, offset-preserving)

`halyard_eco_maven::grammar` is a hand-rolled scanner over the pom.xml
subset halyard plans (the ADR-0001 JSON-scanner precedent — no new
parser dependency, spans point into the original bytes, comments and
the XML declaration skipped in place, unknown/corrupt shapes fail
closed `E-DISC-MANIFEST` via the new `ManifestFormat::Pom`). It reads
the `<project>` children halyard needs: `groupId`/`artifactId`/
`version`/`packaging`, the `parent` block, `modules`/`module`, and
`dependencies`/`dependency` rows. Property references (`${...}`) inside
a version value are recognized-but-non-canonical (D3), never resolved —
resolving them would mean evaluating build semantics, which a pure
discovery pass does not do.

- A root `pom.xml` with `<modules>` discovers its members (module
  entries normalized repo-relative; a directory entry appends
  `pom.xml`, a `.xml` entry is taken verbatim; an absolute or
  tree-escaping path is a fail-closed `E-DISC-GLOB`). Every named
  member must carry its pom in the committed tree, and the member's
  `<parent>` coordinates must name the aggregator (the documented
  coordinates-match rule — a mismatch refuses `E-DISC-MANIFEST`
  instead of guessing the parentage). Nested poms no module claims are
  skipped (the orphan-manifest posture, ADR-0057's).
- No `<modules>` (or no root pom beyond a single one): the single
  package posture (§6.1). A repository with no `pom.xml` yields no
  maven rows (the other adapters' posture — `E-DISC-NO-PACKAGE` stays
  the orchestrator's own refusal when nothing at all is plannable).

### D2 — package identity is the `artifactId`; internal edges are coordinate-proven

The package name is the member's `artifactId` (colon-free, so the
shared appendix-A.3 tag grammar renders legal git refnames — git
refnames cannot carry `:`). A `<dependency>` row is a proven-internal
edge only when its (groupId, artifactId) pair names a discovered
member; the row's name is the matched member's `artifactId`, and the
orchestrator's name-keyed resolution needs no change. Two members with
one `artifactId` (different groupIds) refuse as `E-DISC-DUPLICATE`
before any edge is resolved — the shared name-keyed group/tag grammar
requires artifactId uniqueness in one workspace (the cargo posture),
and silently grouping them would plan ambiguous tags.

### D3 — the version strategy: semver-canonical, fail closed (the ADR-0056 discipline)

The plan contract is semver-typed (§5.5) and Maven's is not: the
adapter applies exactly the PEP 440 discipline of ADR-0056. A
`<version>` that parses as semver is canonical — planned with
`RawVersionSource::Local` and an edit site on the value's inner span —
**whatever Maven calls it**: the qualifier forms are semver prerelease
identifiers (`1.2.3-SNAPSHOT`, `1.0-alpha-1` both parse; the
contract's type governs, not Maven's workflow names — the maintainer
releases the spelling, halyard never rewrites it). Every other
Maven-legal spelling is recognized-but-non-canonical: two-component
`1.0` (never padded to `1.0.0` — the rewrite would be a guess),
`2022.01.1`, property references — **warned, unpublishable, no edit
site, never rewritten** (a `non-canonical-version` warning names the
spelling; the package still versions-by-inheritance and cascades, it
just plans no release target of its own). Dependency requirement text
rides `ReqSpec::Other` verbatim (ADR-0006) — the semver rewriter never
touches a Maven row (Maven tooling owns them), and the cascade still
bumps dependents.

### D4 — parent inheritance rides the existing workspace-inheritance machinery

A member pom without its own `<version>` inherits the parent's (the
documented inheritance list): the row carries
`RawVersionSource::WorkspaceInherited { file: <parent pom>, key_path:
"project.version" }` with the version site on the **parent** pom's
value span — the cargo `[workspace.package].version` posture (one
edit, all inheritors lockstep). The parent pom itself is a package
when it carries coordinates (a pom-packaging artifact is a real
artifact); when it carries `<modules>` it is additionally the
aggregator whose members were listed above.

### D5 — the release half: the edit seam, the apply routing, the probe arm, the publish posture

- `edit_versions` re-applies the plan's own-version byte patches
  semantically and in place (invariant I4): each patch span must land
  on a `<project>`-owned `<version>` element's text of the owning pom;
  the inner bytes swap for the plan's target, the surrounding
  markup/indentation never moves, and a re-parse read-back verifies the
  pom now carries the target before the patch is accepted
  (`E-EDIT-PATCHES`/`E-EDIT-UNREWRITABLE` otherwise). Inherited
  members have no own site — the one parent-pom edit covers them, and
  a patch aimed at an inheritor's manifest fails closed.
- `halyard-edit`'s manifest map routes `pom.xml` → the `maven` adapter
  (the go.mod routing precedent), and the tag-target pinpoint walk's
  manifest probe gains the explicit `maven` arm
  (`halyard_eco_maven::manifest_version` — the semver-canonical pairing
  text, `None` for the non-canonical spellings, which keep the
  documented HEAD fallback).
- A Maven release row carries a publish decision with the engine's
  disabled shape: `publishable = false` at discovery (halyard owns no
  Maven publish grammar — there is no `mvn` argv arm, no registry
  probe and no auth arm; inventing one would violate the VERIFY rule),
  so `plan_publication` records the member as a `publish_disabled`
  skip and no tool is ever spawned. The planner's ecosystem→registry
  mapping gains the `Maven` arm (`maven-central`, the ecosystem's
  canonical registry) so the decision field is truthful the day the
  argv arm lands — flipping `publishable` is the whole change.

### D6 — layering and wire-up

- New crate `halyard-eco-maven` (layer rule in `xtask/src/layers.rs`:
  workspace deps `halyard-core` only; external `semver` — the pom.xml
  grammar is hand-rolled, no new parser dependency).
- `halyard-discover` gains the adapter in the fixed dispatch order
  (after go) and the tree walk reads `pom.xml` (root + nested).
- `Ecosystem::Maven` joins core (`resolve` → `maven`,
  `is_tag_only` → `false`); `ManifestFormat::Pom` names the grammar in
  discovery errors; no plan-schema change (the frozen wire format of
  ADR-0055 carries Maven rows unchanged — the ecosystem string is
  data).

## Consequences

- Fifth adapter live end-to-end: discovery → workspace rows → plan
  (canonical versions, parent lockstep, cascade) → release rows with
  byte-preserving pom edits and an honest publish posture; the
  conformance discipline (unit + contract + example + bench + CLI e2e)
  matches the cargo/npm/pypi/gomod pattern.
- The M8 Maven cell is **partial** by design: the Maven half ships
  in-tree green; the **Gradle** half stays recorded-open — a
  `build.gradle(.kts)` version is arbitrary Groovy/Kotlin build code
  (an assignment, a closure, a `gradle.properties` lookup, a plugin
  convention); extracting one without an evaluated, VERIFIED grammar
  would guess, and halyard's discipline forbids guessing. The corpus
  replay on 2 real repos and the `mvn deploy` argv arm stay
  recorded-open too (PROGRESS row), like the PyPI/Go cells' remainders.
- The stability contract (ADR-0055) is untouched: the plan schema
  carries no new keys, and no new error code was needed (discovery/edit
  reuse the `E-DISC-*`/`E-EDIT-*` pages; `ManifestFormat` gains the
  `Pom` variant its error reports name).
- The manifest-family refusal texts (`halyard init`'s no-manifests
  message, `E-DISC-NO-PACKAGE`'s display and its docs page) name the
  pom grammar now that a maven-only repository is plannable (docs
  drift fixed with the seam, the ADR-0057 convention).
