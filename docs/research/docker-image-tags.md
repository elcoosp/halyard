# Docker image tags — the grammar research note (M8, ADR-0063)

> Verification status: **VERIFIED 2026-11-27** — every quoted line below
> was read from the linked, live documentation at session time. Quotes
> are verbatim; the decisions follow the quotes.

The M8 row's `Docker image tags` cell needs a documented grammar for
"where a Docker image's release version lives in the repository". This
note surveys the two candidate carriers (the `FROM` row's tag and the
`LABEL`-carried OCI annotations) and fixes the cell's scope decisions.

## 1. The carriers

### 1.1 The `FROM` row's tag — the **base** image's version, not this package's

The Dockerfile reference
(<https://docs.docker.com/reference/dockerfile/>, the `FROM` section):

> The FROM instruction initializes a new build stage and sets the base
> image for subsequent instructions.

> The tag or digest values are optional. If you omit either of them, the
> builder assumes a latest tag by default.

A `FROM alpine:3.20` row names the image **built upon** — bumping the
literal would change what the build consumes, not what it releases. It
is a dependency-like row at somebody else's version, so the cell
consumes `FROM` rows unmodeled and **never** treats them as a version
site (the same discipline that keeps Maven's `<parent>` versions at one
shared edit site rather than a dependency bump, ADR-0058).

### 1.2 The OCI version annotation carried by `LABEL` — this package's version

The OCI Image Spec's annotations table
(<https://raw.githubusercontent.com/opencontainers/image-spec/main/annotations.md>,
the "Annotations" section) defines the canonical version key:

> **org.opencontainers.image.version** version of the packaged software
>   - The version MAY match a label or tag in the source code repository
>   - version MAY be [Semantic versioning-compatible](https://semver.org/)

The Dockerfile reference's `LABEL` section documents how the annotation
is carried in a manifest:

> `LABEL <key> = <value> [ <key> = <value>... ]` The LABEL instruction
> adds metadata to an image. A LABEL is a key-value pair.

> An image can have more than one label. You can specify multiple labels
> on a single line.

Value quoting:

> To include spaces within a LABEL value, use quotes and backslashes as
> you would in command-line parsing.

> Note — Be sure to use double quotes and not single quotes.
> Particularly when you are using string interpolation (e.g.
> `LABEL example="foo-$ENV_VAR"`), single quotes will take the string as
> is without unpacking the variable's value.

The reference's own examples establish the pair grammar halyard's
scanner implements — spaces around `=`, quoted keys, multi-pair
instructions, and line continuations joining a logical instruction:

> ```
> LABEL "com.example.vendor" = "ACME Incorporated"
> LABEL com.example.label-with-value = "foo"
> LABEL version = "1.0"
> LABEL multi.label1 = "value1" multi.label2 = "value2" other = "value3"
> ```

### 1.3 Duplicate rows — Docker's own override rule

The `LABEL` section documents what happens when a label is set twice:

> Labels included in base images (images in the `FROM` line) are
> inherited by your image. If a label already exists but with a
> different value, the most-recently-applied value overrides any
> previously-set value.

In-file duplicates resolve the same way (each `LABEL` instruction
applies in order), so the **last** `org.opencontainers.image.version`
row is the version site; superseded rows are observable warnings, not
errors. (Stage-reachability — *"In a multi-stage build, labels from
intermediate stages are only present in the final image if the final
stage is directly or indirectly based on them"* — is the recorded-open
superset, D6.)

### 1.4 The default registry host

The `docker image pull` CLI reference
(<https://docs.docker.com/reference/cli/docker/image/pull/>) documents
the unqualified-name resolution:

> If no tag is provided, Docker Engine uses the `:latest` tag as a
> default. This example pulls the `debian:latest` image: … Status:
> Downloaded newer image for `debian:latest` … `docker.io/library/debian:latest`

An unqualified pull resolves to `docker.io/...` — the canonical
registry host the planner's publish arm names (truthful the day an
argv arm flips `enabled`).

### 1.5 The default manifest name

The Dockerfile reference's format section fixes the file the builder
reads (the documented build input):

> Here is the format of the Dockerfile:
>
> ```
> # Comment INSTRUCTION arguments
> ```

`docker build PATH` reads `PATH/Dockerfile`; a `-f`-named or suffixed
variant (`Dockerfile.prod`) is buildable input but is not the
documented default — the cell scans the literal `Dockerfile` name (root
and nested) and records the variants as the open superset (D6).

Instruction case and comments (the format section):

> The instruction is not case-sensitive. However, convention is for them
> to be UPPERCASE to distinguish them from arguments more easily.

> Comment lines are removed before the Dockerfile instructions are
> executed.

## 2. Prior art

None of the surveyed competitors ships a Dockerfile grammar: Knope's
`versioned_files` set has no Docker entry (its escape hatch is the same
`Generic` regex locator halyard's `[[generic]]` cell rode, ADR-0062);
release-please's updater set has no Dockerfile updater (its generic
`x-release-please-*` annotation vocabulary is the one ADR-0062 chose to
avoid inventing against). The cell therefore follows the OCI spec — the
industry-standard metadata vocabulary for images — rather than any
competitor's private convention.

## 3. Decisions (carried into ADR-0063)

1. **D1 — the version site is the OCI version label.**
   `org.opencontainers.image.version` is the documented *"version of the
   packaged software"*; the label's value span is the edit site; only
   semver-parseable values are canonical (the ADR-0056 fail-closed
   discipline — every other spelling is warned, unpublishable, never
   rewritten).
2. **D2 — the identity chain is title → context directory → path.**
   The OCI `org.opencontainers.image.title` annotation (*"Human-readable
   title of the image (string)"*) names the image when present; a
   nameless manifest falls back to the containing directory's name (the
   documented build context), and a root manifest without a title rides
   its own path. Collisions are the orchestrator's per-ecosystem
   duplicate refusal (`E-DISC-DUPLICATE`).
3. **D3 — `FROM` rows are never version sites.** The tag names the base
   image; consumed unmodeled, never rewritten.
4. **D4 — the LAST version label is the site.** Docker's documented
   override rule governs in-file duplicates; superseded rows warn
   (`superseded-version-label`).
5. **D5 — the publish posture is the engine's disabled shape.** Building
   and pushing an image spawns the local daemon — no argv grammar
   exists; rows carry `publishable = false`, the planner's registry arm
   names `docker.io` (§1.4), the engine records the `publish_disabled`
   skip, no tool is spawned.
6. **D6 — recorded-open supersets.** Suffixed/`-f` manifest variants,
   stage-reachability modeling (multi-stage labels only reaching the
   final image through a `FROM`-based chain), an ARG-interpolated label
   value grammar, the `docker build`/`docker push` argv arm, the
   docker.io existence probe, and the corpus-lane replay stay open —
   recorded, not silently re-scoped.

## 4. Links

- Dockerfile reference — format, `FROM`, `LABEL` sections:
  <https://docs.docker.com/reference/dockerfile/>
- OCI Image Spec, annotations:
  <https://github.com/opencontainers/image-spec/blob/main/annotations.md>
- `docker image pull` reference (the `docker.io` default):
  <https://docs.docker.com/reference/cli/docker/image/pull/>
