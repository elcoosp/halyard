# ADR-0025: prerelease-channel planning in the core planner (M5.6 step 1, §10.3)

- **Status:** accepted
- **Date:** 2026-11-15
- **Milestone:** M5.6 step 1 — the §10.3 prerelease channels become a
  planning capability: a configured `[versioning].channel` turns every
  planned release into `{target}-{channel}.{n}`, the counter derived
  from the tags the repository already carries (no state file), and
  removing the key promotes a leftover prerelease manifest to its own
  stable triple
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

The M1.3 slice shipped the §10.3 algebra — `advance_prerelease`,
`promote_to_stable`, the `PrereleaseChannel` grammar — in
`halyard-version` with tests and an example, but nothing consumed it:
the planner always planned stable bumps, and the roadmap's M5.6 row
("prerelease channels + maintenance-branch release trains; fixtures:
`next`/`rc`, `1.x` backports") stayed unstarted. Every consumer that
wanted a prerelease train needed a manual `Release-As` footer per
release — the one stateful, error-prone path through an otherwise
stateless planner.

Constraints that shaped the design:

1. **The planner stays pure and stateless (I3/I6).** §10.3 says the
   counter is "derived from existing tags — no state file". The
   planner's inputs are the manifest and the release base tags, so the
   counter witnesses are exactly those: the base tag's parsed version
   (the newest tag on the train — the resolver already picked it) and
   the manifest version itself (which witnesses the applied-but-untagged
   case where the manifest is ahead of the base tag).
2. **The algebra lives where the planner can call it (ADR-0008
   precedent).** `halyard-core` cannot depend on `halyard-version`, and
   the planner composes `bump.rs` directly. The channel half therefore
   moved to `halyard-core::bump` (with `promote_to_stable`), and
   `halyard-version` re-exports the paths unchanged — the M1.3 public
   API and its test suite stay where Slice A put them.
3. **A train step that does not move the triple must not fake a bump.**
   On an active train (the base tag itself is a prerelease), a
   non-breaking step is absorbed into the train's committed triple:
   `1.3.0-rc.1` + one `fix` plans `1.3.0-rc.2`, not `1.3.1`. The row's
   `bump` reports the bump the commits derived, never `none` while a
   release is owed. Only a breaking step (`Major`) opens a new triple
   (`2.0.0-rc.1`). Without a channel the derivation is byte-identical
   to the stable planner (the golden plans did not move).
4. **Channels are parallel tracks, not an order.** Switching
   `rc` → `beta` may emit `1.3.0-beta.1` — semver-below the previous
   `rc.1` tier. The plan's skip guard is therefore equality with the
   manifest version, never `<=` (which would silently swallow the
   switch).
5. **Promotion is a lifecycle act, not a guess.** Removing the key with
   nothing new landed plans the manifest's own stable triple
   (`1.3.0-rc.2` → `1.3.0`) — the §10.3 "promotion `rc → stable` drops
   suffix" rule — unless the manifest is behind the latest release tag
   (the reconciliation reports that state and never tags downward). A
   migration never sets the key (ADR-0022's draft carries only what the
   foreign config maps).
6. **`Release-As` stays the escape hatch.** A forced target is the
   user's explicit version; the channel never rewrites it.
7. **Fail-closed config.** An invalid channel name is a new stable
   error (`E-CFG-CHANNEL`) raised at resolve time, so `plan_config`
   stays total and the CLI fails before any planning.

## Decision

- `halyard-core::bump` gains `PrereleaseChannel` and
  `advance_prerelease` (moved from `halyard-version`, re-exported
  unchanged) plus the internal `same_triple`.
- `PlanConfig` gains `prerelease_channel: Option<PrereleaseChannel>`;
  `materialize_versions` becomes train-aware (absorbed triple on an
  active train), `promote_pending_stable` fills the promotion gap, and
  `channelize_targets` applies the counter from the base/manifest
  witnesses before requirement rewrites (so dependent manifests carry
  the exact prerelease triple, the rewrite.rs exact-triple path).
- `halyard-config` gains `[versioning].channel` (file layer; the env
  layer's documented variable set is unchanged), validated at resolve
  time into `ResolvedConfig::prerelease_channel`, merged low→high,
  included in `config_hash` (a channel change invalidates the cache
  key, §8.7) and mapped into `plan_config`.
- The tag phase and the reconciliation need no changes: the §11.1
  manifest-vs-tags decision already compares full semver versions, so
  a prerelease manifest tags through the same template rule, and
  `1.3.0 > 1.3.0-rc.2` makes promotion pending correctly.
- **Step 2 — the branch binding.** §10.3's "Branch-bound channels
  (`next` branch ⇒ `rc`)" ships as `[versioning].channel_branches =
  { next = "rc" }`: branch name → channel, validated at resolve time
  into `ResolvedConfig::channel_branches` (an invalid binding value is
  `E-CFG-CHANNEL` naming the branch), merged per-key low→high like
  `[commits.types]`. The resolution is the pure
  `ResolvedConfig::channel_for_branch(branch)`: a binding wins over the
  unbound default, `None` (detached/unborn HEAD) plans with the
  default. The CLI resolves the checked-out branch once in `context()`
  (`GITHUB_HEAD_REF` wins — the §11.5 workflow view — else the gix
  HEAD referent; the lenient `checked_out_branch` helper degrades to
  `None`, the binding being an optimization of the default, never a
  failure surface) and applies it *before* the §8.7 `config_hash` is
  computed, so planning `next` and `main` never share attributions.

## Consequences

- The M5.6 `next`/`rc` fixture flows work offline end-to-end:
  plan → apply → tag → re-plan all agree on `v1.3.0-rc.N` tags.
- The maintenance-branch half of M5.6 (branch-bound channels,
  `1.x` backports) is the declared next slice — the channel policy is
  in place; binding it to a branch is config resolution over the same
  seam.
- The train triple absorbs non-breaking steps until a breaking commit
  or a `Release-As` moves it; a repo wanting a new minor inside a
  train promotes, re-channels, or force-releases explicitly.
- Known shape gap (documented, not hidden): per-package channels
  (a `[[package]] channel` override) are deferred — the policy is
  repo-wide like `pre_major`, the one place a train definition lives.
