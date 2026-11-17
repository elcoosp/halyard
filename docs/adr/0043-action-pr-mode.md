# ADR-0043: the §12 `pr` mode — the release machine's pass view through the action-output seam

- **Status:** accepted
- **Date:** 2026-11-17
- **Milestone:** M10/§12 step 2 — the `pr` mode of the §12 command surface
  (ADR-0037 recorded the seam and the `plan` mode; the remaining modes
  land with their executors — this is the first of them)
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

ADR-0037 landed the action-output seam: the §12 outputs vocabulary, the
`GithubActionsWriter`/`PreviewActionWriter` pair and the `plan` mode of
`halyard action` — offline, testable end to end, honestly absent-mode
for the rest (`auto|pr|release|check` "land with their executors"). The
`pr` executor already existed: `halyard pr` drives the §11.1 release
machine over the adapter the posture selects (`PreviewForge` forecasts
offline, `GitHubRestForge` executes live, the §11.5 governor paces the
live run), and its `MachineReport` carries every effect in order.

The §12 outputs vocabulary has `pr_number` / `pr_url` pinned since
ADR-0037 ("a forge mode fills it"), but two gaps kept the mode absent:

1. **The machine report dropped the forge's URL.** `MachineEffect`
   carried only the PR *number*; the URL the adapter minted (the REST
   `html_url`, the preview's `preview://` shape) was discarded at the
   match. A composed `https://github.com/{slug}/pull/{n}` guess in the
   shell would be exactly the kind of fabrication the repo refuses —
   the report must carry what the forge actually returned.
2. **The executor logic lived inline in `cmd_pr`.** A §12 renderer
   driving "the same machine pass" by re-implementing the
   context→plan→reconcile→spec→forge sequence would fork the exact
   logic M4.1b's adapter-swap posture exists to keep single-sourced.

## Decision

- **The forge's URL rides the report** — `MachineEffect::PrOpened`,
  `PrUpdated`, `PrUnchanged` and `PrClosed` carry the adapter's own URL
  beside the number (added to the existing struct variants; the JSON
  effect rows in the `pr` report gain the same `url` key, the human/GHA
  forecast lines stay byte-identical). The report remains the single
  observation point: "deterministic data only, comparable across runs
  (I3)" — the adapter's URL is deterministic data.
- **One executor, two renderers** — the §11.1 pass assembly
  (context → plan → reconcile → §11.3 spec assembly → adapter
  selection → §11.5-governed machine pass) extracts from `cmd_pr` into
  a shell-internal `run_pr_pass` returning a `PrPassOutcome` value.
  `cmd_pr` renders the `pr` report from it; `cmd_action_pr` maps the
  report onto the §12 view and renders the seam outputs from it. No
  forked logic; the adapter swap stays the only posture difference.
- **A plain `PrPass` view in `halyard-action`** — the crate stays
  forge-free (the ADR-0037 layer note): `PrEffect` (the
  opened/updated/unchanged/closed effect with number + URL),
  `PrPass` (effect + `propagated` + `released` + minted
  `release_urls` + branch + forecast posture) and the renderers
  `pr_outputs` / `pr_summary`. The mapping from `MachineReport` is the
  shell's; the vocabulary is the crate's.
- **The §12 outputs for the `pr` mode**, in the documented order:
  `released` (the pass's honest execution value — `true` only when the
  live tag phase created a release; a dry forecast stays `false`, I5),
  `published` (always `false` — the pr mode never publishes), the
  `pr_number` / `pr_url` pair when the pass has a Release PR to report
  (absent on the §11.1 PROPAGATE and no-op passes — the keys the mode
  does not produce are not written), `plan_json` (the canonical plan
  file, written as in the plan mode), `releases_json` (the plan rows,
  the pass's minted URLs filled by tag match), the per-package keys and
  `paths_released` — the plan vocabulary unchanged.
- **The summary names the posture** — the release table plus one
  deterministic pass line: the effect (opened/updated/already
  current/closed), the §11.1 PROPAGATE posture, the no-op posture, the
  pass's minted releases (`minted <tag> (<url>)`, so a PROPAGATE
  forecast's `preview://` mints are observable beside the plan's
  next-release rows) and the `forecast (nothing executed)` marker on
  dry runs (I3: no wall-clock, effects in report order).
- **The dry posture rides `--dry`** — the flag the `pr` command already
  owns, on the `action` command: the §11.1 machine over the preview
  adapter, no network (I5), the `preview://`/`0` shapes marking the
  forecast. Live mode (`--dry` unset) runs the same pass over the REST
  adapter — fail-closed on missing credentials and foreign forges,
  exactly `halyard pr`'s live posture.
- **Alternatives considered and rejected**: composing the PR URL in the
  shell from the slug — a guess that can diverge from the forge's own
  `html_url`; re-implementing the pass in `cmd_action_pr` — a fork of
  the one code path whose whole value is being single-sourced;
  `releases_json` from the tag-phase queue instead of the plan rows —
  the vocabulary would change shape per posture, and ADR-0037's
  contract ("the url empty until a forge mode fills it") pins the plan
  rows; minted rows ride the summary's pass line and the `pr` report.

## Consequences

- `halyard action --mode pr [--dry] [--plan-out PATH]` is the §12 row's
  as-built form: the release-PR executor's pass view, rendered through
  the same sink postures as the plan mode (env files in a step, the
  preview call log outside).
- The `MachineEffect` widening is a breaking change to a `0.0.0`
  workspace's public enum — every constructor/match site updated in the
  same commit (the forge state-machine tests, the preview test, the
  `pr` report renderers); the human/gha forecast lines stay
  byte-identical, the JSON effect rows gain `url`.
- No new error code: the mode's failures ride the existing taxonomy
  (`E-FORGE-*` from the machine pass, `E-ACTION-OUTPUT`/`-SUMMARY`/
  `-PLAN` from the seam, `E-FORGE-AUTH`/`E-CLI-PR` from the live
  posture) — the bidirectional-exactness invariant is untouched.
- The man pages and completions gain the mode and flag automatically
  (the clap-derivation rule).
- `halyard-action`'s benches gain the `pr_outputs`/`pr_summary` groups
  over the 40-release plan (the §12 renderers stay microsecond-scale);
  an example (`pr_forecast`) renders a sample pass offline.
- The remaining §12 modes (`auto|release|check`) stay absent rather
  than stubbed; `release` is the natural next slice (it reuses the
  plan-row `releases_json` where the tags always match the minted
  ones).
