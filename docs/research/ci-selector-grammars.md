# Research: CI selector grammars (VERIFY, M10 step 2)

**Checked:** 2026-11-16 · Sources fetched live per §0.4.

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

## Design consequences (feeds ADR-0034 + ADR-0035)

- Turbo consumes **change roots** (its own dependent closure + union
  replace the re-encoding); Nx consumes the **affected closure** list;
  Bazel consumes **roots as recursive patterns** inside one `rdeps`
  query; GitHub Actions consumes the **affected closure as the bare
  JSON array** (the matrix-output value; Actions computes nothing);
  GitLab consumes the **affected closure as one variable hash per
  package** under `parallel: matrix:`. One seam carries name + root;
  empty selection ⇒ empty output. The closure-vs-roots split is a
  property of the consuming system: tools that compute dependencies
  themselves (turbo, bazel) get the roots; tools that compute nothing
  (nx, gha, gitlab) get the closure.
