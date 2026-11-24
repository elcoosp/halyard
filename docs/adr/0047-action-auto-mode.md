# ADR-0047: the §12 `auto` mode — the composition policy over the two executors

- **Status:** accepted
- **Date:** 2026-11-24
- **Milestone:** M10/§12 step 1 completed — the §12 inputs' `auto`
  mode (declared absent since ADR-0037, reaffirmed by ADR-0046) lands
  as the composition policy over the two shipped executors; the §12
  input list (`mode`: `auto|pr|release|plan|check`) is complete
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

The §12 input list starts with `auto`, and §11.1 names it the
push-trigger entry: "On every push to the base branch the **same
command** (`halyard release --auto`) runs" — the stateless
reconciliation whose tag phase always runs first and whose prepare
phase runs "only when nothing pending" (the PROPAGATE rule). Every
executor the composition needs has shipped: the release executor
(`run_release_pass` — the §11.1 step-2 tag phase over the `TagWriter`
seam, the §11.2 push over the `TagPusher` seam, the forge half) and the
pr executor (`run_pr_pass` — the §11.1 machine pass with both halves'
inputs). ADR-0046 kept `auto` declared-absent because its trigger
policy is a design decision of its own: which half owns a given run,
and how the two executors compose without re-planning between them.

## Decision

- **The composition policy is §11.1's own rule, not a new one** —
  `run_auto_pass` runs, in §11.1's order: one context → plan →
  manifest-vs-tags reconciliation; then the release executor's
  git-side tag effects (the tag phase over the posture-selected
  writer, the §11.2 push over the posture-selected pusher — an empty
  pending is an honest no-op: zero directives, zero writes); then the
  §11.1 machine pass with **both** halves' inputs (`pending_releases`
  and the Release-PR spec). The machine's own PROPAGATE rule selects
  the half: pending releases gate the prepare phase (§11.1 step 2),
  so the Release PR lands on the next pass; nothing pending means the
  prepare phase upserts or closes the Release PR (§11.1 step 3). The
  half selection is never a caller guess.
- **One plan, one reconciliation, one machine pass** — the ADR-0046
  lesson applied across the composition: the auto pass never calls the
  release executor's separate forge half (whose machine input carries
  no Release-PR spec) nor re-plans between the halves. Composing the
  two shipped executors verbatim would either double-plan or, on the
  nothing-pending shape, run two machine passes whose effects disagree
  (a close probe followed by an upsert); the composed pass runs the
  machine once with both inputs so the report's `propagated` is the
  whole truth.
- **The §12 view is plain data (`AutoPass`)** — the same convention as
  `ReleasePass`/`PrPass`: the git-side `created_tags` (the tag phase's
  own name-sorted row order, I3), the machine's `propagated` half
  selection, the `pr_effect` when the prepare phase ran (`None` on the
  PROPAGATE and one-probe no-op passes), the machine's minted URLs
  keyed by tag, the forge posture (`ForgePosture::Ran`/`Skipped` — the
  skip's reason rides verbatim) and the honest `released` flag (`true`
  only when the live tag phase created a tag; a dry forecast stays
  `false`, I5). The crate stays git-free and forge-free (the layer
  note).
- **The outputs are the pr/release superset** — `released`,
  `published` (always `false`), the `pr_number`/`pr_url` pair only
  when the pass has a Release-PR effect (absent on the PROPAGATE and
  no-op passes — the machine's own half selection), `plan_json`,
  `releases_json` (the minted URLs filled by tag), the per-package
  keys and `paths_released`. A consumer gates on the same keys
  whichever mode ran; the summary's heading is `## halyard auto` and
  its pass line renders the created tags, the half selection, the
  mints, the forge posture and the forecast marker.
- **The posture rides `--dry`** — the dry forecast runs the tag phase
  over `PreviewTagWriter`, the push over `PreviewTagPusher` and the
  machine over `PreviewForge` (nothing written, no network, I5); the
  forge posture is the honest dry skip and the mints render the
  documented `preview://` shapes. Live runs fail-closed on missing
  credentials exactly like the `pr` executor (never a pretended pass),
  and the §11.5 call governor governs the machine's calls with the
  same `HALYARD_FORGE_*` policy.
- **§11.1's `halyard release --auto` phrasing resolves to this mode** —
  the §12 action surface is what a workflow step execs (the composite
  action runs `halyard action`), so the same command the push
  workflow runs is `halyard action --mode auto`; a second `--auto`
  flag on `halyard release` would fork the surface for no capability.
- **Alternatives considered and rejected**: composing
  `run_release_pass` + `run_pr_pass` verbatim — double
  plan/reconciliation and the two-machine-pass disagreement above;
  a caller-side half selection (`if pending { release } else { pr }`)
  — re-derives the PROPAGATE rule the machine already owns and can
  drift from it; a new bespoke outputs grammar — forks the §12
  contract.

## Consequences

- `halyard action --mode auto [--dry]` is the §12's complete entry
  surface: a push-triggered workflow needs no mode plumbing — the
  mode reconciles, tags when owed (and creates the forge releases),
  and manages the Release PR when nothing is pending.
- The `render` bench grows the `auto_outputs_40_releases` /
  `auto_summary_40_releases` cases — microsecond-scale over the
  40-release plan, like the modes before it.
- The `auto_forecast` example demonstrates the PROPAGATE vocabulary
  over the preview writer (the observability convention).
- The §11.1 step-3 content-commit remainder (apply the plan's EditSet
  in memory → tree-OID compare against the release-branch tip → the
  CAS branch push) stays the M4.2 remainder it already is: the machine
  composes the forge lifecycle the `Forge` seam carries today
  (`commit_via_api` has no caller), and that gap is a separate
  milestone's work, unchanged by this mode.
