# ADR-0038: the CircleCI dynamic-config continuation adapter

- **Status:** accepted
- **Date:** 2026-11-17
- **Milestone:** M10 step 2 continued — the ADR-0036 "more adapters
  (CircleCI dynamic config, Buildkite, Jenkins) remain behind the
  closed lookup" consequence, landed under the same closed seam
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

ADR-0036 closed with `circleci` as the refused-unknown pin in the
contract tests and named CircleCI dynamic config as the next recorded
system. CircleCI consumes **machine-readable selections** through its
documented dynamic-configuration flow, VERIFIED per §0.4 (fetched
quotes in `docs/research/ci-selector-grammars.md`, checked
2026-11-17): the setup workflow (`setup: true`) invokes the
`circleci/continuation` orb, whose `continue` command takes
`parameters` — "The `parameters` parameter accepts a JSON object as a
string. Note the single quote encapsulating the full JSON object" —
and the continuation config declares the matching pipeline parameters
consumed through the documented `<< pipeline.parameters.… >>` /
`when:` keys.

## Decision

- **One impl under the unchanged ADR-0034 seam** — the
  `CiSelectorAdapter` trait, the `SelectedPackage` inputs and the
  empty-renders-nothing contract carry over untouched; the new system
  is one impl + one lookup entry + one CLI value-enum variant, exactly
  the extension path the closed surface anticipated.
- **CircleCI (`circleci`) emits one paste-ready
  `parameters: '{ "halyard_pkgs": [...] }'` line over the affected
  closure, name-sorted**: the value is the documented `parameters`
  shape (a JSON object as a string, single-quoted), the object's
  `halyard_pkgs` array is the affected closure, and the consumer
  pastes the line into the setup workflow's `continuation/continue`
  step, then declares the matching pipeline parameter in the
  continuation config and iterates `pipeline.parameters.halyard_pkgs`
  on its side. CircleCI computes no dependency closure of its own, so
  the array encodes the **full affected closure** (the ADR-0035 rule:
  closure-vs-roots is a property of the consuming system).
- **`halyard_pkgs` is the halyard-owned identifier in the parameter
  grammar's own casing** — the ADR-0035/0036 namespaced-identifier
  policy carried over, spelled lower-snake because the docs' own
  pipeline-parameter examples are lower-snake (`further`,
  `other_param`); it cannot collide with the pipeline's own
  parameters.
- **No serialization dependency**: the single-quoted JSON literal is
  hand-rendered (the same posture ADR-0036 recorded for the azure and
  teamcity adapters) — and it never needs escaping, because neither
  package-name grammar the selection carries (cargo's
  `[A-Za-z0-9_-]`, npm's URL-safe set) contains a quote or a
  backslash.
- **Alternatives considered and rejected**: emitting the bare JSON
  array (the gha shape) — the consumer would have to invent the
  wrapping `parameters:` entry and quoting, which the docs pin
  themselves; emitting a generated continuation-config YAML document —
  halyard renders *one expression per step*, the continuation config's
  jobs/workflows are the consumer's own (the ADR-0034 seam renders the
  selection, not the pipeline); `parameters` as a file path — the
  documented file form needs a pre-written file, while the inline
  object is the paste-ready one-liner the seam contract renders.
- **The lookup stays closed**: `ci_selector_adapter` names the eight
  systems; anything else is `None`, and extending one is still an ADR
  (`buildkite` is the refused-unknown pin the contract tests carry
  now).

## Consequences

- `halyard affected --ci circleci` writes the `parameters:` line a
  setup workflow's `continuation/continue` step consumes verbatim; no
  re-shaping happens consumer-side beyond declaring the pipeline
  parameter the continuation config reads.
- The render cost stays pinned free (the `affected_ci` bench grows the
  `ci_selector_circleci_40chain` case over the same 40-package chain —
  nanosecond-scale, like the seven before it).
- The empty-selection contract and the refused-unknown pin move one
  row over: the contract-test loop names eight systems, `buildkite` is
  the refused unknown.
