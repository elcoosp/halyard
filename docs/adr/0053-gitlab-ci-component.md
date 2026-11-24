# ADR-0053: the GitLab CI component — `templates/halyard.yml`

- **Status:** accepted
- **Date:** 2026-11-24
- **Milestone:** M7 (§17 Year-2 "More forges": "GitLab (MR flow, CI
  component), Gitea/Forgejo, Bitbucket; forge conformance test-suite
  shared across adapters") — the CI component is the row's last
  in-repo deliverable after the conformance suite (ADR-0049), the
  GitLab MR adapter (ADR-0049), Forgejo (ADR-0050) and Bitbucket
  (ADR-0051); the recorded-open corpus-lane e2e is the remaining item.
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

The M7 forge work closed with every adapter lane green; the row's
scope cell still names "GitLab (MR flow, **CI component**)" and the
ADR-0051 as-built note records the CI component as one of the two
remaining open items (with the corpus-lane e2e — the latter is
forge-token-bound, recorded twice as not shippable in this sandbox).
What the plan asks for that does not exist is the component template:
no `templates/` directory, no `.gitlab-ci.yml`, zero component
references (`grep -rn "component" docs/` hits only research prose).

The component grammar is VERIFIED first (docs.gitlab.com/ci/components/,
fetched this session — quoted in `docs/research/own-artifact-attestation.md`'s
GitLab section): a repo
exposes components as `templates/<name>.yml` with a `spec:` header
(`inputs` with `default`/`description`) above a `---` separator, and
`$[[ inputs.NAME ]]` interpolation below it.

## Decision

- **The component is `templates/halyard.yml`** (the documented layout;
  `halyard` is the component name, include-able as
  `include: component: gitlab.com/elcoosp/halyard/halyard@<ref>`).
- **It consumes the same distribution the composite action does** —
  the pinned static binary from `elcoosp/halyard`'s GitHub Releases,
  verified against the same `SHA256SUMS` manifest (`sha256sum -c` over
  the pinned asset line), on an `alpine:3.20` image (the musl static
  binary runs there; GitLab.com's shared runners are amd64). No new
  distribution path, no second artifact set — one grammar
  (`halyard_action::distribution`), now three template consumers.
- **The §12 mode surface is the same five-mode vocabulary** as the
  composite action (`auto|pr|release|plan|check`, default `plan`); the
  inputs are `stage`, `mode`, `dry-run`; the token rides the
  pipeline's own CI variables under the shell's existing env contract
  (`HALYARD_FORGE_TOKEN` exported as `GITHUB_TOKEN` when present —
  the cross-lane spelling rule, never a new variable name).
- **The attestation verify is an honest decline on this lane** (§7.3):
  `gh attestation verify` is the GitHub CLI over GitHub-hosted
  attestations; the component verifies the checksum manifest and
  documents the absence in its header — it never pretends to run it.
  The GitHub composite action is the lane that carries the M5.4
  attestation acceptance.
- **The drift gate extends**: `cargo run -p xtask -- check-release`
  now also pins the component (the spec block's inputs, the mode
  vocabulary, the `$[[ inputs.* ]]` mappings, the checksum verify, the
  release origin) — the same single-source rule the action template
  and the release pipeline ride.

## Consequences

- The M7 row's in-repo scope is complete; the corpus-lane e2e stays
  the row's recorded open item (forge-token-bound, off-sandbox).
- A GitLab repo including the component gets the §12 executor with
  checksum-verified, attestation-honest distribution; the component's
  stage joins the consumer's pipeline (`stage` input).
- The component template's YAML is validated structurally by the gate;
  the live GitLab include is the recorded off-sandbox half.
