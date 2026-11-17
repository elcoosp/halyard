# ADR-0036: Azure Pipelines / TeamCity CI adapters (variable & matrix-parameter grammars)

- **Status:** accepted
- **Date:** 2026-11-17
- **Milestone:** M10 step 2 continued — the ADR-0034/0035 "More CI
  systems (Azure Pipelines variables, TeamCity) join by adding an impl
  + an enum variant" consequence, landed under the same closed seam
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

ADR-0034 closed the `CiSelectorAdapter` seam; ADR-0035 added the two
matrix-job grammars (GitHub Actions output values, GitLab
`parallel:matrix`). The recorded next work named the remaining
systems: Azure Pipelines (variables) and TeamCity. Both consume
**machine-readable selections** rather than argv, and both grammars
are VERIFIED per §0.4 (fetched quotes in
`docs/research/ci-selector-grammars.md`): Azure's
`##vso[task.setvariable …]` logging command plus its documented
`split(…, ',')` + `${{ each }}` consumption, and TeamCity's Matrix
Build feature iterating `param("env.HALYARD_PKG", listOf(value(…),
…))` entries.

## Decision

- **Two impls under the unchanged ADR-0034 seam** — the
  `CiSelectorAdapter` trait, the `SelectedPackage` inputs and the
  empty-renders-nothing contract carry over untouched; each new system
  is one impl + one lookup entry + one CLI value-enum variant, exactly
  the extension path the closed surface anticipated.
- **Azure Pipelines (`azure`)** emits **one
  `##vso[task.setvariable variable=HALYARD_PKGS;isOutput=true]`
  logging command whose value is the affected closure, name-sorted and
  comma-joined**: the command is documented to "set a variable in the
  variable service of taskcontext" that "following tasks are able to
  use", the cross-job reference is
  `dependencies.<job-name>.outputs['<step-name>.<variable-name>']`,
  and the documented consumption of a comma-joined variable is the
  `split(variables.environments, ',')` example iterated with the
  documented `${{ each }}` keyword. Pipelines computes no dependency
  closure of its own, so the variable encodes the **full affected
  closure**. `HALYARD_PKGS` is the halyard-owned identifier (the
  ADR-0035 policy carried over: namespaced so it cannot collide with
  the pipeline's own variables). The command's escape sequences
  (`;`→`%3B`, `\n`→`%0A`, `\r`→`%0D`, `]`→`%5D`, `%`→`%AZP25`) never
  fire — neither package-name grammar (cargo's `[A-Za-z0-9_-]`, npm's
  URL-safe set) contains an escapable character, so the value rides
  verbatim.
- **TeamCity (`teamcity`)** emits **one
  `param("env.HALYARD_PKG", listOf(value(…), …))` line over the
  affected closure**: the Matrix Build feature "enables you to define
  a collection of builds by iterating over specified parameter values
  and generating a build for every combination", the documented DSL
  entry is `param("Browser", listOf(value("Chrome"), value("Firefox")))`,
  and the documented sample iteration set carries an `env.*` parameter
  (`env.ShouldFail: true, false`). The consumer pastes the line inside
  its `matrix { }` block and each matrix instance reads `$HALYARD_PKG`
  in the script — the ADR-0035 `HALYARD_PKG` identifier policy carried
  over. The Kotlin string literal never needs escaping: neither
  package-name grammar contains a quote.
- **Closure-vs-roots is a property of the consuming system, not of the
  adapter** (the ADR-0035 rule): tools that compute dependencies
  themselves (turbo, bazel) receive the change roots; tools that
  compute nothing (nx, gha, gitlab, azure, teamcity) receive the full
  affected closure. The trait's two inputs carry both without a flag.
- **The lookup stays closed**: `ci_selector_adapter` names the seven
  systems; anything else is `None`, and extending one is still an ADR
  (`circleci` is the current refused-unknown pin in the contract
  tests).

## Consequences

- `halyard affected --ci azure` writes the output-variable line a
  pipeline iterates verbatim; `halyard affected --ci teamcity` writes
  the `matrix { }` block entry verbatim. No re-shaping happens
  consumer-side beyond the systems' own documented reference syntax.
- The render cost stays pinned free (the `affected_ci` bench grows the
  `ci_selector_azure_40chain` / `ci_selector_teamcity_40chain` cases
  over the same 40-package chain; both stay sub-microsecond).
- Neither adapter needs a serialization dependency: the azure value is
  a comma-join (the documented delimiter), the teamcity line is the
  documented DSL spelling — no JSON/YAML escaping is owned by the
  producer (unlike the gha adapter, whose array escapes through
  `serde_json`).

## Alternatives considered

- **Emit a JSON array for Azure (the gha shape) consumed via
  `fromJSON`.** Rejected: Azure's documented consumption of a runtime
  variable is the `split(…, ',')` + `${{ each }}` example; `fromJSON`
  is documented for template *expressions*, while a
  `task.setvariable` value is a string — the comma-join is the shape
  the docs themselves demonstrate end to end.
- **Emit `##teamcity[setParameter name='env.HALYARD_PKGS' value='…']`
  service messages for TeamCity.** Rejected: `setParameter` updates
  build parameters "available in the build steps following the
  modifying one" and in dependent builds via `%dep.*%` — a per-package
  fan-out needs build-configuration-level iteration, which the Matrix
  Build feature owns (and the same page documents that
  "Reverse dependency parameters, reverse.dep.*, do not work correctly
  in matrix builds", so the fan-out is the feature's, not a script's,
  to shape). The service-message path is recorded in the research note
  as the runtime companion.
- **One `value("…")` atom per package instead of the joined
  `param(...)` line.** Rejected: the matrix block consumes a
  `param(name, listOf(…))` entry, not bare values; the joined line is
  the one paste-able unit and matches the house "one expression the CI
  system consumes" shape (nx's `--projects=a,b`, gha's one array).
- **A generated pipeline file.** Rejected: the adapters render the
  selection, not the consumer's pipeline — the CI step owns its job
  definition (the ADR-0034 "no re-parsing" boundary, kept).
