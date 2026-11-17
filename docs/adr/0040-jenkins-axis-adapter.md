# ADR-0040: the Jenkins declarative axis adapter

- **Status:** accepted
- **Date:** 2026-11-17
- **Milestone:** M10 step 2 continued — the ADR-0038 "Jenkins" row of
  the recorded next systems, landed under the same closed seam
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

ADR-0039 closed with `jenkins` as the refused-unknown pin in the
contract tests and named Jenkins as the next recorded system (the last
row of ADR-0038's list). Jenkins consumes **machine-readable
selections** through the declarative pipeline's documented `matrix`
directive, VERIFIED per §0.4 (fetched quotes in
`docs/research/ci-selector-grammars.md`, checked 2026-11-17): the
section defines "a multi-dimensional matrix of name-value combinations
to be run in parallel" ("cells"); the required `axes` section
"specifies one or more `axis` directives" and "Each `axis` consists of
a `name` and a list of `values`" — the documented one-axis example is
`values 'linux', 'mac', 'windows'`. The grammar's own statement rules
pin the render shape: "No semicolons as statement separators. Each
statement has to be on its own line."

## Decision

- **One impl under the unchanged ADR-0034 seam** — the
  `CiSelectorAdapter` trait, the `SelectedPackage` inputs and the
  empty-renders-nothing contract carry over untouched; the new system
  is one impl + one lookup entry + one CLI value-enum variant, exactly
  the extension path the closed surface anticipated.
- **Jenkins (`jenkins`) emits one paste-ready `axis { }` block over
  the affected closure, name-sorted**:

  ```
  axis {
      name 'HALYARD_PKG'
      values 'app', 'lib'
  }
  ```

  The consumer pastes the block inside its `matrix { axes { … } }`
  (the `axes` indentation applies) and each cell reads `$HALYARD_PKG`
  in its stages. Jenkins computes no dependency closure of its own, so
  the axis encodes the **full affected closure** (the ADR-0035 rule:
  closure-vs-roots is a property of the consuming system).
- **`HALYARD_PKG` is the halyard-owned identifier spelled upper-snake
  like the docs' own axis names** (`PLATFORM`, `BROWSER`) — the
  ADR-0035/0036/0038/0039 namespaced-identifier policy carried over;
  it cannot collide with the pipeline's own axes.
- **One statement per line, per the grammar's own rules** — the
  declarative syntax's "No semicolons as statement separators" rule
  forbids folding `name`/`values` onto one line, so the render is the
  block's four lines in order (the multi-line posture ADR-0039
  recorded; a block-shaped grammar renders as a block).
- **The render stops at the axis**: the `matrix`/`axes` wrappers and
  the required `stages` section are the consumer's own — halyard
  renders *one expression per step*, the pipeline is the consumer's
  own (the ADR-0034 seam renders the selection, not the pipeline).
- **No serialization dependency**: the lines are hand-rendered (the
  posture ADR-0036/0038/0039 recorded) — and the single-quoted Groovy
  strings never need escaping, because neither package-name grammar
  the selection carries (cargo's `[A-Za-z0-9_-]`, npm's URL-safe set)
  contains a quote.
- **Alternatives considered and rejected**: a one-line semicolon-joined
  spelling (`axis { name 'HALYARD_PKG'; values 'app', 'lib' }`) —
  Groovy tolerates semicolons, but the declarative syntax's own rules
  explicitly forbid them ("No semicolons as statement separators"), so
  the paste-ready shape is the documented block. The scripted
  `activeChoices`/plugin parameter grammars — not the declarative
  `matrix` directive, not part of the documented core syntax this
  seam renders. A generated whole-pipeline Groovy document — halyard
  renders one expression per step; the Jenkinsfile is the consumer's
  own.
- **The lookup stays closed**: `ci_selector_adapter` names the ten
  systems; anything else is `None`, and extending one is still an ADR
  (`bitbucket` is the refused-unknown pin the contract tests carry
  now — the next unrendered recorded system; its grammar VERIFY
  precedes any adapter).

## Consequences

- `halyard affected --ci jenkins` writes the `axis` block a
  declarative `matrix { axes { … } }` section consumes verbatim; no
  re-shaping happens consumer-side beyond pasting the block.
- The render cost stays pinned free (the `affected_ci` bench grows the
  `ci_selector_jenkins_40chain` case over the same 40-package chain —
  ≈182 ns, nanosecond-scale, like the nine before it).
- The empty-selection contract and the refused-unknown pin move one
  row over: the contract-test loop names ten systems, `bitbucket` is
  the refused unknown.
