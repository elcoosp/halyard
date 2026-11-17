# ADR-0039: the Buildkite matrix adapter

- **Status:** accepted
- **Date:** 2026-11-17
- **Milestone:** M10 step 2 continued — the ADR-0038 "Buildkite" row of
  the recorded next systems, landed under the same closed seam
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

ADR-0038 closed with `buildkite` as the refused-unknown pin in the
contract tests and named Buildkite as the next recorded system.
Buildkite consumes **machine-readable selections** through its
documented build-matrix flow, VERIFIED per §0.4 (fetched quotes in
`docs/research/ci-selector-grammars.md`, checked 2026-11-17): a
`matrix: setup:` block "expand[s] a step template and array of matrix
elements into multiple jobs", the dimension keys map to lists of
quoted-string values, and the step interpolates `{{matrix.<key>}}` in
its label, command, env, plugins and agents ("You can't use matrix
values in other attributes, including step keys and concurrency
groups").

## Decision

- **One impl under the unchanged ADR-0034 seam** — the
  `CiSelectorAdapter` trait, the `SelectedPackage` inputs and the
  empty-renders-nothing contract carry over untouched; the new system
  is one impl + one lookup entry + one CLI value-enum variant, exactly
  the extension path the closed surface anticipated.
- **Buildkite (`buildkite`) emits one paste-ready `matrix:` block
  declaring the `halyard_pkg` setup dimension over the affected
  closure, name-sorted**:

  ```
  matrix:
    setup:
      halyard_pkg:
        - "app"
        - "lib"
  ```

  The shape is the docs' own multi-dimension spelling (block style,
  every element a quoted string); the consumer pastes the block under
  a step — the step's own indentation applies — and interpolates
  `{{ matrix.halyard_pkg }}` per cell. Buildkite computes no
  dependency closure of its own, so the dimension encodes the **full
  affected closure** (the ADR-0035 rule: closure-vs-roots is a
  property of the consuming system).
- **`halyard_pkg` is the halyard-owned identifier in the docs' own
  dimension-key style** (lower-snake: `arch`, `os`, `test`) — the
  ADR-0035/0036/0038 namespaced-identifier policy carried over; it
  cannot collide with the pipeline's own dimensions. Singular, because
  each matrix cell consumes exactly one package (the GitLab/TeamCity
  identifier convention at Buildkite's grammar).
- **The render is one block, one line per line of the block** — the
  seam's `Vec<String>` carries the block's lines in order, the same
  multi-line posture the GitLab adapter uses for its per-package
  hashes; a block-shaped grammar renders as a block, never folded
  into a one-line approximation the docs do not spell.
- **No serialization dependency**: the lines are hand-rendered (the
  posture ADR-0036/0038 recorded) — and the scalars never need
  escaping, because neither package-name grammar the selection
  carries (cargo's `[A-Za-z0-9_-]`, npm's URL-safe set) contains a
  quote.
- **The documented caps are the consumer's policy, not the adapter's**:
  the docs cap a dimension at 25 elements and a matrix configuration
  at 50 jobs — a 25+-package workspace must chunk or pick another
  system. The adapter renders the grammar, never the guard (the seam
  has no validation surface and gains none here).
- **Alternatives considered and rejected**: the simple list form
  (`matrix: ["app", "lib"]` — one anonymous dimension interpolated as
  `{{matrix}}`) — it renders the closure but anonymizes it; the
  consumer's step cannot tell a package element from any other string
  and the identifier policy (namespaced, collision-free) is lost. A
  one-line flow-mapping spelling (`matrix: { setup: { halyard_pkg:
  [...] } }`) — valid YAML but not the docs' own shape, which is
  block style throughout. A generated whole-pipeline YAML document —
  halyard renders one expression per step, the pipeline is the
  consumer's own (the ADR-0034 seam renders the selection, not the
  pipeline).
- **The lookup stays closed**: `ci_selector_adapter` names the nine
  systems; anything else is `None`, and extending one is still an ADR
  (`jenkins` is the refused-unknown pin the contract tests carry now).

## Consequences

- `halyard affected --ci buildkite` writes the `matrix.setup`
  dimension block a Buildkite step consumes verbatim; no re-shaping
  happens consumer-side beyond pasting the block under the step.
- The render cost stays pinned free (the `affected_ci` bench grows the
  `ci_selector_buildkite_40chain` case over the same 40-package chain —
  ≈132 ns, nanosecond-scale, like the eight before it).
- The empty-selection contract and the refused-unknown pin move one
  row over: the contract-test loop names nine systems, `jenkins` is
  the refused unknown.
