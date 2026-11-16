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

## Design consequences (feeds ADR-0034)

- Turbo consumes **change roots** (its own dependent closure + union
  replace the re-encoding); Nx consumes the **affected closure** list;
  Bazel consumes **roots as recursive patterns** inside one `rdeps`
  query. One seam carries name + root; empty selection ⇒ empty output.
