# ADR-0050: the M7 step-2 forge lane — the Forgejo/Gitea REST adapter

- **Status:** accepted
- **Date:** 2026-11-24
- **Milestone:** M7 step 2 (§17 Year-2 "more forges": "GitLab (MR flow,
  CI component), Gitea/Forgejo, Bitbucket; forge conformance test-suite
  shared across adapters" — the gate is "same e2e matrix passes per
  forge"); the Forgejo/Gitea adapter lands here; the GitLab CI
  component, Bitbucket and the corpus-lane e2e stay their own slices
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

ADR-0049 shipped the M7 gate's machinery — the shared conformance suite
(`halyard_forge::conformance`) and the GitLab MR-flow adapter — and
recorded the remaining rows verbatim: "the GitLab CI component,
Forgejo/Gitea, Bitbucket, the corpus-lane e2e". The seam's own decline
message still read "needs a GitHub or GitLab origin". The next lane's
`VERIFY` artifact already existed and was fresh:
`docs/research/forgejo-rest-forge.md` (the research note names this ADR
as its decisions record). Every dependency the plan lists for the lane
exists in code: the [`Forge`] seam with the optional-capability
declines (M4.1a), the transport vocabulary with the scripted fake
(M4.1b), the §11.5 governor (M5.5), the conformance suite (M7 step 1),
and the CLI origin-resolution skeleton with two lanes wired (GitHub,
GitLab).

## Decision

- **One adapter, two names** — `halyard-forge-forgejo` implements the
  §11.4 [`Forge`] over the REST v1 surface Forgejo inherits from Gitea
  (research §1): the pull-request flow (find by the documented
  `state=open&head=` filter, confirmed client-side on the parsed
  `head.ref`; create `POST …/pulls` with `head`/`base`/`title`/`body`;
  rewrite `PATCH …/pulls/{index}`), the releases API's
  GET-by-tag-first, and the issue comments' marker scan (a pull request
  **is** an issue on this API; the endpoint documents no pagination
  parameters, so the scan walks the array once). The origin grammar
  recognizes the two hosted instances — `codeberg.org` (Forgejo) and
  `gitea.com` (Gitea) — over the https / `git@` / `ssh://` forms;
  self-hosted instances ride `FORGEJO_API_URL` (the base override
  carries the `/api/v1` root, never appended implicitly).
- **Auth is the spec's `AuthorizationHeaderToken`** —
  `Authorization: token <token>` on every call, the spec's own wording
  ("API tokens must be prepended with `token` followed by a space").
  An empty token fails closed `E-FORGE-AUTH` before any call. No
  `ring` edge: the adapter is token-only, there is no App-JWT surface
  to sign. The shell's env grammar is `FORGEJO_TOKEN` first, then the
  Gitea-native `GITEA_TOKEN` name for gitea.com origins, then the
  nightly job's `HALYARD_FORGE_TOKEN` contract — three names because
  the adapter is deliberately one (a gitea.com operator looks for the
  Gitea name); `doctor` names all three.
- **The close shape is verified, not carried over** — the research
  note's §2 stopped at create/rewrite; this session verified the close
  against the live codeberg.org spec (research §8, this session's
  fetch): `PATCH …/pulls/{index}` with
  `EditPullRequestOption.state = "closed"`. The documented response
  table (`201`/`403`/`404`/`409`/`412`/`422`) maps `409` onto
  `E-FORGE-CONFLICT` — a documented conflict the GitLab close did not
  carry. The vanished-pull race (404 between the find and the close)
  reports success: the end state is already the intended one (I1).
- **`commit_via_api` is the documented honest decline, kept** — the
  contents API writes ONE file per commit and a release commit carries
  the plan's whole `EditSet` as one `chore: release` commit (research
  §5); a per-file commit train is semantically wrong for that, so the
  adapter answers `ForgeError::Unsupported` naming itself
  (`"ForgejoRestForge"`), and the Forgejo lane rides `commit_mode =
  "git"` (the §11.2 CAS push the M4.2 remainder shipped). The
  conformance matrix reports the decline as the `Declined` verdict —
  the exact case the M7 suite's verdict model was built for; the
  adapter's matrix row is eight passes + one decline, asserted
  explicitly.
- **The conscious differences from the other lanes, recorded and
  pinned** — no ETag conditionals (not defined on this surface:
  research §7), and the `RateInfo` snapshot stays an honest zero until
  a deployment sends quota headers (research §6: none are defined;
  `429` still maps to `E-FORGE-RATE-LIMIT` with the standard
  `retry-after` per RFC 9110 §10.2.3, else the 60 s floor). The
  quota-header snapshot logic still records `x-ratelimit-*` when a
  deployment sends them — the observed-absence posture, not a blind
  zero.
- **The CLI resolves the lane from the origin's shape** —
  `forge_credentials` and the release forge half grow a Forgejo branch
  after the GitLab one (the adapter's own remote grammar, the
  `FORGEJO_API_URL` base override for self-hosted + the offline
  loopback lane). A committed (`HALYARD_APP_*`) App env on a
  Forgejo/Gitea origin fails closed naming the mismatch — App auth is
  the GitHub lane's surface. The foreign-origin decline and the
  release-half skip message now read "GitHub, GitLab, or Forgejo/Gitea
  origin"; every other forge stays an honest decline. `forge-smoke`
  remains the GitHub probe (the nightly job's contract shape is
  GitHub's); the Forgejo lane's live proof is the `pr` loopback e2e
  plus the conformance matrix.
- **The layer rule mirrors the GitLab rule minus `base64`** —
  `halyard-forge-forgejo → halyard-core + halyard-forge` with its own
  blocking `ureq` client (the forge adapters are the HTTP clients of
  the runtime graph; the seam crate stays on core only). No `base64`
  edge: there is no commits-API payload encoder to ship — the decline
  is the capability's whole surface.

## Consequences

- The M7 gate "same e2e matrix passes per forge" now runs over three
  REST lanes + the two reference adapters; the Bitbucket adapter (the
  remaining M7 row) inherits the proof by wiring its factory into
  `run_conformance_matrix`.
- The doctor's token check and the CLI error grammar name all three
  lanes; the forge release half reports its skips with the lane's own
  token name (`FORGEJO_TOKEN`).
- The remaining M7 rows (the GitLab CI component, Bitbucket, the
  corpus-lane e2e) are unchanged open items — recorded, not silently
  re-scoped.
- A pre-existing CLI-fixture quirk surfaced during the loopback e2e and
  is left as recorded (not silently "fixed"): a fixture repo with no
  base tag selected plans with `base_branch = "HEAD"` (PROGRESS row
  §10.3's recorded quirk), so the derived pull base is the release
  branch's own segment verbatim — the derivation is the contract, the
  planner's base resolution is its own slice.
