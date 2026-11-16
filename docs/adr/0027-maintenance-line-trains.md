# ADR-0027: maintenance-line release trains (M5.6 completion, §10.3)

- **Status:** accepted
- **Date:** 2026-11-16
- **Milestone:** M5.6 completion — the maintenance-branch half of the
  roadmap row ("prerelease channels + maintenance-branch release
  trains; fixtures: `next`/`rc`, `1.x` backports") that ADR-0025
  deferred after shipping the channel half
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

A maintenance branch carries backports for an old line while the
default branch moves on. The planner already handles most of the flow
by construction — everything is computed from the checked-out
branch's manifests and tags, so a `1.x` checkout plans from the `1.x`
manifests and the `1.x` tags. The one hole is the bump derivation
itself: a breaking commit that lands on the branch (an accidental
`feat!:`, a mis-cherry-picked hunk) derives `Major` and plans
`2.0.0` — a release that silently abandons the line the branch
exists to serve. Nothing in the planner knows the branch is a
maintenance branch.

Constraints that shaped the design:

1. **The branch names its own line; the config only points at it.**
   `[versioning].maintenance_branches = ["1.x", "1.2.x"]` — each entry
   is the literal branch name AND the pinned line in one string
   (`MAJOR.x` or `MAJOR.MINOR.x`). No glob, no separate mapping table:
   the two things a maintainer must keep in agreement (branch, line)
   are one token, validated at resolve time (`E-CFG-MAINTENANCE`).
2. **Fail closed, never clamp.** A target that leaves the line is a
   refusal (`E-PLAN-MAINTENANCE` naming the package, the line and the
   offending target), not a silent clamp to the line's head. A clamp
   would publish a version the commits do not justify; a refusal
   surfaces the mis-landed breaking commit — the repo's "refusal, not
   a guess" instinct (ADR-0023's two-tool refusal, ADR-0026's name-less
   override refusal).
3. **The guard runs over the FINAL targets, once.** Rather than
   threading a Result through bump derivation, promotion and
   channelization, the guard is one pass after `channelize_targets`:
   the channelized triple is what is checked, so a train target whose
   stable part leaves the line refuses too. Forced (`Release-As`)
   targets stay exempt — the ADR-0025 exemption reasoning applies
   verbatim (explicit user intent beats derived policy).
4. **The pin rides the branch like the channel does (ADR-0025 step 2
   shape).** `ResolvedConfig::maintenance_line_for(branch)` is pure;
   the shell resolves the checked-out branch in `context()` and
   applies it *before* the §8.7 `config_hash` is computed, so planning
   `1.x` (pinned) and `main` (free) never share attributions.
5. **The §10.2 pre-major remap applies first; the guard is the last
   gate.** On `0.1.x` a breaking commit remaps to `minor` (`0.2.0`) —
   which still leaves the line and refuses. The guard checks the
   final target, not the derived bump, so the remap cannot smuggle a
   line crossing through.

## Decision

- `halyard-core`: `MaintenanceLine` (parse the documented grammar,
  `matches` the final target, render back to `1.x` / `1.2.x`),
  `PlanConfig::maintenance_line` (the branch-resolved pin), and the
  `enforce_maintenance_line` pass in `plan()` — plus
  `PlanError::MaintenanceLine` (`E-PLAN-MAINTENANCE`).
- `halyard-config`: `[versioning].maintenance_branches` (a list of
  branch-naming entries), merged per-key low→high (the entry string IS
  the branch name, so dedupe-by-entry is the union with the higher
  layer winning), validated at resolve time into
  `ResolvedConfig::maintenance_lines` plus the pure
  `maintenance_line_for` resolution; the resolved field rides
  `config_hash`.
- `halyard-cli`: `context()` applies the branch-resolved pin next to
  the channel resolution, before the cache key.

## Consequences

- The `1.x` backport flow works end-to-end offline: check out `1.x`,
  land `fix:` commits, `halyard plan` derives the in-line patch; a
  breaking commit refuses with the fix named instead of planning
  across the line.
- A channel and a pin compose: `1.2.x` pinned plus
  `channel_branches = { "1.2.x" = "rc" }` runs an rc train whose
  triples stay on the line — the guard sees the channelized target.
- The refusal is branch-scoped: the same breaking commit on the
  unpinned default branch plans free, so nothing about the default
  flow changes.
- Pre-existing quirk recorded (not introduced here): the CLI fixtures'
  walk includes the tagged scaffold commit when no release base is
  selected, so base-derived and initial-release targets coincide on
  the repo's single-`feat`-after-tag fixture shape; the maintenance
  gates shape their fixture (`chore` scaffold) so the assertions hold
  under either base behavior.
