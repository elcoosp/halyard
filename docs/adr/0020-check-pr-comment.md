# ADR-0020: the check-comment capability on the forge seam (M4.5 step 1, §11.6)

- **Status:** accepted
- **Date:** 2026-11-15
- **Milestone:** M4.5 step 1 — the §11.6 "optional PR comment showing
  *if merged, this will release*" as the `check --pr-title --comment`
  mode: the `Forge::upsert_pr_comment` marker-upsert seam capability,
  the GitHub REST exchange, the §11.6 comment renderer in
  `halyard-changelog`, and the shell's live-only wire-up
- **Deciders:** autonomous implementation agent (per §0 rule 3; the
  comment REST shapes ride the research-verified issue-comments
  endpoints already pinned in `docs/research/github-rest-forge.md` — no
  new external facts; the marker convention reuses the §11.3 one)

## Context

§11.6 closes with one unshipped sentence: "Optional PR comment showing
'if merged, this will release: core minor'." Every session log since
M5.5 has carried it as a declared deferred item ("the
`github-release-notes` PR-comment mode"), and the `check` command —
the squash-merge gate that runs *in* PR jobs, where the comment has an
audience — shipped in M4.3 without it. The gap at base:

- the `Forge` seam had no comment write: a PR comment could not be
  posted, updated or made idempotent anywhere below the shell;
- the adapter's comment exchange (the issue-comments endpoints) was
  unwired, though the endpoint shapes were already research-verified
  alongside the pulls/releases ones;
- `check` had no path to a forge at all — its design runs credential-
  free, so the comment needed the same explicit live gating the `pr`
  live mode raised at M4.1b (`E-CLI-PR`), under its own code.

Constraints that shaped the design:

1. **Idempotence without a state file (I1/I3).** `check` runs on every
   PR push; the re-run must rewrite the same comment, never pile up
   duplicates. The other forge writes answer this with find-first
   conventions (`upsert_pr` finds by branch, `create_release` is
   GET-by-tag-first); a comment has no natural key on the wire — the
   marker has to be the key.
2. **The renderer stays pure and markdown-only (§4, §7.2).** The body
   is a render over a verdict, so it belongs beside the §11.3 Release-
   PR body renderer (the same plan-in/markdown-out shape), not in the
   CLI or the seam.
3. **The comment mode is live-only.** `check` has no dry mode; a
   forecast of a comment would either probe the network (I5) or lie
   about a target it cannot see — the honest decline is the same
   fail-closed shape the live `pr` mode uses.

## Decision

1. **The marker is the idempotence key.** The comment body's first line
   is the stable `<!-- halyard:check v1 -->` HTML comment
   (`halyard_changelog::CHECK_MARKER`), the §11.3 embedded-plan
   marker's convention at comment scale. `Forge::upsert_pr_comment`
   scans the PR's comments for the marker and rewrites the hit in
   place (same URL, `created: false`), or creates when the scan comes
   back empty — the seam's find-first convention, one level up. The
   scan walks the comment list ascending in pages of 100 until a short
   page (GitHub lists issue comments ascending by id), a complete
   deterministic walk; the version field lets the body layout evolve
   without re-finding old comments' shape.
2. **`CommentSpec::validate` is the shared fail-closed contract.** A
   1-based PR number, a non-empty marker, a non-empty body that
   carries its own marker — an invalid spec is `E-FORGE-REQUEST`
   before any store touch or exchange. A marker the body does not
   contain would make the comment unfindable (the upsert could never
   be idempotent), so it is refused, not corrected.
3. **The trait default is the honest decline** (`E-FORGE-UNSUPPORTED`
   naming the adapter), mirroring `close_pr`/`commit_via_api` and the
   lockfile seam's default-decline convention (§7.3). `MemoryForge`
   implements the capability fully (the reference model), `PreviewForge`
   forecasts it (the fresh path, `preview://` URL, nothing stored), the
   GitHub REST adapter implements it over `GET/POST/PATCH
   …/issues/{n}/comments`, and the governor treats the upsert as a
   content call (paced, budgeted, retried — §11.5).
4. **The renderer lives in `halyard-changelog`** (`check_comment`
   module): `CheckComment`/`CheckVerdict`/`CheckPrediction` plain data
   in, marker-first markdown out, deterministic (I3). Both verdict
   shapes render — a rejected title posts its diagnosis too, then the
   command still fails with `E-CLI-TITLE` (the comment is the check
   that teaches; the exit code is unchanged).
5. **The target is the PR found by head branch.** The shell resolves
   the run's head branch (`GITHUB_HEAD_REF`, else the checked-out
   branch through in-process `gix`) and probes the open PR whose head
   it is — the adapter's existing find exchange, not a new one. No
   open PR on that head is an honest skip (a value in the report,
   nothing posted); a forked PR's foreign-owner head resolves the same
   skip (the `head=owner:branch` filter finds nothing) — fork-aware
   targeting stays M11's bot mode, documented on `E-CLI-COMMENT`.
6. **The shell's gate is fail-closed under its own code.** Credentials
   and origin resolve before any verdict processing; the decline is
   `E-CLI-COMMENT` (the `E-CLI-PR` gate's shape, check-mode-scoped)
   naming the fix. The live call rides the §11.5 governor like every
   other live forge site.

## Consequences

- The `Forge` trait gained a method — every adapter below the seam
  inherits the honest-decline default, so no adapter silently breaks;
  the `Forge`-consumption tests (contract, loopback, CLI) pin the
  comment path end-to-end over scripted transports, the in-memory
  model and a loopback HTTP fake.
- `check` gains its first live effect. Its offline gates (`check` with
  no flags, `--pr-title` without `--comment`, the `E-CLI-TITLE`
  diagnoses, the exit-3 pending gate) are untouched and byte-stable —
  the `comment` report key exists only when the flag ran.
- The comment is rewrite-in-place idempotent (I1) and deterministic
  (I3): the same verdict re-renders byte-identically, the marker finds
  the same comment, and a re-run is a PATCH — the §15.2 discipline
  holds for the new effect without new state.
- What stays open: fork-aware comment targeting (M11 bot mode), a
  `--dry` forecast of comments (deliberately absent — see Decision 3),
  and reactions/review-comment threads (no §-requirement; the flat PR
  comment is the documented surface).
