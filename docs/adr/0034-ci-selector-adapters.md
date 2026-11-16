# ADR-0034: CI-selector adapters (Turborepo / Nx / Bazel output grammars)

- **Status:** accepted
- **Date:** 2026-11-16
- **Milestone:** M10 step 2 — "integration helpers for
  Turborepo/Nx/Bazel graphs" beside the ADR-0032 selector (step 1)
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

The affected selector (ADR-0032) answers *which packages must rebuild
and test* as pure data; its renderings were human/json/gha only. The
declared M10 step 2 asks for the integration helpers: a CI step needs
the selection in the CI system's **own** grammar, not a JSON document
it would have to re-interpret. The grammars are VERIFIED per §0.4
(fetched quotes in `docs/research/ci-selector-grammars.md`).

## Decision

- **A closed adapter seam in core** (`halyard_core::ci_selector`): the
  `CiSelectorAdapter` trait (`system()` + `render(changed, affected)`)
  over `SelectedPackage { name, root }` inputs — one seam carries both
  the tool-facing names (turbo/nx filters) and the repo-relative roots
  (the Bazel target patterns). Inputs are plain data (name-sorted by
  the CLI), so the adapters stay pure, testable and benchable.
- **Three impls, one line per argv atom (I7)**: Turbo
  (`--filter=...pkg` per change root — the dependents close on turbo's
  side, filters union), Nx (`--projects=a,b` over the affected closure
  — dependents-first scheduling is the tool's own default), Bazel
  (`rdeps(//..., //root/... + //root/...)` over the change roots — the
  documented reverse-dependency operator).
- **Empty renders nothing** (the honest no-op: the CI step skips
  itself, exit 0 — the existing empty-selection contract).
- **The surface is closed**: `ci_selector_adapter(system)` knows the
  three; anything else is `None`, and extending one is an ADR (the
  §16.2 convention). The CLI's `--ci` value enum refuses unknown
  systems before the lookup.
- **The command's own flag** (`affected --ci SYSTEM`): the same
  vocabulary/flag split lesson as `graph --graph-format` — the global
  `--format` owns human|json|gha, `--ci` owns the CI grammars.

## Consequences

- `halyard affected --ci turbo` drops straight into a `turbo run`
  invocation; the CI integration needs no JSON re-parsing.
- The render cost is pinned free (the `affected_ci` bench over the
  40-package chain shape).
- More CI systems (GitLab matrix, GitHub jobs JSON) join by adding an
  impl + an enum variant — the seam and the refusal contract are
  already pinned.

## Alternatives considered

- **JSON only, CI-side re-interpretation.** Rejected: it pushes the
  grammar knowledge to every consumer — exactly the drift the
  single-source rule (the completions/man renderer's) exists to
  prevent.
- **A generic expression DSL.** Rejected: three verified grammars
  today; a DSL would invent a fourth and translate it. The adapters are
  the honest smallest surface.
