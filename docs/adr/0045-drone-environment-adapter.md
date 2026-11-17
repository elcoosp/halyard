# ADR-0045: the Drone step-scoped environment adapter

- **Status:** accepted
- **Date:** 2026-11-17
- **Milestone:** M10 step 2 continued — the `drone` refused-unknown pin
  ADR-0041 left behind becomes the twelfth recorded system under the
  same closed seam
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

ADR-0041 closed the recorded CI surface at eleven systems and left
`drone` as the refused-unknown pin — "an unrecorded system, so no
adapter follows from it". Drone CI consumes machine-readable step
definitions through the documented `.drone.yml` pipeline shape, VERIFIED
per §0.4 (fetched quotes in
`docs/research/ci-selector-grammars.md`, checked 2026-11-17):

- **Steps are the unit of work**: `steps:` sits at the pipeline's top
  level and each entry carries `name`, `image` and `commands` — "NB
  'commands' overrides the docker entrypoint" (docs.drone.io/pipeline/
  docker/syntax/steps/).
- **Per-step environment is a documented first-class mechanism**:
  "Drone provides the ability to define environment variables scoped to
  individual build steps" — the `environment` key sits under an
  individual step as a name→value mapping (the docs' own example maps
  `GOOS: linux` / `GOARCH: amd64` beside a step's `commands:`), and
  "the environment section cannot expand environment variables or
  evaluate shell expressions" (docs.drone.io/pipeline/environment/
  syntax/). Unlike Bitbucket, whose `variables` option is a
  `pipe`-only property (ADR-0041), Drone *does* expose a per-step env
  surface — so the assignment does not have to ride a script line.
- **Parallelism is the consumer's policy**: steps are "executed
  sequentially by default"; parallel execution is the documented
  `depends_on` DAG ("You can optionally describe your build steps as a
  directed acyclic graph"), with the rule "when you define the
  dependency graph you must configure dependencies for all pipeline
  steps" (docs.drone.io/pipeline/docker/syntax/parallelism/). Drone
  computes no dependency closure and has no matrix grammar in the 1.x
  documented surface.

## Decision

- **One impl under the unchanged ADR-0034 seam** — the
  `CiSelectorAdapter` trait, the `SelectedPackage` inputs and the
  empty-renders-nothing contract carry over untouched; the new system
  is one impl + one lookup entry + one CLI value-enum variant, exactly
  the extension path the closed surface anticipated.
- **Drone (`drone`) emits one step-scoped `environment:` fragment per
  affected package, name-sorted** — the documented per-step env
  mapping, the halyard-owned identifier inside:

  ```yaml
  environment:
    HALYARD_PKG: app
  ```

  The consumer pastes one fragment under one step per package (the
  step's own indentation applies — the ADR-0039 Buildkite convention)
  and reads `$HALYARD_PKG` in that step's `commands`. Drone computes no
  dependency closure of its own, so the fragments over the affected
  closure encode the **full affected closure** (the ADR-0035 rule:
  closure-vs-roots is a property of the consuming system).
- **`HALYARD_PKG` is the halyard-owned identifier spelled upper-snake
  like the docs' own `DRONE_*` variable style** — the
  ADR-0035/0036/0038/0039/0040/0041 namespaced-identifier policy
  carried over; it cannot collide with the pipeline's own variables or
  the documented `DRONE_*` metadata variables Drone injects.
- **The value is verbatim-safe and unquoted**: the documented
  no-expansion rule ("the environment section cannot expand environment
  variables or evaluate shell expressions") makes the mapping value a
  literal, and neither package-name grammar the selection carries
  (cargo's `[A-Za-z0-9_-]`, npm's URL-safe set) contains a shell
  metacharacter — the bare-word value is emitted verbatim.
- **The render stops at the step fragment**: the surrounding
  `kind:`/`type:`/`name:` pipeline envelope, each step's `name`/`image`/
  `commands` and the tool invocation are the consumer's own — halyard
  renders *the one expression per step* (the ADR-0034 seam renders the
  selection, not the pipeline). Emitting a full step would guess the
  consumer's image and commands; the fragment never guesses.
- **The sequential-by-default and DAG rules are the consumer's policy**:
  the adapter renders one fragment per package; whether the consumer
  runs the steps sequentially or wires the `depends_on` graph (and the
  all-steps-must-declare rule that graph carries) is their pipeline's
  shape, never the adapter's guard (the ADR-0039 posture).
- **Alternatives considered and rejected**: one full step entry per
  package (`- name: halyard-app …`) — incomplete without an `image`
  and `commands` (which would be guesses, violating the never-a-guess
  rule), and it invents step names that can collide with the
  consumer's own; a single `environment` fragment mapping all packages
  into one array value — Drone's `environment` maps scalar strings
  (the docs' examples are scalars), and a composed array value would
  push the iteration onto the consumer's shell, which the documented
  grammar does not express.
- **The lookup stays closed**: `ci_selector_adapter` names the twelve
  systems; anything else is `None`, and extending one is still an ADR
  (`travis` is the refused-unknown pin the contract tests carry now —
  an unrecorded system, so no adapter follows from it).

## Consequences

- `halyard affected --ci drone` writes the step-scoped `environment:`
  fragments a Drone pipeline consumes verbatim; no re-shaping happens
  consumer-side beyond pasting each fragment under its step and
  keeping the tool invocation in the same step's `commands`.
- The render cost stays pinned free (the `affected_ci` bench grows the
  `ci_selector_drone_40chain` case over the same 40-package chain —
  nanosecond-scale, like the eleven before it).
- The empty-selection contract and the refused-unknown pin move one
  row over: the contract-test loop names twelve systems, `travis` is
  the refused unknown.
- The recorded CI-adapter surface (ADR-0034 → ADR-0045) is complete:
  every recorded system renders. A new system means a new grammar
  VERIFY + ADR, as always.
