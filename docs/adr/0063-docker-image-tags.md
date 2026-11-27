# ADR-0063: the M8 `Docker image tags` cell — the `Dockerfile` grammar, the OCI version-label strategy, and the `docker` row shape

- **Status:** accepted
- **Date:** 2026-11-27
- **Milestone:** §17 YEAR 2, **M8** *"More ecosystems"* — the next
  listed cell after the shipped `generic` cell (ADR-0062), *`Docker
  image tags`*, with the acceptance row *"each: discover/edit/publish/
  resume tests + replay on 2 real repos"*. The reconciliation survey
  found the cell **missing in code** (`grep -rni "docker" crates/
  --include="*.rs"` returns zero code hits; no `halyard-eco-docker`
  crate; no docker research note; the `Ecosystem` enum ends at
  `Generic` + the unused `Custom(Sym)`; `git log --all --oneline -S
  "docker" -i` carries only the docs mentions ADR-0062's row text
  introduced; `git branch -a` shows only the stale merged-ADR branches
  earlier sessions recorded), and ADR-0062's recorded next-start point
  (*"the next M8 cell per plan order (Docker image tags)"*) names it
  next per the M8 row's own list. M1 lands the discovery + planning
  slice (the Dockerfile lexer, the adapter's discovery, the planner
  gates); M2 lands the release half (the semantic edit seam, the
  apply-shell routing, the tag-target probe arm, the publish posture).
  The suffixed/`-f` manifest variants, stage-reachability modeling, the
  argv arm, the docker.io probe, and the corpus replay stay the
  recorded-open remainders (see Consequences).
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

M8's ecosystems are eight cells in: PyPI (ADR-0056), Go (ADR-0057), the
Maven half of Maven/Gradle (ADR-0058), NuGet (ADR-0059), Dart/pub
(ADR-0060), RubyGems (ADR-0061), `generic` (ADR-0062). Every landed
adapter owns a **fixed**, VERIFIED manifest grammar and one documented
version row. The Docker cell's manifest candidate set has exactly one
documented shape worth planning — the `Dockerfile` — and exactly one
documented carrier for the image's **own** version: the OCI Image
Spec's `org.opencontainers.image.version` annotation
(*"version of the packaged software"*, *"version MAY be Semantic
versioning-compatible"*) riding the Dockerfile reference's `LABEL`
instruction (*"The LABEL instruction adds metadata to an image. A LABEL
is a key-value pair."*). The `FROM` row's tag names the **base** image
(*"The FROM instruction initializes a new build stage and sets the base
image"*) — rewriting it would change what the build consumes, not what
it releases, so it is consumed unmodeled. No surveyed competitor ships
a Dockerfile grammar (Knope's escape hatch and release-please's generic
annotations are the vocabularies ADR-0062 already declined to imitate),
so the cell rides the OCI spec — the industry-standard metadata
vocabulary for images. The full quote set and the survey live in
`docs/research/docker-image-tags.md` (VERIFIED 2026-11-27).

## Decision

D1 **— the version site is the OCI version label.** The
`org.opencontainers.image.version` label's value span is the row's
`ManifestSite`; only semver-parseable values are canonical (the
ADR-0056 fail-closed discipline — every other spelling is warned,
unpublishable, no edit site, never rewritten).

D2 **— the identity chain is title → context directory → path.** The
OCI `org.opencontainers.image.title` annotation (*"Human-readable title
of the image (string)"*) names the image when present; a nameless
manifest falls back to the containing directory's name (the documented
build context); a root manifest without a title rides its own path.
Collisions are the orchestrator's per-ecosystem duplicate refusal
(`E-DISC-DUPLICATE`) — no invented namespacing.

D3 **— `FROM` rows are never version sites.** Counted (multi-stage
builds included), consumed unmodeled, never rewritten.

D4 **— the LAST version label is the site.** Docker's documented
override rule (*"If a label already exists but with a different value,
the most-recently-applied value overrides any previously-set value"*)
governs in-file duplicates; superseded rows warn
(`superseded-version-label`). Stage-reachability (a label in an
intermediate stage not based on by the final stage) is the recorded-open
superset — the scanner stays a per-file, offset-preserving grammar.

D5 **— the publish posture is the engine's disabled shape.** Building
and pushing an image spawns the local daemon — no argv grammar exists.
Rows carry `publishable = false`; the planner's publish arm names the
documented default registry host `docker.io` (an unqualified `docker
pull debian` resolves to `docker.io/library/debian:latest`); the engine
records the `publish_disabled` skip; no tool is spawned.

D6 **— recorded-open supersets.** Suffixed/`-f` manifest variants, an
ARG-interpolated label value grammar, the `docker build`/`docker push`
argv arm, the docker.io existence probe, and the corpus-lane replay
stay open — recorded, not silently re-scoped.

## Consequences

- The workspace's 30th member is `halyard-eco-docker` (`halyard-core` +
  `semver`, the §7.2 leaf shape); the xtask layer rule grows to 30
  crates and `halyard-discover`'s adapter list grows to nine (fixed
  dispatch order: cargo → npm → pypi → go → maven → nuget → dart →
  rubygems → docker, then the config-declared `generic` rows).
- The `Dockerfile` walk arm joins `collect_ctx` (the literal documented
  default name, root and nested); `ManifestFormat::Dockerfile` joins
  the error vocabulary; `Ecosystem::Docker` resolves to `"docker"`.
- The tag-target probe gains the `docker` arm with the release half
  (the git engine never parses manifests itself).
- The cell's remainders (D6) join the M8 row's recorded-open set, same
  lane discipline as the seven landed cells' replay and argv arms.

## Links

- `docs/research/docker-image-tags.md` — the VERIFIED quote set.
- ADR-0056 (the fail-closed version strategy), ADR-0058 (the disabled
  publish posture's first shape), ADR-0061 (the per-file package row
  shape), ADR-0062 (the recorded cell order this ADR resumes).
