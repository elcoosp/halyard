# ADR-0051: the M7 step-3 forge lane — the Bitbucket REST adapter

- **Status:** accepted
- **Date:** 2026-11-24
- **Milestone:** M7 step 3 (§17 Year-2 "more forges": "GitLab (MR flow,
  CI component), Gitea/Forgejo, Bitbucket; forge conformance test-suite
  shared across adapters" — the gate is "same e2e matrix passes per
  forge"); the Bitbucket adapter is the row's last adapter slice
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

The M7 row's remaining adapter is Bitbucket. Every dependency the plan
lists already exists in code: the `Forge` seam (M4.1a), the transport
vocabulary (M4.1b), the conformance suite (M7 step 1, ADR-0049) with the
Forgejo adapter as the newest lane (M7 step 2, ADR-0050), and the CLI's
origin resolution with the per-lane env grammars. What the plan asks for
that does *not* exist is the Bitbucket lane itself. The shapes are
VERIFIED first (`docs/research/bitbucket-rest-forge.md`): Bitbucket
Cloud's REST 2.0 has a pull-request flow (find by the documented `q`
grammar, create/rewrite, close by the **decline** POST), a PR-comment
surface with documented `page`/`pagelen` pagination, and — decisive —
**no Releases API group at all** ("Addon"…"Workspaces", verbatim; the
commits resource "does NOT support new commit creation").

## Decision

- **The adapter is the Forgejo crate's twin** (`BitbucketRestForge` over
  an injected `HttpTransport`, its own blocking `UreqTransport`), wired
  through the conformance suite's factory — the gate is the same matrix.
- **The release marker is the git tag.** With no Releases group there is
  no release object to create and halyard invents none (§7.3):
  `release_exists`/`create_release` probe `refs/tags/{name}` (`404` is
  the documented not-found value); a present tag answers the existed
  shape with the tag's web URL (the git-side tag phase pushed it — the
  idempotent re-run is honest, I1), an absent tag declines
  `E-FORGE-UNSUPPORTED`. The conformance matrix's `MachinePropagate`
  counts `ReleaseExisted`, so the declined set stays exactly
  `commit_via_api`.
- **`commit_via_api` is the documented honest decline** — the commits
  resource "does NOT support new commit creation" (research §5); the
  lane rides `commit_mode = "git"`, like Forgejo's (no `base64` layer
  edge ships).
- **Auth is the OAuth 2.0 Bearer scheme** — the documented single-token
  scheme ("the access token placed as a Bearer token in the request
  Authorization header", research §1). API tokens and repository access
  tokens ride Basic auth with a second credential (the account email /
  `x-token-auth`), outside the single-token env grammar; the doctor
  names it. Env grammar: `BITBUCKET_TOKEN`, then
  `HALYARD_FORGE_TOKEN`; base override `BITBUCKET_API_URL`.
- **The close verb is the decline POST** (`POST …/pullrequests/{id}/decline`,
  research §8) — not a state field on the update PUT; the close path
  probes first (the find filter pins `state = "OPEN"`), so an
  already-declined pull is the not-found value before any POST.
- **Conscious absences** (research §6/§7): no ETag conditionals (not
  documented on this surface), and the quota snapshot records only the
  documented `X-RateLimit-Limit` — no remaining-requests header exists,
  so `remaining` stays the honest zero; `429` honors `retry-after` when
  a deployment sends it, else the shared 60 s floor.

## Consequences

- The M7 gate runs green over the fourth REST lane; the declined set is
  pinned (`commit_via_api_capability` only) and the CLI's foreign-origin
  decline and doctor's token check name four lanes.
- The corpus-lane e2e and the GitLab CI component stay the M7 rows'
  remaining open items — recorded, not silently re-scoped.
