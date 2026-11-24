# ADR-0049: the M7 step-1 forge lane — the shared conformance suite and the GitLab MR-flow adapter

- **Status:** accepted
- **Date:** 2026-11-24
- **Milestone:** M7 step 1 (§17 Year-2 "more forges": "GitLab (MR flow,
  CI component), Gitea/Forgejo, Bitbucket; forge conformance test-suite
  shared across adapters" — the gate is "same e2e matrix passes per
  forge"); the GitLab MR-flow half and the conformance suite land here;
  the GitLab CI component, Forgejo/Gitea and Bitbucket stay their own
  slices
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

The forge seam's own error message deferred the next lane: "live forge
mode needs a GitHub origin … foreign forges are Year-2 milestones" —
and §17's M7 row is that milestone. Every dependency the plan lists for
it already exists in code: the [`Forge`] seam with the optional
capability declines (M4.1a), the transport vocabulary with the scripted
fake (M4.1b), the §11.5 governor decorator (M5.5), the CLI origin
resolution with the honest foreign-origin decline (M4.1b/c), and the
GitHub REST adapter as the reference REST lane. What the plan asks for
that does *not* exist is two-fold: a **shared conformance suite** (the
same e2e matrix passing per forge, instead of each adapter pinning the
machine's semantics alone) and the **GitLab MR-flow adapter** itself.

## Decision

- **The conformance suite lives in the seam crate**
  (`halyard_forge::conformance`), not in test-support: it is pure code
  over the [`Forge`] trait, needs no fixture machinery, and every
  adapter crate can dev-depend on the seam it already implements — no
  cycle. The matrix is nine named scenarios (`Scenario::ALL`), each run
  against a *fresh* adapter produced by the factory
  (`ForgeFactory = &mut dyn FnMut(Scenario) -> Box<dyn Forge>`): the
  `Scenario` argument lets a scripted adapter pre-load exactly the
  responses its scenario's conversation produces, which is the
  adapter's conformance wiring. The scenarios assert only the
  invariants every adapter owes: the §11.4 not-found values, the I1
  upsert echo (same branch ⇒ same PR number), the three §11.1 machine
  passes (propagate, prepare-upsert, prepare-noop) and the §11.5
  observable call log.
- **The documented honest decline is a verdict, not a failure** — the
  trait's optional capabilities (`close_pr`, `commit_via_api`,
  `upsert_pr_comment`) default to `ForgeError::Unsupported` naming the
  adapter (§7.3's default-decline convention); the suite maps that to
  the `Declined` verdict and `is_green()` counts it as green. Every
  other error is `Failed` carrying the adapter's own diagnosis.
  Persistence nuances (GET-by-tag-first, rewrite-in-place, closed-PR
  non-reuse) stay in each adapter's own contract tests — the
  `PreviewForge` forecast persists nothing by design, so a persistence
  assertion would be a forecast bug, not a conformance one.
- **The GitLab adapter is a full lane, not a stub** — the new
  `halyard-forge-gitlab` crate implements every [`Forge`] method over
  REST v4 (shapes verified in `docs/research/gitlab-rest-forge.md`):
  the MR flow (find by `source_branch`, create/rewrite `PUT …/:iid`,
  close via `state_event`), the releases API's GET-by-tag-first, the
  notes API's marker scan (with the `sort=asc` the GitLab default
  ordering makes mandatory), and the commits API's base64 actions for
  the §11.5 `commit_mode = "api"` half. The `PRIVATE-TOKEN` header is
  the credential (a PAT; the CI-job-token form is a later slice); the
  project path rides as the URL-encoded `:id` segment (nested groups
  preserved).
- **The ADR-0013 decision-6 mapping carries over unchanged** — the MR
  target branch is the §11.2 release prefix stripped, so
  `halyard/release/main` targets `main` identically on both lanes, and
  a non-release branch fails closed before any call.
- **Two conscious differences from the GitHub adapter, recorded and
  pinned** — no ETag conditionals (GitLab does not document them on
  this surface, so the adapter sends unconditional probes and the
  "≤ 2 calls on the update path" budget holds without a `304` lane),
  and the `RateInfo` snapshot stays an honest zero until a deployment
  sends quota headers (a fabricated quota is worse than an observed
  absence; `429` still maps to `E-FORGE-RATE-LIMIT` with the standard
  `retry-after`).
- **The commits API replaces the tree/blob choreography** — one
  `POST /projects/:id/repository/commits` with `actions[]` carries the
  payloads (`encoding: "base64"`), but the API's `create`/`update`
  action kinds are exclusive, so the adapter probes each file's
  existence first (the probe is recorded like every call, §11.5: the
  budget is files + one commit, observable).
- **The CLI resolves the lane from the origin's shape** — the
  `forge_credentials` resolution and the release forge half grow a
  GitLab branch ahead of the GitHub slug parse: the adapter's own
  remote grammar (https / `git@` / `ssh`, nested groups), `GITLAB_API_URL`
  as the base override (self-hosted + the offline loopback lane,
  mirroring `GITHUB_API_URL`), `GITLAB_TOKEN` first then the nightly
  job's `HALYARD_FORGE_TOKEN` contract. A committed (`HALYARD_APP_*`)
  App env on a GitLab origin fails closed naming the mismatch — App
  auth is the GitHub lane's surface. The foreign-origin decline now
  reads "needs a GitHub or GitLab origin"; every other forge stays an
  honest decline. `forge-smoke` remains the GitHub probe (the nightly
  job's contract shape is GitHub's); the GitLab lane's live proof is
  the `pr` loopback e2e plus the conformance matrix.
- **The layer rule widens consciously** — `halyard-forge-gitlab →
  halyard-core + halyard-forge` with its own blocking `ureq` client
  (the forge adapters are the HTTP clients of the runtime graph; the
  seam crate stays on core only). No `ring` edge: the adapter is
  token-only, there is no App-JWT surface to sign.

## Consequences

- The M7 gate "same e2e matrix passes per forge" is now executable:
  the suite runs green over `MemoryForge`, `PreviewForge`, the GitHub
  REST adapter (scripted routing transport) and the GitLab REST
  adapter (same), and every future adapter (Forgejo/Gitea, Bitbucket —
  the M7 rows that stay open) inherits the proof by wiring its factory
  into `run_conformance_matrix`.
- The doctor's token check and the CLI error grammar now name both
  lanes; the forge release half reports its skips with the lane's own
  token name (`GITLAB_TOKEN`).
- The remaining M7 rows (the GitLab CI component, Forgejo/Gitea,
  Bitbucket, the corpus-lane e2e) are unchanged open items — recorded,
  not silently re-scoped.
