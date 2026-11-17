# ADR-0044: the §12 `check` mode — the title verdict through the action-output seam

- **Status:** accepted
- **Date:** 2026-11-17
- **Milestone:** M10/§12 step 3 — the `check` mode of the §12 command
  surface (ADR-0037 the seam + `plan`, ADR-0043 `pr`; the remaining
  `auto|release` modes stay with their milestones)
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

The §12 inputs list `check` among the modes, and the executor already
existed: the §11.6 `check` surface classifies a squash-merge title
(`check::classify_title` — the same mapping the planner applies) and
predicts, per pending package, the bump the merge would convey
(`check::predict`, the §10.2 pre-major remap included). The action
mode is that verdict rendered through the seam: a pull_request-event
workflow step calls `halyard action --mode check --pr-title "<title>"`
and gates on the outputs/summary, exactly the CI half of §11.6.

## Decision

- **The outputs ride the plan vocabulary unchanged** — `plan_outputs`
  is reused verbatim (the merge forecast's rows, the per-package keys,
  `paths_released`, the canonical plan file). The verdict is the
  summary's own surface, so the §12 shape stays mode-independent: a
  downstream consumer parses the same keys regardless of mode.
- **The verdict is plain data in `halyard-action`** — `CheckVerdict` /
  `TitleOutcome` (accepted: kind → level + one `CheckRow` per pending
  package with the conveyed bump; rejected: the diagnosis) +
  `check_summary`. The crate stays check-logic-free (the layer note):
  the classification and the prediction stay in the shell's `check`
  module, the mapping is the shell's, the rendering is the crate's.
- **Fail-closed ordering is the mode's contract** — the classification
  precedes every effect: a rejected title refuses `E-CLI-TITLE` (the
  §11.6 code, JSON error channel included) before the plan file is
  written or any sink call exists. A workflow step must not gate on
  outputs the mode refused to stand behind.
- **Without a title the pending forecast is the verdict** — the no-title
  posture mirrors `check`'s own no-title render; the summary states it
  (`no title to lint — the pending forecast is the verdict`).
- **No `--exit-code` on the §12 surface** — the `check` command's
  pending gate (§15.4 exit 3) stays CLI-side; in a workflow step the
  outputs and summary are the signal, and a rejected title already
  fails the step. No new flag, no new posture.
- **Alternatives considered and rejected**: inventing per-package
  `--bump` output keys — the §12 vocabulary is pinned (ADR-0037) and
  the conveyed bumps are the summary's and the JSON document's
  (`verdict`/`pr_title` extras) to teach, not machine keys; posting
  the §11.6 comment from the mode — the comment is the `check`
  command's live-only surface (`E-CLI-COMMENT`), and the §12 mode
  stays offline (I5); a separate `halyard-check` binary — no.

## Consequences

- `halyard action --mode check [--pr-title TITLE] [--plan-out PATH]`
  is the §12 row's as-built form: the verdict line in the job summary
  (`title accepted: <kind> → <level> · conveyed <pkg> <bump>, …` /
  `title rejected: …` / `no title to lint …`), the plan vocabulary in
  the outputs, the `--format json` document carrying `pr_title` and
  `verdict` beside the fixed shape.
- No new error code: the mode's only failure is the §11.6
  `E-CLI-TITLE` passthrough (the bidirectional-exactness invariant is
  untouched).
- The man pages and completions gain the flag automatically (the
  clap-derivation rule).
- `halyard-action`'s bench gains `check_summary_40_releases`; an
  example (`check_forecast`) renders the verdict offline.
- The remaining §12 modes (`auto|release`) stay absent rather than
  stubbed; `release` is the natural next slice.
