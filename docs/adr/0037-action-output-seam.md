# ADR-0037: the action-output seam (the §12 GitHub Action glue, step 1)

- **Status:** accepted
- **Date:** 2026-11-17
- **Milestone:** M10/§12 step 1 — the `halyard-action` crate lands
  (the first of §7.1's two planned-but-unbuilt slots to fill; the
  composite distribution and the remaining §12 modes stay with their
  milestones)
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

§12 specifies the GitHub Action distribution (the composite wrapper:
binary download, checksum + attestation verification, caching) and,
inside it, one command the wrapper execs: `halyard action`, with a
documented inputs/outputs contract. The outputs vocabulary is the
action's real deliverable — the §12 gate shape is a downstream step
gating on halyard's own outputs:

```yaml
- id: hly
  uses: halyard/action@v1
- if: steps.hly.outputs.released == 'true'
  run: ./build-docker.sh
```

The composite wrapper's halves (release-asset plumbing, artifact
attestation, `halyard-action/action.yml`) need releases to download —
M5.4's own-artifact slice — and stay with their milestone. The outputs
contract has no such dependency: the plan wire types exist (§5.8),
the plan mode is offline (I5), and the env-file grammar is small and
documented (VERIFIED quotes in `docs/research/github-actions-outputs.md`).
So step 1 lands the seam: the outputs vocabulary, the writer, and the
`plan` mode of the command the wrapper will exec — testable offline
end to end, wired today as `halyard action`.

## Decision

- **One crate, one seam** — `halyard-action` (layer: `core` +
  `serde_json`): the §12 outputs vocabulary over an injected
  [`ActionWriter`] (`set_output` / `append_summary` / `emit_warning`),
  following the repo's injected-effect convention
  (`PublishEffect`, `TagWriter`, `Forge`).
- **Two writers** — the real [`GithubActionsWriter`] (the
  `GITHUB_OUTPUT` / `GITHUB_STEP_SUMMARY` env files, resolved from the
  step environment) and the [`PreviewActionWriter`] (the ordered call
  log, the `MemoryForge` observability convention; a poisoned log is
  recovered, not an error). Outside a workflow step `from_env()` is
  honestly `None` and the caller renders through the preview — the
  outputs are the same, only the sink differs. That is what makes
  `halyard action` locally useful today: it prints exactly the
  env-file entries a real step would receive.
- **The env-file grammar is fail-closed, not best-effort**
  (VERIFIED 2026-11-17): single-line values ride the documented
  `{name}={value}` line, multi-line values the documented
  `{name}<<{delimiter}` heredoc. The delimiter is the fixed `HALYARD`
  (deterministic render, I3) and a value carrying it on its own line
  refuses `E-ACTION-OUTPUT` — the docs' own caution; rewriting a
  value silently would mean a downstream step gates on bytes halyard
  did not write. Output names validate against the file format's own
  delimiters (the docs define no name character set): non-empty ASCII,
  no control characters, no `=`.
- **The forecast is honest** — the §12 `plan` mode sets
  `released=false` and `published=false` because nothing was executed
  (I5); `releases_json` carries the planned rows with the `url` empty
  until a forge mode fills it (the §12 row grammar: name, version,
  tag, ecosystem, url); the per-package `{package}--version` /
  `{package}--tag` keys ride the plan order; `paths_released` carries
  the distinct edit sites sorted and deduplicated (I3) newline-joined.
  `plan_json` names the canonical plan file the mode writes
  (`halyard_core::canonical_json` — the same document the §11.3
  verifier accepts), defaulting to `halyard-plan.json` beside the run,
  overridable with `--plan-out`; a missing parent directory refuses
  `E-ACTION-PLAN` (the caller's contract, never invented).
- **The job summary renders deterministically** — the release table
  (package, version transition, bump, tag), the honest counts and the
  stated call surface ("zero API calls (offline forecast, I5)"). The
  §12 "timing breakdown" renders as the walk's own work counters
  (trees/entries/memo/bloom, the `HistoryStats` provenance), not
  wall-clock: a clock would break I3's byte-stable reports, and the
  wall-clock surface already exists as `--profile`'s Chrome trace.
  The summary chunks follow the runner's automatic-newline append
  semantics (a chunk without a trailing newline gets one).
- **The degraded-mode annotation is real** — §12's "Annotations:
  warnings for degraded modes (no commit-graph, shallow clone)" wires
  to the walk's own `HistoryStats::degraded` marker: the action emits
  `::warning::` exactly when the engine reports the §8.3 degraded
  walk. Nothing is annotated from a guess.
- **The remaining §12 modes stay out** — `auto|pr|release|check` land
  with their milestones (the forge/live executors they wrap are the
  same shapes `pr`/`release`/`publish` already drive); the value enum
  carries only `plan`, and the command doc says the rest are absent
  rather than stubbed.

## Consequences

- `halyard action` is a complete, offline-capable §12 step-1 surface:
  CI consumers can gate downstream steps on `releases_json` /
  `lib--version` / `paths_released` today, and the composite wrapper
  (M5.4's slice) will exec exactly this command.
- The env-file grammar's cost is pinned: the `action_render` bench
  carries `plan_outputs_40_releases` (≈ 38 µs over a 40-release plan —
  the render is not the step's cost), `env_file_entry_single_line`
  and `env_file_entry_heredoc` (sub-microsecond).
- The taxonomy grows `E-ACTION-OUTPUT` / `E-ACTION-SUMMARY` /
  `E-ACTION-PLAN`, each with its page under `docs/errors/`.
- The layer rule table gains `halyard-action` (workspace: core;
  external: serde_json) — the conscious §7.2 entry the gate requires
  when a crate lands.
- The mode enum is a one-variant value enum by design: the CLI has
  never shipped a stub arm, and §12's other modes will join as
  ordinary variants when their executors exist.
