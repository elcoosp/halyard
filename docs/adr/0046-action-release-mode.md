# ADR-0046: the §12 `release` mode over the release executor

- **Status:** accepted
- **Date:** 2026-11-17
- **Milestone:** M10/§12 step 1 continued — the §12 inputs' `release`
  mode (declared absent since ADR-0037) rendered through the same
  action-output seam; the `auto` mode stays declared-absent
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

ADR-0037 shipped the §12 outputs contract and the `plan` mode; ADR-0043
shipped the `pr` mode (extracting `run_pr_pass` — one §11.1 machine
pass, two renderers); ADR-0044 shipped the `check` mode. The §12
inputs' documented surface (`mode`: `auto|pr|release|plan|check`) still
carried `release` and `auto` as declared-absent: absent rather than
stubbed, landing with their executors. The release executor exists —
the live `halyard release` command (the §11.1 step-2 tag phase over the
`TagWriter` seam, the §11.2 push over the `TagPusher` seam, the forge
half over the M4.1b adapter) — so the mode is a renderer over an
existing executor, exactly the shape ADR-0043 established.

## Decision

- **One executor, two renderers (the ADR-0043 pattern carried over)** —
  `cmd_release` extracts into the shell-internal `run_release_pass`:
  context → plan → reconcile → the §11.1 step-2 tag phase → the §11.2
  push → the forge half. Everything the report renderers observe rides
  the `ReleasePassOutcome`; the `release` command keeps its three
  formats byte-identical (its suite pins that), and `action --mode
  release` renders the same pass as the §12 vocabulary.
- **One plan, one reconciliation** — the extracted pass computes the
  plan and the manifest-vs-tags reconciliation once and feeds every
  half (the tag phase, the forge specs, the §12 outputs). The forge
  half's internal recomputation (it used to re-plan and re-reconcile)
  goes away — the halves cannot disagree and a live run does the work
  once.
- **The §12 view is plain data (`ReleasePass`)** — the created tags
  (the tag phase's own name-sorted row order, I3), the forge half's
  minted URLs keyed by tag, the forge posture (`ForgePosture::Ran {
  repo, releases }` / `Skipped { reason }` — the skip's reason rides
  verbatim so a forecast or a credential gap is observable), the
  honest `released` flag (`true` only when the live tag phase created
  a tag; a dry forecast stays `false`, I5) and the forecast marker.
  The crate stays git-free and forge-free (the layer note).
- **The outputs are the plan vocabulary minus the PR keys** —
  `released`, `published` (always `false` — the release mode never
  publishes), `plan_json`, `releases_json` (the minted URLs filled by
  tag), the per-package `{package}--version`/`--tag` keys and
  `paths_released`. No `pr_number`/`pr_url` keys exist: the release
  mode opens no Release PR (the `pr` mode's surface). ADR-0037's
  "a forge mode fills it" contract is satisfied by the pass's minted
  URLs.
- **The posture rides `--dry`** — the dry forecast runs the tag phase
  over `PreviewTagWriter` and the push over `PreviewTagPusher`
  (nothing written, no network, I5), names the would-create tags in
  the created-tags key, and the forge half is the honest
  `Skipped { reason }` with the dry posture's own wording. Live runs
  write and push for real with the same fail-closed ordering the
  `release` command applies; `--no-push` stays the `release`
  command's flag (the §12 mode runs the full pass).
- **The job summary is the release table plus the pass line** — the
  created tags, the forge posture (the skip's reason verbatim) and the
  forecast marker; the same release table layout the `plan`/`pr`/
  `check` summaries render (I3).
- **The `auto` mode stays declared-absent** — it composes the
  `pr` and `release` modes under a trigger policy (which mode owns a
  given event), a design decision of its own; absent rather than
  stubbed until that ADR lands.
- **Alternatives considered and rejected**: reusing `plan_outputs`
  verbatim (the `check` mode's choice) — the release pass has
  execution semantics the plan mode cannot express (`released` is
  honest, the minted URLs exist); a bespoke outputs grammar — the §12
  contract is the superset vocabulary, and a second grammar would
  fork the consumers' expectations for no gain.

## Consequences

- `halyard action --mode release [--dry]` renders the release pass as
  the §12 vocabulary; a workflow can gate on `released == 'true'` and
  read the per-package tags the release minted.
- `halyard release` loses its duplicate plan/reconciliation work in
  live mode (the forge half rides the shared pass); its reports stay
  byte-identical (the `cli_release` suite pins the shapes).
- The `render` bench grows the `release_outputs_40_releases` /
  `release_summary_40_releases` cases — microsecond-scale over the
  40-release plan, like the modes before it.
- The `release_forecast` example demonstrates the full vocabulary over
  the preview writer (the observability convention).
- The §12 `auto` mode is the only declared-absent surface left in the
  §12 inputs; the modes shipped so far cover every executor that
  exists.
