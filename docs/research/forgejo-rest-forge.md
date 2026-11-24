# Forgejo REST forge adapter — verified request/response reference (M7 step 2)

> **Status:** verified 2026-11-24 against the official Forgejo v1 API
> specification (`https://try.next.forgejo.org/swagger.v1.json`, the
> instance `forgejo.org/docs/latest/user/api/` names as the browsable
> spec's source) and the forgejo.org API-usage documentation. Every
> claim below was read from a cited source; nothing is carried over
> from the §11 sketch un-verified. This note is the `VERIFY` artifact
> §0 rule 4 requires before the adapter lands; the M7 row
> (`docs/HALYARD_ARCHITECTURE.md` §17: "… Gitea/Forgejo, Bitbucket;
> forge conformance test-suite shared across adapters") is the plan
> source. ADR-0050 records the resulting decisions.

## 1. One adapter, two names: Forgejo and Gitea

Source: <https://forgejo.org/docs/latest/user/api/> and the spec's own
security definitions.

- Forgejo is a soft fork of Gitea and inherits its REST v1 surface;
  the plan's "Gitea/Forgejo" bullet is one adapter (`halyard-forge-forgejo`)
  over that shared shape. The hosted instances this adapter's origin
  grammar recognizes are `codeberg.org` (Forgejo) and `gitea.com`
  (Gitea); self-hosted instances ride the `FORGEJO_API_URL` base
  override.
- The API root is `/api/v1` (the spec's `basePath`).
- Auth: the `Authorization` header with the token **prefixed by
  `token `** — the spec's `securityDefinitions.AuthorizationHeaderToken`
  reads: "API tokens must be prepended with `token` followed by a
  space". The adapter sends `Authorization: token <token>` on every
  call; an empty token fails closed before any call.
- `User-Agent` rides the `halyard/<version>` convention (the GitHub
  adapter's; good HTTP citizenship, documented as required by neither
  spec).

## 2. Pull requests (the Release-PR surface, §11.1 step 3)

Source: the spec's `/repos/{owner}/{repo}/pulls` operations and the
`CreatePullRequestOption` / `EditPullRequestOption` / `PullRequest`
definitions.

- Find probe: `GET /repos/{owner}/{repo}/pulls?state=open&head=<branch>`
  — the documented query parameters include `state` and `head`; the
  result is the open-PR list, and the adapter confirms the match on the
  parsed `PullRequest.head.ref` field (`PRBranchInfo.ref`) client-side
  (the same belt-and-braces as the other lanes).
- Create: `POST /repos/{owner}/{repo}/pulls` with
  `CreatePullRequestOption` — `head` ("string"), `base` ("string"),
  `title` ("string"), `body` ("string"). The target is the §11.2
  inverse mapping (`halyard/release/<base>` strips to `<base>`), the
  ADR-0013 decision-6 mapping both other lanes carry.
- Rewrite: `PATCH /repos/{owner}/{repo}/pulls/{index}` with
  `EditPullRequestOption` — `title` ("string"), `body` ("string").
- The `PullRequest` response carries `number` ("integer"), `title`,
  `body`, `state`, `html_url`, `head` (`PRBranchInfo`) — the field
  names the wire parser reads.

## 3. Releases (the tag-phase surface, §11.1 step 2)

Source: the spec's release operations and the `CreateReleaseOption` /
`Release` definitions.

- Probe (GET-by-tag-first): `GET /repos/{owner}/{repo}/releases/tags/{tag}`
  — the spec documents the path for both `GET` and `DELETE`; a missing
  release is the REST's `404` (the not-found-is-a-value convention).
- Create: `POST /repos/{owner}/{repo}/releases` with
  `CreateReleaseOption` — `tag_name` is the definition's only
  **required** field ("required: [tag_name]"), `body` ("string") is
  the release notes, `name` optional. The §11.1 tag phase creates the
  git tags first (git side), so the release rides the existing tag.
- The `Release` response carries `tag_name`, `html_url`, `id` — the
  adapter maps `html_url` onto the seam's `Release::url` (empty when
  absent, never composed).

## 4. Comments (the §11.6 check-comment surface)

Source: the spec's issue-comment operations and the `Comment` /
`CreateIssueCommentOption` / `EditIssueCommentOption` definitions.
A pull request **is an issue** on this API — the comment surface is
addressed by the PR's issue index.

- Scan: `GET /repos/{owner}/{repo}/issues/{index}/comments` — the
  documented query parameters are `since`/`before` (timestamps) only;
  the endpoint documents **no pagination parameters** (unlike GitHub's
  `per_page`/GitLab's `page`), so the marker scan walks the returned
  array once and finds the stable `<!-- halyard:check v1 -->` marker.
  A deployment-side cap on the array would make the scan incomplete —
  recorded honestly here; the hosted instances' comment volumes are
  far below any such cap, and the check comment is one marker per PR.
- Create: `POST /repos/{owner}/{repo}/issues/{index}/comments` with
  `CreateIssueCommentOption` — `body` is the definition's only
  required field.
- Rewrite: `PATCH /repos/{owner}/{repo}/issues/comments/{id}` with
  `EditIssueCommentOption` — `body` required. The scan's `Comment.id`
  is the address; `Comment.html_url` maps onto the seam's
  `PrComment::url` (empty when absent).

## 5. The `commit_via_api` decline (§11.5 `commit_mode = "api"`)

Source: the spec's repository-contents operations.

- The Forgejo/Gitea surface has **no atomic multi-file commit**: the
  contents API (`POST`/`PUT /repos/{owner}/{repo}/contents/{filepath}`)
  writes one file per request, each producing its own commit. A
  release commit carries the plan's whole `EditSet` (typically several
  manifests through the cascade) as ONE `chore: release` commit — a
  per-file commit train is semantically wrong for that, so the adapter
  keeps the trait's documented honest decline
  ([`Forge::commit_via_api`](crate::Forge)'s default,
  `ForgeError::Unsupported` naming the adapter, §7.3): the §11.5
  `commit_mode = "api"` shape is the shell's GitLab/GitHub lanes, and
  the Forgejo lane rides the git half (`commit_mode = "git"` — the
  §11.2 CAS push the M4.2 remainder shipped). The conformance matrix
  reports the decline as the `Declined` verdict — the exact case the
  M7 suite's verdict model was built for.

## 6. Rate limits and headers

Source: the spec (no quota headers defined) and the forgejo.org API
usage documentation.

- No `x-ratelimit-*`-style response headers are defined on this
  surface: the adapter's `RateInfo` snapshot stays an honest zero
  until a deployment sends quota headers (the same posture the GitLab
  lane records; a fabricated quota is worse than an observed
  absence).
- `429` maps to `E-FORGE-RATE-LIMIT` with `retry_after_secs` from the
  standard HTTP `retry-after` response header (RFC 9110 §10.2.3), else
  the 60 s floor the other lanes use.

## 7. Not used here (recorded absences)

- App-JWT auth: GitHub-specific (ADR-0014); the Forgejo lane is
  token-only (`Authorization: token`).
- ETag conditionals: not defined on this surface; the adapter sends
  unconditional probes.
- The `since`/`before` comment filters: not needed — the scan walks
  the full array the endpoint returns.
