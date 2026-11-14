# ADR-0012: the tag-push effect seam and its `git` subprocess

- **Status:** accepted
- **Date:** 2026-11-14
- **Milestone:** the live release shell, git side (§11.1 step 2 "push
  tags (single `git push`)" / §11.2)
- **Deciders:** autonomous implementation agent (the halyard-git crate
  doc carried a flat "no subprocess, no network" claim; documented here
  per §0 rule 3)

## Context

The tag phase's local half (annotated tag objects + create-only ref
updates) ships in-process through `gix` (`halyard-git::tag`). Its remote
half — pushing the created tags — cannot: the engine's minimal `gix`
build has **no network client and no push support**, and §11.1
explicitly sanctions the effect ("push tags (single `git push`)").
Meanwhile the crate doc claimed halyard-git "performs no subprocess (no
`git` binary, invariant I6) and no network (I5)" — a planning-path
property stated as a whole-crate one.

## Decision

1. The **`TagPusher`** seam lives in `crates/halyard-git/src/push.rs`:
   the trait (object-safe `&dyn`), the report vocabulary
   (`TagPushReport`/`TagPushRow`: `Pushed`/`AlreadyOnRemote`/
   `WouldPush`/`Failed{GitError}`), the pure wire-format parsers
   (`parse_remote_tag_targets`, `parse_porcelain_push` — pinned against
   output probed empirically) and the refspec builder (`refspecs_for`,
   explicit `refs/tags/X:refs/tags/X` — no surprise refs).
2. Two adapters, the `TagWriter` split's mirror:
   **`GixTagPusher`** (live) probes with one `git ls-remote --tags`
   round trip, classifies per tag (already-on-remote → skip; remote tag
   at a *different* commit → `E-GIT-TAG-CONFLICT`, never forced), then
   sends one `git push --porcelain <remote> <refspecs…>` and attributes
   per-ref outcomes from the porcelain rows. **`PreviewTagPusher`**
   (forecast) probes only what is probe-able without a process (the
   configured `remote.<name>.url`, read in-process via `gix` config) and
   forecasts every mutation as `WouldPush` — the dry run stays offline
   (I5).
3. The subprocess contract (I6/I7): argv arrays only, no shell strings;
   `GIT_TERMINAL_PROMPT=0` (an auth prompt would hang CI — the effect
   fails closed as `E-GIT-PUSH` instead); **no `--force` and no
   `--delete`** in the argv ever; the subprocess runs rooted at the
   opened repository path (the ambient process cwd is not inherited).
4. The planning read path keeps its property; the crate doc now scopes
   it: the *planning read path* performs no subprocess/network, and the
   §11.1 effect steps are the sanctioned exceptions (`tag` writes
   in-process; `push` spawns `git`).

## Consequences

- `halyard release` (live) becomes the second repository-mutating CLI
  command (after `apply`), still forge-free: the machine's forge half
  stays deferred (M4.1b) and is reported as deferred, never faked.
- The `E-GIT-PUSH` code joins the taxonomy (page + index) before the
  first real-remote use, like the other pre-network codes.
- A repo without a configured `remote.<name>.url` renders the push phase
  as an honest skip (`remote: null`), not an error — local-only releases
  are a state, not a failure.
- I2 resumability holds end to end: a partially-pushed phase re-runs,
  the probe classifies what landed, only the remainder is sent, and a
  divergent remote tag fails closed instead of being overwritten.

## Alternatives considered

1. **Push inside `halyard-git`'s read path via a gix feature set with
   networking** — rejected: gix 0.90 has no push support; widening the
   feature set would pull the transport stack into the planning binary.
2. **Push effect in the CLI shell only** (like the native lockfile
   refresh orchestration) — rejected for this seam: the wire formats,
   the conflict classification and the forecast writer are contract
   surface tests and benches pin in the engine crate; the CLI keeps only
   the wire-up and the identity/clock ownership (its own shell job).
3. **Batch push without `--porcelain`** (parse human stderr) — rejected:
   the porcelain format is stable and per-ref; human stderr is
   locale-flavored prose.
