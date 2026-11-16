# ADR-0032: the affected-package selector (`halyard affected`)

- **Status:** accepted
- **Date:** 2026-11-16
- **Milestone:** M10 step 1 — the affected-package selector the
  architecture has declared since M1.10 (§16.3 "first-class monorepo
  ergonomics: `--package`, `--since`, `--changed` selectors"; §17 M10
  "affected-package selectors for CI (`halyard affected --base
  origin/main`)"), landed ahead of M10's scale work because the seam is
  self-contained and the CI consumers need it independently of the
  10k-package performance targets
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

Every CI system that touches a monorepo eventually asks one question:
*which packages does this change actually affect?* halyard already holds
both halves of the answer — the per-package pending-commit attribution
(the §8.3/§8.4 walk + tree-diff digest, `HistoryDigest::per_pkg`) and
the workspace's dependency graph (§6.6, `WorkspaceGraph` with its CSR
in-edges) — but nothing composed them. `plan` answers the release
question (what to bump, in release order); CI needs the selection
question (what to rebuild and test, before any release exists), against
an arbitrary base ref (`origin/main`) rather than only against release
tags.

Constraints that shaped the design:

1. **Purity.** The selection is a pure function of the graph and the
   changed set (§4): no I/O, no clocks, deterministic (I3). The shell
   owns the walk; the engine owns the closure.
2. **CI semantics, not release semantics.** The publish order (§6.6,
   §10.4) deliberately excludes dev edges; a CI selector must include
   them — a dev-dependency still runs the dependent's test suite
   against the changed package. This is a documented divergence, not an
   oversight.
3. **The `--base` shape is part of the capability.** The M10 spelling
   is `halyard affected --base origin/main`; without an explicit-ref
   walk the selector could only ever answer "what is pending release",
   which `plan` and `status` already answer. The git engine gained the
   minimal option that makes the walk honest against any ref.
4. **Fail-closed refs.** An unresolvable `--base` ref must refuse
   (`E-GIT-REF`, naming the ref) — a guessed base would misattribute
   the pending work silently, the one failure mode the selector has no
   way to detect downstream.

## Decision

- **`halyard-core/src/affected.rs`** — the pure engine:
  - `changed_from_digest(&HistoryDigest) -> Vec<PkgId>` — the packages
    with pending commits (the change roots), in package order.
  - `affected_selection(&WorkspaceGraph, &[PkgId]) -> AffectedSelection`
    — the reverse BFS: change roots seeded ascending, then every
    affected package's `in_neighbors` visited (all edge kinds, dev
    included). Rows are ascending-`PkgId` sorted; `AffectedReason` is
    `Changed` or `Dependent { via }` with `via` the **nearest** affected
    ancestor — the first discoverer over the sorted CSR in-edges, so
    one graph yields one attribution. A package that is itself changed
    is never demoted to a dependent row. Foreign ids are skipped, not
    fatal.
- **`GitHistoryOptions.base_override`** — the git engine's additive
  option: one explicitly named ref resolves (gix reference-search
  rules: branch, remote-tracking ref, tag; annotated tags peel) and
  replaces **every** package's release base for one walk; the digest's
  bases carry the ref name so the report states what was selected
  against. Unresolvable → `E-GIT-REF` with the fix named (including the
  shallow-clone guidance). Per-commit cache entries stay valid: a
  commit's tree diff does not depend on where a walk starts.
- **The CLI `affected [--base <ref>]`** — the same pipeline `plan` runs
  (discover → config → history walk, cache included), then the pure
  closure, rendered in all three formats (json: `base` / `changed` /
  `packages[{package, reason, via}]` / `total`, name-sorted, byte-stable
  I3; human: the aligned table; gha: the markdown table plus the
  machine-greppable `affected=N changed=M` tally). No new exit code —
  the §15.4 codes stay untouched; CI consumers read the JSON.
- **No new error code.** The refusals reuse `E-GIT-REF` (unknown base)
  and `E-GIT-HEAD` (unborn HEAD) — the taxonomy stays exact; both pages
  already carry producers.

## Alternatives rejected

- **A `--changed` flag on `plan`**: conflates two contracts. The plan
  is the frozen release wire format (I3, `plan_hash`); a CI selector is
  a different question with different semantics (dev edges, arbitrary
  base, no bump levels). A separate command keeps the plan
  contract untouched and gives CI a stable document of its own.
- **Deriving the closure from the planner's cascade**: the cascade
  computes *bump propagation* under release semantics (non-dev edges,
  requirement satisfaction); the CI selection needs *reverse
  reachability* under test semantics. Reusing it would silently drop
  dev edges and release-base-only bases.
- **Transitive closure materialized in the graph crate** (`closure()`
  on `WorkspaceGraph`): the generic closure has no `via` attribution
  and no reason model; the selector owns the row vocabulary. The graph
  stays a view (§6.6); the BFS lives with its data.
- **Shelling out to `git rev-parse` for `--base`**: the engine owns
  in-process resolution (I6 — no subprocess during planning); gix's
  reference search is the same machinery `resolve_bases` already uses.

## Consequences

- CI can gate jobs on `halyard affected --base origin/main --format
  json` offline (I5) — the same walk the plan runs, one graph pass
  (≈85 ns chain-of-40, ≈547 ns fan-out-39, pinned by the `affected`
  bench group).
- The M10 scale work (10k packages, sparse checkout, Turborepo/Nx/Bazel
  exports) inherits the selection seam; its remaining scope is
  performance and integration, not semantics.
- `--since`/`--changed` selectors on other commands (the §16.3 DX
  principle) can reuse `changed_from_digest` + the closure; the
  selector's types are the shared vocabulary.
- npm/pnpm ecosystems need no special case: the workspace graph's edges
  are ecosystem-agnostic (§6.5), and the attribution is path-based.
