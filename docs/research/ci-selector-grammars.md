# Research: CI selector grammars (VERIFY, M10 step 2)

**Checked:** 2026-11-16 · Sources fetched live per §0.4 (the Azure
Pipelines and TeamCity sections fetched 2026-11-17 for ADR-0036; the
CircleCI section fetched 2026-11-17 for ADR-0038; the Buildkite and
Jenkins sections fetched 2026-11-17 for ADR-0039/0040).

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
  the closure. (The matrix-jobs systems that landed after ADR-0035 —
  CircleCI's continuation `parameters` object, Buildkite's
  `matrix.setup` dimension, Jenkins' declarative `axis` — follow the
  same rule: each computes nothing, so each carries the full affected
  closure.)

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

## Buildkite — <https://buildkite.com/docs/pipelines/configure/workflows/build-matrix>

(Fetched 2026-11-17 for ADR-0039.)

- "Build matrices help you simplify complex build configurations" by
  "expanding a step template and array of matrix elements into
  multiple jobs."
- The multi-dimension form carries `matrix: setup:` with dimension
  keys mapping to lists of values:
  `matrix: { setup: { arch: ["amd64", "arm64"], os: ["windows", "linux"] } }`
  in the docs' block style — `matrix:` → `setup:` → dimension key →
  quoted-string elements (`arch:` → `- "amd64"` / `- "arm64"`), each
  combination fanning the step out into one job ("Each dimension you
  add is multiplied by the other dimensions").
- Matrix values are interpolated into the step's environment variables,
  labels, commands, plugins and agents as `{{matrix.<key>}}` ("You
  can't use matrix values in other attributes, including step keys and
  concurrency groups").
- Every documented element is a quoted string; elements are size-bounded
  ("128 bytes maximum size for each individual matrix element (both
  keys and values)"), with the documented caps "6 dimensions maximum",
  "25 elements per dimension" and "50 jobs created per `matrix`
  configuration on a `command` step".

## Jenkins — <https://www.jenkins.io/doc/book/pipeline/syntax/>

(Fetched 2026-11-17 for ADR-0040.)

- The declarative `matrix` directive: "section defining a
  multi-dimensional matrix of name-value combinations to be run in
  parallel. We'll refer these combinations as "cells" in a matrix."
- "The `matrix` section must include an `axes` section and a `stages`
  section. The `axes` section defines the values for each `axis` in
  the matrix."
- "The `axes` section specifies one or more `axis` directives. Each
  `axis` consists of a `name` and a list of `values`. All the values
  from each axis are combined with the others to produce the cells."
- The documented one-axis example ("One-axis with 3 cells") is
  `axis { name 'PLATFORM' values 'linux', 'mac', 'windows' }` in block
  form — `name` and `values` as separate statements, the values a
  comma-separated list of single-quoted strings.
- The declarative grammar's own statement rules pin the render shape:
  "No semicolons as statement separators. Each statement has to be on
  its own line."

## Bitbucket Pipelines — <https://support.atlassian.com/bitbucket-cloud/docs/parallel-step-options/> + <https://support.atlassian.com/bitbucket-cloud/docs/step-options/> + <https://support.atlassian.com/bitbucket-cloud/docs/variables-and-secrets/>

(Fetched 2026-11-17 for ADR-0041.)

- "Parallel steps allow you to group pipeline steps that can run at the
  same time (concurrently) to reduce build time."
- The group's shape (the documented example, verbatim indentation):
  `- parallel:` as a pipeline list item, the required `steps:` property
  under it, the step list under `steps:`:

  ```yaml
  - parallel:      # these 2 steps will run in parallel
      steps:
        - step:
            script:
              - ./integration-tests.sh --batch 1
        - step:
            script:
              - ./integration-tests.sh --batch 2
  ```

  The docs' own example differentiates the group's steps **in the
  script itself** (`--batch 1` / `--batch 2` as literal per-step
  arguments) — per-step script differences are the grammar's native
  way to carry a varying value.
- The `steps` property is required: "The `steps` property contains the
  lists of `step`s in a `stage` or a `parallel`. At least 1 `step` is
  required within the steps list." (Required — Yes.)
- **No per-step `env` property exists**: the `step` option's child
  properties enumerate `after-script, artifacts, caches, clone,
  config, condition, deployment, fail-fast, …` (no `env`), and the
  `variables` option states "Allowed parent properties — `[pipe]`"
  ("The pipe variables options is used to configure the environmental
  variables of a pipe"). A per-step value therefore rides the script
  line itself.
- Script lines consume variables natively (the documented example
  `echo "Building commit $BITBUCKET_COMMIT"`), so a script-line
  assignment (`export HALYARD_PKG=app`) is readable in the same
  script's later lines.
- The parallel group's default variables: "In addition to the standard
  pipelines variables, parallel step groups also have the following
  default variables:" — `BITBUCKET_PARALLEL_STEP` ("zero-based index
  of the current step in the group (such as 0, 1, 2, …)") and
  `BITBUCKET_PARALLEL_STEP_COUNT` ("total number of steps in the
  group"). Availability: "Only pipelines within a parallel step";
  "Not available outside a parallel step."
- Variable-name rule (the `output-variables` section): "Names can only
  contain ASCII letters, digits and underscores" / "Names can't start
  with a digit" — `HALYARD_PKG` complies.
- The step cap: "The 100-step limit on pipelines includes each step in
  a parallel group." (The workspace concurrency numbers — "Standard
  and premium workspaces can support 600 steps running concurrently",
  "Free workspaces can support 10 steps" — are plan-dependent consumer
  policy.)
- Design consequence: no matrix grammar and no per-step `env` means
  the selection can only ride literal per-step script content — one
  step per affected package, each carrying `export HALYARD_PKG=<name>`
  (the relative indentation of the docs' example preserved), the
  consumer's tool invocation appended in the same script.

## Drone — <https://docs.drone.io/pipeline/environment/syntax/> + <https://docs.drone.io/pipeline/docker/syntax/steps/> + <https://docs.drone.io/pipeline/docker/syntax/parallelism/>

*(Fetched 2026-11-17 for ADR-0045.)*

- Per-step scoping: "Drone provides the ability to define environment
  variables scoped to individual build steps." The `environment` key
  sits under an individual step as a name→value mapping; the
  documented example maps `GOOS: linux` / `GOARCH: amd64` beside a
  step's `commands:` (two-space indent under the step's keys, entries
  two further spaces in).
- No expansion: "the environment section cannot expand environment
  variables or evaluate shell expressions" — the documented bad example
  is `GOPATH: ${HOME}/golang` ("bad"), the documented good pattern is
  an `export` inside `commands:`. A literal value (a package name)
  rides the mapping verbatim.
- Secret form: the same mapping carries `from_secret:` values
  (`PASSWORD: from_secret: password`) — irrelevant to the selection
  render (no secret rides a public selection) but pinned to show the
  mapping's two value shapes.
- Step shape: `steps:` sits at the pipeline's top level; each entry
  carries `name`, `image`, `commands` ("NB 'commands' overrides the
  docker entrypoint"). The docs never label required fields, but every
  documented example gives all three; the adapter therefore emits only
  the `environment:` fragment (a full step would guess the consumer's
  image/commands).
- Sequential-by-default: "Pipeline steps are executed sequentially by
  default." Parallel execution is the documented `depends_on` DAG —
  "You can optionally describe your build steps as a directed acyclic
  graph" (the docs' example fans out `backend`/`frontend` and fans in
  `notify`) — with the rule "when you define the dependency graph you
  must configure dependencies for all pipeline steps." Both are the
  consumer's pipeline policy; the adapter renders the grammar, never
  the guard.
- Design consequence: a documented per-step env mapping exists
  (unlike Bitbucket's `pipe`-only `variables`), so the selection rides
  step-scoped `environment:` fragments — one fragment per affected
  package (`environment:` + `  HALYARD_PKG: <name>`), paste-ready
  modulo the step's own indentation, the consumer's tool invocation
  reading `$HALYARD_PKG` in the same step's `commands`.
