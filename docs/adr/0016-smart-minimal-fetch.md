# ADR-0016: the smart minimal fetch (§8.8) — planner, seam and shallow policy

- **Status:** accepted
- **Date:** 2026-11-14
- **Milestone:** M4.6 step 1 — the §8.8 fetch subsystem (planner +
  effect + CLI `fetch`; the §17 M4.6 acceptance — corpus checkout-time
  reduction — stays deferred to the corpus lane)
- **Deciders:** autonomous implementation agent (the §8.8 recipe needed
  three empirically-pinned decisions to become executable; recorded
  here per §0 rule 3)

## Context

§8.8 ("smart minimal fetch") promises a CI checkout proportional to the
unreleased history: probe the remote's tags, select the per-package
release tags, fetch those plus the base branch in one minimal `git
fetch --filter=blob:none --no-tags …` with `--shallow-exclude` per
selected base tag. It is the last §16.2 command that was absent rather
than wired. Three parts of the recipe are underdetermined and had to be
settled against real `git` behavior before the seam could be honest:

1. **Where the per-package templates come from.** The discovery-resolved
   `pkg.tag_template` is *not* the name the plan and the tag phase use:
   the planner's `effective_template` rule (config override →
   single-package `v{version}` → discovery template) owns tag naming
   (§11.1's "the tag phase and the plan can never disagree").
2. **When `--shallow-exclude` may be passed at all.** Probed empirically
   (git 2.x, `file://` v2 transport, the probe shapes pinned in
   `tests/fetch.rs`): on a **complete** clone, `--shallow-exclude` fails
   the whole fetch with `fatal: no commits selected for shallow requests`
   whenever the excluded history swallows the request — it is a
   *deepening* option, meaningless (and fatal) when there is nothing to
   deepen.
3. **What the exclude boundary is.** Excluding the *selected* (latest)
   tag while fetching it is self-contradictory (the boundary swallows
   the request — probed, exit 128). §8.8's "for each selected *base*
   tag" reads correctly as the **previous** release tag per package: the
   fetched tags are strictly newer than their boundary, so the boundary
   bounds the deepening without swallowing the request (probed: shallow
   clone + `--shallow-exclude=v0.1.0` + fetch `main` + `v0.2.0` → exit 0,
   tag landed).

## Decision

1. **The planner is pure and public** (`halyard_git::fetch`):
   `latest_tags_for` (the §8.2 step-2 matcher over a tag→target map,
   resolving through the now-public `halyard_core::planner::
   effective_template` so the fetch phase, the plan and the tag phase
   cannot disagree — I3), `deepen_boundaries_for` (per package the
   previous release tag; single-tag packages contribute nothing),
   `plan_fetch` / `fetch_attempts` (the argv composition, byte-pinned).
2. **One seam, one live adapter.** `TagFetcher` (object-safe) with
   `GixTagFetcher`: probe (`git ls-remote --tags`, parsed by the push
   suite's pinned `parse_remote_tag_targets`) → select → classify
   (absent → request; same target → skip, I1; divergent →
   `E-GIT-TAG-CONFLICT`, release tags are never moved) → one minimal
   fetch → report. **No preview adapter, by design**: every other effect
   seam forecasts because its mutations are known offline; the fetch
   effect's whole input is the remote listing itself, which an offline
   forecast cannot have (I5) — the honest offline surface is the pure
   planner (docs, example, bench). `fetch` therefore has no `--dry`.
3. **Shallow policy:** the shallow-exclude machinery rides **only when
   the repository is shallow** (`$GIT_DIR/shallow` present, the same
   detector the walk's `E-GIT-SHALLOW` gate uses), and only when a
   package has a previous release tag to bound with. Complete clones
   compose the plain minimal argv.
4. **Fallback (§8.8 step 3):** a failing minimal fetch is retried once
   as the full fetch of the **same refspecs** (no filter, no excludes);
   the report carries `fallback_used`. `--deepen` loops stay deferred —
   they are an interactive server negotiation the full fetch supersedes
   as the terminal fallback.
5. **CLI `fetch`** runs discovery + config only — deliberately not the
   history walk (a shallow clone, the very state fetch repairs, fails
   the walk with `E-GIT-SHALLOW`). Remote is `origin` (the §11.2 push
   convention); the base branch is the config override, defaulting to
   `HEAD` like the planner's own resolution. A failed row exits 1 (I2:
   the re-run completes).
6. **New error code `E-GIT-FETCH`** (probe or both attempts failed —
   never a prompt, `GIT_TERMINAL_PROMPT=0`); per-tag divergences stay
   `E-GIT-TAG-CONFLICT` rows.

## Consequences

- `halyard_core::planner::effective_template` became `pub` (the same
  pure rule the planner and the reconciliation already use; no new
  logic).
- The §17 M4.6 acceptance ("CI checkout time reduced ≥ 50 % on a large
  corpus repo") stays deferred to the corpus lane — the mechanism is
  shipped, the corpus measurement is not claimed.
- The fetch effect is the third sanctioned subprocess effect in
  `halyard-git` (tag write, tag push, fetch) and the one §I5 names
  explicitly; the crate doc's exception list names all three.
- Re-runs are I1 no-ops by construction: every selected tag already
  local at the probed target is not requested, so the phase reaches a
  fixed point with zero duplicate fetches.
