# Research: CI selector grammars (VERIFY, M10 step 2)

**Checked:** 2026-11-16 · Sources fetched live per §0.4 (the Azure
Pipelines and TeamCity sections fetched 2026-11-17 for ADR-0036; the
CircleCI section fetched 2026-11-17 for ADR-0038).

## Turborepo — <https://turborepo.dev/docs/crafting-your-repository/running-tasks>

- "To limit the scope to a package and its dependencies, append `...`
  to the package name." (the `pkg...` direction — dependencies)
- Dependents: "When you're working on a specific package, you might
  want to run tasks for the package and its dependents" — the
  `--filter=...ui` example (the `...pkg` direction halyard emits).
- "Multiple filters are combined as a union".

## Nx — <https://nx.dev/docs/reference/nx-commands>

- `run-many --projects`: "Projects to run. (comma/space delimited
  project names and/or patterns)." No `--with-deps` option exists on
  `run-many`; dependents-first scheduling is the default
  (`--excludeTaskDependencies` "Skips running dependent tasks first."
  default `false`).

## Bazel — <https://bazel.build/query/language>

- `rdeps(u, x)`: "evaluates to the reverse dependencies of the argument
  set x within the transitive closure of the universe set u".
- `union`: part of the set-operations production; "The `intersect` (`^`)
  and `union` (`+`) operations are commutative (symmetric)."
- `foo/...`: "a target pattern that evaluates to a set containing all
  rules in every package recursively beneath the `foo` directory".

## GitHub Actions — <https://docs.github.com/en/actions/writing-workflows/choosing-what-your-workflow-does/running-variations-of-jobs-in-a-workflow>

- "You can use the output from one job to define matrices for multiple
  jobs." (the ADR-0035 `gha` adapter's emission shape: the output
  value)
- The defining job writes the values via:
  `echo 'colors=["red", "green", "blue"]' >> "$GITHUB_OUTPUT"` — the
  output value is the **bare JSON array**.
- The downstream job consumes it inside its own matrix with
  `color: ${{ fromJSON(needs.define-matrix.outputs.colors) }}` — the
  consumer names the matrix dimension (`color`), not the producer.
- "Within your matrix, define one or more variables followed by an
  array of values."

## GitLab — <https://docs.gitlab.com/ci/yaml/> ("parallel:matrix")

- "Use parallel:matrix to run a job multiple times in parallel in a
  single pipeline, but with different variable values for each
  instance of the job."
- "Supported values: An array of hashes of variables: The matrix
  identifiers, which become the variable names, can use only numbers,
  letters, and underscores (`_`). The values must be either a string,
  or an array of strings. The number of permutations cannot exceed
  200."
- The documented example expands two hashes into seven jobs
  (`deploystacks: [aws, monitoring]` …), the variables readable in the
  script (`environment: $PROVIDER/$STACK`) — a one-dimensional matrix
  is one hash per instance.

## Azure Pipelines — <https://learn.microsoft.com/en-us/azure/devops/pipelines/scripts/logging-commands> + <https://learn.microsoft.com/en-us/azure/devops/pipelines/process/expressions>

- The emission command (Logging commands, "SetVariable", VERIFIED):
  `##vso[task.setvariable variable=shouldrun;isOutput=true]true` —
  "Sets a variable in the variable service of taskcontext. The first
  task can set a variable, and following tasks are able to use the
  variable." Properties: "`variable` = variable name (Required)",
  "`isOutput` = boolean (Optional, defaults to false)".
- The cross-job consumption (Expressions, "Dependencies", VERIFIED):
  "Reference an output variable in a different job in the same stage"
  — Syntax: `dependencies.<job-name>.outputs['<step-name>.<variable-name>']`
  (the documented example pairs the exact same `setvariable …
  isOutput=true` emission with the reference).
- The comma-joined consumption (Expressions, `split`, VERIFIED):
  "Splits a string into substrings based on the specified delimiting
  characters" — the documented example iterates a comma-joined
  variable with the loop keyword: `variables: environments: prod1,prod2`
  → `- ${{ each env in split(variables.environments, ',')}}: - script:
  ./deploy.sh --environment ${{ env }}`; the `each` keyword "loops
  through parameters with the object type".
