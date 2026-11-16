# ADR-0026: per-package channel overrides (M5.6 step 3, §10.3)

- **Status:** accepted
- **Date:** 2026-11-16
- **Milestone:** M5.6 step 3 — the `[[package]] channel = "…"` override
  names a package's prerelease train, closing the known shape gap
  ADR-0025 recorded ("per-package channels are deferred — the policy is
  repo-wide like `pre_major`")
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

ADR-0025 shipped the channel policy repo-wide: one
`[versioning].channel` (plus `channel_branches` bindings) governs every
package in the workspace. Real polyglot workspaces are not that flat —
the canonical case is a library trained on `rc` while an internal tool
in the same repo ships stable, or one package demoing the next channel
(`beta`) while the rest stay on `rc`. The only per-package escape today
is a `Release-As` footer on every release of the deviating package:
stateful, manual, and exactly the path ADR-0025 called error-prone.

Constraints that shaped the design:

1. **The override is the most explicit policy, so it wins everywhere.**
   Precedence is `[[package]] channel` > `channel_branches` binding >
   unbound `[versioning].channel` > stable. The rationale is the same
   one ADR-0025 applied to `Release-As` exemptions: explicit user intent
   beats environment-derived policy. Concretely, a package whose
   override names `rc` keeps its train even when HEAD sits on a branch
   bound to `beta` — the binding governs the repo, the override governs
   the package.
2. **Exact-name matching, like `release = false`.** The planner's
   existing per-package set (`release_disabled`) matches overrides by
   exact package name; path-glob overrides refine include/exclude
   filters only. A channel override follows the same rule, so
   `plan_config` stays total — no workspace introspection needed to
   resolve globs. A `channel` on a name-less override cannot be applied
   and fails closed (`E-CFG-CHANNEL`, the cause named), never guessed.
3. **One resolution point, three consumers.** The effective channel is
   `PlanConfig::channel_for_package(name)`: the override map, else the
   repo-wide default. Every channel decision point — the
   active-train absorption rule in `materialize_versions`, the
   per-package promotion gate in `promote_pending_stable`, and
   `channelize_targets` itself — consults that one method. The
   wholesale "channel configured ⇒ no promotion" early return is gone:
   promotion is now per package, so a repo with one trained package and
   one leftover prerelease manifest plans both correctly in a single
   pass.
4. **Validation and hashing ride the ADR-0025 rails.** Override
   channels validate at resolve time into
   `ResolvedConfig::package_channels` (an invalid value is
   `E-CFG-CHANNEL` naming the package), merge low→high through the
   documented `[[package]]` concatenation rule (later entries win when
   both match a package), and serialize into `config_hash` — a
   per-package channel change invalidates the §8.7 cache key exactly
   like a repo-wide one.
5. **The frozen wire format stays frozen.** The plan's `to` field
   already carries the full prerelease string; no `halyard-plan/v1`
   change, no schema bump beyond the new optional config key.

## Decision

- `halyard-config`: `PackageOverride` gains `channel: Option<String>`;
  the loader's `package.element` key list and the regenerated
  `halyard-config-v1` schema carry it; `resolve` validates the entries
  into `ResolvedConfig::package_channels` (BTreeMap, name-keyed).
- `halyard-core`: `PlanConfig` gains `package_channels:
  BTreeMap<String, PrereleaseChannel>` and the
  `channel_for_package` resolution; the three channel decision points
  switch from the repo-wide flag to the per-package resolution.
- `migrate` drafts keep `channel: None` — a migration never sets a
  channel (ADR-0022), and no foreign tool has a halyard-shaped channel
  concept to map.

## Consequences

- Mixed-policy workspaces plan in one pass: `[[package]] channel =
  "rc"` on the library, nothing else — the tool ships stable, the
  library runs its train, promotion splits per package.
- The override is absolute for its package: it does not participate in
  branch bindings (no per-package `channel_branches`). A package that
  must switch channels per branch uses the repo-wide binding plus a
  temporary override, or `Release-As` for one-off moves; the combinator
  explosion is deliberately not built.
- The M5.6 maintenance-branch backport flows (`1.x` release trains)
  remain the row's open half — they need a line-pinning decision, not a
  channel resolution.
- Cost: one BTreeMap lookup per package per decision point; the
  `plan_channel_override` bench pins the mixed two-package workspace at
  the repo-wide channel's cost shape (no re-planning pass).
