# ADR-0041: the Bitbucket Pipelines parallel-steps adapter

- **Status:** accepted
- **Date:** 2026-11-17
- **Milestone:** M10 step 2 continued — the ADR-0040 "bitbucket" row,
  the last recorded system, landed under the same closed seam
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

ADR-0040 closed with `bitbucket` as the refused-unknown pin in the
contract tests and named Bitbucket Pipelines as the next recorded
system (its grammar VERIFY precedes any adapter). Bitbucket consumes
**machine-readable selections** through the documented parallel-steps
shape, VERIFIED per §0.4 (fetched quotes in
`docs/research/ci-selector-grammars.md`, checked 2026-11-17):
"Parallel steps allow you to group pipeline steps that can run at the
same time (concurrently) to reduce build time"; the group carries the
required `steps:` property — "The `steps` property contains the lists
of `step`s in a `stage` or a `parallel`. At least 1 `step` is required
within the steps list" — and the docs' own example differentiates the
group's steps **in the script itself** (`--batch 1` / `--batch 2` per
step). There is no per-step `env` property (the `variables` option's
"Allowed parent properties" is `[pipe]`), and Bitbucket computes no
dependency closure and has no matrix grammar.

## Decision

- **One impl under the unchanged ADR-0034 seam** — the
  `CiSelectorAdapter` trait, the `SelectedPackage` inputs and the
  empty-renders-nothing contract carry over untouched; the new system
  is one impl + one lookup entry + one CLI value-enum variant, exactly
  the extension path the closed surface anticipated.
- **Bitbucket (`bitbucket`) emits one paste-ready `parallel:` block
  over the affected closure, name-sorted** — one step per affected
  package, whose script carries one literal assignment:

  ```yaml
  parallel:
    steps:
      - step:
          script:
            - export HALYARD_PKG=app
      - step:
          script:
            - export HALYARD_PKG=lib
  ```

  The consumer pastes the block at pipeline level (`- parallel:`, its
  own leading indent applies) and appends its own tool invocation
  after the assignment, reading `$HALYARD_PKG` in the same script (the
  docs' own example consumes default variables —
  `$BITBUCKET_COMMIT` — inside script lines, so a script-line
  assignment is the grammar's native surface). Bitbucket computes no
  dependency closure of its own, so the steps encode the **full
  affected closure** (the ADR-0035 rule: closure-vs-roots is a
  property of the consuming system).
- **`HALYARD_PKG` is the halyard-owned identifier spelled upper-snake
  like the docs' own `BITBUCKET_*` variable style** — the
  ADR-0035/0036/0038/0039/0040 namespaced-identifier policy carried
  over; it cannot collide with the pipeline's own variables, and it
  satisfies the documented variable-name rule ("Names can only contain
  ASCII letters, digits and underscores").
- **The relative indentation follows the documented example exactly**
  — `steps:` +2 under `parallel:`, the step list +2 under `steps:`,
  `script:` +4 under the step, the script items +2 under `script:` —
  so the pasted block keeps the docs' shape at any leading indent.
- **The assignment is verbatim-safe and unquoted**: neither
  package-name grammar the selection carries (cargo's
  `[A-Za-z0-9_-]`, npm's URL-safe set) contains a shell metacharacter,
  so the bare-word `export` value is emitted verbatim (the docs' own
  script arguments are bare words too).
- **The render stops at the parallel group**: the surrounding
  `pipelines:`/`default:` envelope and the tool invocation are the
  consumer's own — halyard renders *one expression per step*, the
  pipeline is the consumer's own (the ADR-0034 seam renders the
  selection, not the pipeline).
- **The step cap is the consumer's policy**: "The 100-step limit on
  pipelines includes each step in a parallel group" — the adapter
  renders the grammar, never the guard (the ADR-0039 posture).
- **Alternatives considered and rejected**: an indexed-array trick
  (`PACKAGES=(…)`, `${PACKAGES[$BITBUCKET_PARALLEL_STEP]}`) — compact,
  but it makes the consumer write shell indirection halyard invented,
  and the default variables ("zero-based index of the current step in
  the group") are documented for addressing steps, not for building
  selection arrays; the paste-ready shape is the literal per-step
  assignment. A `definitions:`-anchored step template — YAML anchors
  are outside the documented core shape this seam renders.
- **The lookup stays closed**: `ci_selector_adapter` names the eleven
  systems; anything else is `None`, and extending one is still an ADR
  (`drone` is the refused-unknown pin the contract tests carry now —
  an unrecorded system, so no adapter follows from it; the recorded
  CI surface is complete with this ADR).

## Consequences

- `halyard affected --ci bitbucket` writes the `parallel: steps:` block
  a Bitbucket pipeline consumes verbatim; no re-shaping happens
  consumer-side beyond pasting the block and appending the tool
  invocation.
- The render cost stays pinned free (the `affected_ci` bench grows the
  `ci_selector_bitbucket_40chain` case over the same 40-package chain —
  nanosecond-scale, like the ten before it).
- The empty-selection contract and the refused-unknown pin move one
  row over: the contract-test loop names eleven systems, `drone` is
  the refused unknown.
- The recorded CI-adapter surface (ADR-0034 → ADR-0041) is complete:
  every recorded system renders. A new system means a new grammar
  VERIFY + ADR, as always.