- The command's escape table (Logging commands, "Special characters in
  values", VERIFIED) escapes `;`→`%3B`, `\n`→`%0A`, `\r`→`%0D`,
  `]`→`%5D` and `%`→`%AZP25` — none of which can appear in a cargo
  (`[A-Za-z0-9_-]`) or npm (URL-safe) package name, so the emitted
  value needs no escaping.

## TeamCity — <https://www.jetbrains.com/help/teamcity/service-messages.html> + <https://www.jetbrains.com/help/teamcity/matrix-build.html>

- The Matrix Build feature (VERIFIED): "The Matrix Build build
  feature enables you to define a collection of builds by iterating
  over specified parameter values and generating a build for every
  combination." The documented sample iteration set carries an
  `env.*` parameter (`Browser: Chrome, Safari, Firefox`,
  `env.ShouldFail: true, false`).
- The documented Kotlin DSL entry (VERIFIED): "add a matrix block to
  the features block in the build type configuration", containing
  `param(...)` entries — `param("Browser", listOf(value("Chrome"),
  value("Firefox")))`; a value "consists of a single line of text and
  (optionally) some parameter references".
- The `setParameter` service message (VERIFIED, the alternative
  considered): `##teamcity[setParameter name='ddd' value='fff']`
  "lets a build script dynamically update build parameters of the
  build right from a build step" — the changed parameters are
  "available in the build steps following the modifying one" and "can
  be used in the dependent builds via `%dep.*%` parameter
  references". Rejected as the adapter's grammar: a per-package
  fan-out needs build-configuration-level iteration, which the Matrix
  Build feature owns (and the same page documents the limitation
  "Reverse dependency parameters, reverse.dep.*, do not work
  correctly in matrix builds" — so the fan-out is the feature's, not
  a script's, to shape).

## Design consequences (feeds ADR-0034 + ADR-0035)

- Turbo consumes **change roots** (its own dependent closure + union
  replace the re-encoding); Nx consumes the **affected closure** list;
  Bazel consumes **roots as recursive patterns** inside one `rdeps`
  query; GitHub Actions consumes the **affected closure as the bare
  JSON array** (the matrix-output value; Actions computes nothing);
  GitLab consumes the **affected closure as one variable hash per
  package** under `parallel: matrix:`. Azure Pipelines consumes the
  **affected closure as one comma-joined output variable** (the
  `split` + `each` consumption; Pipelines computes nothing). TeamCity
  consumes the **affected closure as one `env.HALYARD_PKG` matrix
  parameter** (the Matrix Build feature fans the values out; TeamCity
  computes nothing). One seam
  carries name + root; empty selection ⇒ empty output. The
  closure-vs-roots split is a property of the consuming system: tools
  that compute dependencies themselves (turbo, bazel) get the roots;
  tools that compute nothing (nx, gha, gitlab, azure, teamcity) get
  the closure.

## CircleCI — <https://circleci.com/docs/guides/orchestrate/using-dynamic-configuration>

- "If you need to pass a parameter (and its associated value) from the
  setup workflow to the continuation workflow, you can leverage the
  `parameters` parameter of the continuation orb's `continue` command."
- The documented setup shape carries `setup: true` and invokes the
  `circleci/continuation@1.1.0` orb.
- "The `parameters` parameter accepts a JSON object as a string. Note
  the single quote encapsulating the full JSON object" — the documented
  shape is `parameters: '{ "further": true }'` (the boolean example:
  "Notice the absence of double-quotes around the boolean's value").
- "You can pass several parameters when invoking the
  `continuation/continue` command. However, they need to be part of
  the same JSON object." (`parameters: '{ "further": true,
  "other_param": "a_string" }'`)
- "The `parameters` parameter accepts either a JSON object or a path
  to a file containing a JSON object."
- The continuation config declares pipeline parameters (`parameters:`
  → `further: {type: boolean, default: false}`) consumed through the
  documented `<< pipeline.parameters.further >>` / `when:` keys.
