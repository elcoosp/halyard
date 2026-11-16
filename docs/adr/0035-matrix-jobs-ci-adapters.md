# ADR-0035: Matrix-jobs CI adapters (GitHub Actions / GitLab output grammars)

- **Status:** accepted
- **Date:** 2026-11-16
- **Milestone:** M10 step 2 continued — the ADR-0034 "More CI systems
  (GitLab matrix, GitHub jobs JSON) join by adding an impl + an enum
  variant" consequence, landed under the same closed seam
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

ADR-0034 closed the `CiSelectorAdapter` seam over three adapters whose
grammars are command lines (turbo filters, nx project lists, bazel
queries). The two remaining target systems consume **matrix job
definitions**, not argv: GitHub Actions builds its `strategy.matrix`
from a job's output value through `fromJSON`, and GitLab expands
`parallel:matrix` from an array of variable hashes. Both grammars are
VERIFIED per §0.4 (fetched quotes in
`docs/research/ci-selector-grammars.md`).

## Decision

- **Two impls under the unchanged ADR-0034 seam** — the
  `CiSelectorAdapter` trait, the `SelectedPackage` inputs and the
  empty-renders-nothing contract carry over untouched; each new system
  is one impl + one lookup entry + one CLI value-enum variant, exactly
  the extension path the closed surface anticipated.
- **GitHub Actions (`gha`)** emits **one compact JSON array over the
  affected closure** (name-sorted): the documented output value
  (`colors=["red", "green", "blue"]` written to `$GITHUB_OUTPUT`,
  fed to `fromJSON(needs.…outputs.…)` inside the downstream
  `strategy.matrix`). Actions computes no dependency closure of its
  own, so unlike turbo/bazel the array encodes the **full affected
  closure**; the consumer names the matrix dimension on its side, so
  no key policy is encoded in the adapter.
- **GitLab (`gitlab`)** emits **one `- HALYARD_PKG: <name>` hash per
  affected package** (name-sorted): `parallel:matrix` is "an array of
  hashes of variables" — "each instance of the job" runs "with
  different variable values" — so a one-dimensional matrix is one hash
  per instance. The identifier `HALYARD_PKG` is the documented policy
  choice of this ADR (identifiers "can use only numbers, letters, and
  underscores"; the namespaced spelling cannot collide with the job's
  own variables); the consumer pastes the lines under
  `parallel: matrix:` and reads `$HALYARD_PKG` in the script.
- **Closure-vs-roots is a property of the consuming system, not of the
  adapter**: tools that compute dependencies themselves (turbo, bazel)
  receive the change roots; tools that compute nothing (nx, gha,
  gitlab) receive the full affected closure. The trait's two inputs
  carry both without a flag.
- **The lookup stays closed**: `ci_selector_adapter` names the five
  systems; anything else is `None`, and extending one is still an ADR.

## Consequences

- `halyard affected --ci gha` writes the output value a GitHub Actions
  job forwards to `strategy.matrix` verbatim; `halyard affected --ci
  gitlab` writes the `parallel:matrix` body verbatim. No JSON/YAML
  re-shaping happens consumer-side.
- The render cost stays pinned free (the `affected_ci` bench grows the
  `ci_selector_gha_40chain` / `ci_selector_gitlab_40chain` cases over
  the same 40-package chain).
- The gha adapter depends on `serde_json` for the array emission
  (ADR-0005 already places serde/JSON in core); the escaping is the
  library's, never hand-rolled.

## Alternatives considered

- **Emit a `{"package": [...]}` object for gha.** Rejected: the
  documented output value is the bare array; naming the dimension in
  the adapter would duplicate the consumer's own matrix definition
  (and freeze halyard's choice into every workflow).
- **Emit YAML strings for GitHub or JSON for GitLab.** Rejected: each
  system consumes exactly one shape — the matrix output value (JSON
  array) on the Actions side, the array of hashes (YAML) on the GitLab
  side. Translating one into the other invents a grammar neither tool
  documents.
- **A generated `.gitlab-ci.yml` / workflow file.** Rejected: the
  adapters render the selection, not the consumer's pipeline — the CI
  step owns its job definition (the ADR-0034 "no JSON re-parsing"
  boundary, kept).
