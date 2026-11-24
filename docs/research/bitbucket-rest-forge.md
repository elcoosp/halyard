# Bitbucket REST forge adapter — verified request/response reference (M7 step 3)

> **Status:** verified 2026-11-24 against Atlassian's documented Bitbucket
> Cloud REST API 2.0 reference (developer.atlassian.com) and the Bitbucket
> Cloud request-limits page (support.atlassian.com). Every claim below was
> read from a cited source; nothing is carried over from the §11 sketch
> un-verified. This note is the `VERIFY` artifact §0 rule 4 requires before
> the adapter lands; the M7 row (`docs/HALYARD_ARCHITECTURE.md` §17:
> "GitLab (MR flow, CI component), Gitea/Forgejo, Bitbucket; forge
> conformance test-suite shared across adapters") is the plan source.
> ADR-0051 records the resulting decisions.

## 1. One adapter, one surface — base URL, auth, headers

Source: https://developer.atlassian.com/cloud/bitbucket/rest/intro/#authentication

- "All Bitbucket Cloud requests start with the https://api.bitbucket.org/2.0
  prefix (for the 2.0 API)" — the endpoint base is
  `https://api.bitbucket.org/2.0`, the repository scope is
  `/repositories/{workspace}/{repo_slug}` under it.
- "To authenticate with an API token, use Basic HTTP Authentication as per
  RFC-2617", "the username is your Atlassian email and password is the API
  token" — an API token rides Basic **with a second credential** (the
  Atlassian account email), which does not fit a single-token env grammar;
  recorded as the adapter's documented scope (§7).
- "App passwords are deprecated." — not a supported auth path.
- OAuth 2.0: "all OAuth 2.0 authenticated API requests must be directed at
  https://api.bitbucket.org" and "with the access token placed as a Bearer
  token in the request Authorization header as per RFC-6750 (2.1)"; the
  documented curl: `curl -H "Authorization: Bearer {access_token}"
  https://api.bitbucket.org/2.0/user`. **The adapter rides this scheme:**
  the lane's single token is an OAuth 2.0 access token (a CI deployment
  mints one via the documented client-credentials grant), sent as
  `Authorization: Bearer <token>` on every call.
- "Our access tokens expire in one hour. When this happens you'll get 401
  responses." — 401 maps to `E-FORGE-AUTH`; the re-run after re-minting
  converges (I2).
- Repository/workspace access tokens ("Access tokens are passwords (or
  tokens) that provide access to a single repository, project or
  workspace") are documented under the Basic scheme for the REST API and
  `x-token-auth` for git — also outside the single-token Bearer grammar
  (§7).

## 2. Pull requests

Source: https://developer.atlassian.com/cloud/bitbucket/rest/api-group-pullrequests/
(filter grammar: https://developer.atlassian.com/cloud/bitbucket/rest/intro/#filtering-and-sorting)

- List: `GET /repositories/{workspace}/{repo_slug}/pullrequests` — "By
  default only open pull requests are returned"; filters ride the `q`
  parameter.
- The filter grammar: the operator table defines `=` as "test for
  equality"; "the String data type is any text inside double quotes";
  nested fields "can be traversed recursively" with dotted paths — the
  documented pull-request examples include `destination.branch.name =
  "master"` and `source.repository.full_name != "main/repo"`, so
  `source.branch.name = "<branch>"` is the documented grammar for the
  find-by-head probe. "the entire query string is put in the q parameter
  and hence needs to be URL-encoded". The adapter sends
  `q=state = "OPEN" AND source.branch.name = "<branch>"` (URL-encoded) and
  **confirms the hit client-side** on the parsed `source.branch.name` —
  the wire filter is the probe, the parsed field is the truth (the same
  discipline the Forgejo `head=` filter ships).
- Create: `POST /repositories/{workspace}/{repo_slug}/pullrequests` —
  minimum body "title and source, specified by a branch name":
  `{"title": …, "source": {"branch": {"name": …}}}`; "destination" is
  optional and "without it it defaults to `repository.mainbranch`" — the
  adapter always sends `destination` explicitly (the §11.2 inverse
  mapping: the prefix-stripped base), plus `title` and `description`
  (the PR body). Success returns `201`.
- Update: `PUT /repositories/{workspace}/{repo_slug}/pullrequests/{id}` —
  "can be used to change the pull request's branches or description";
  "Only open pull requests can be mutated." The adapter sends
  `{"title", "description"}` only — a rewrite never touches `source` or
  state.
- The wire PR object carries `id`, `title`, `state`, `source`,
  `destination`, `created_on`, … (field names verbatim from the endpoint's
  response model); the parsed find confirmation reads
  `source.branch.name`.

## 3. The release surface: no releases object — the tag is the marker

Source: the REST reference's API-group index
(https://developer.atlassian.com/cloud/bitbucket/rest/), whose endpoint
groups are, verbatim: "Addon", "Branch restrictions", "Branching model",
"Commit statuses", "Commits", "Deployments", "Downloads", "GPG", "Merge
queues", "Pipelines", "Projects", "Pullrequests", "Refs", "Reports",
"Repositories", "SSH", "Snippets", "Source", "Users", "Webhooks",
"Workspaces", "Other operations".

- **No "Releases" group exists** — Bitbucket Cloud has no release object
  the REST API can create, and halyard invents none (the §7.3 rule).
  The release marker on this surface is the git tag.
- Source: https://developer.atlassian.com/cloud/bitbucket/rest/api-group-refs/
  — tags: `GET /repositories/{workspace}/{repo_slug}/refs/tags/{name}`
  ("Get a tag"), documented responses `200OK`, `403Forbidden`,
  `404Not Found`; the tag object carries `type` (`"tag"`), `name`,
  `message`, `target` (a commit object with `hash`), and `links` with
  `self`/`commits`/`html`.
- The adapter mapping (ADR-0051): `release_exists` probes the tag
  (`404` ⇒ `false` — the documented not-found value);
  `create_release` probes the same tag — present ⇒ `Release { created:
  false, url: links.html }` (the marker is verifiably in place; the
  git-side tag phase pushed it, so the idempotent re-run is an honest
  no-op, I1); absent ⇒ `ForgeError::Unsupported` naming the adapter
  (there is no release object to create and no tag to report — the
  machine's tag phase runs after the git-side push, so on the `release`
  flow this answers created:false; the decline fires only where the tag
  has not landed yet, which is exactly the case this surface cannot
  serve). `MachineEffect::ReleaseExisted` is the report row — the
  conformance matrix's `MachinePropagate` scenario accepts it (it counts
  `ReleaseCreated | ReleaseExisted`), so the declined set stays exactly
  `commit_via_api`.

## 4. Comments

Source: https://developer.atlassian.com/cloud/bitbucket/rest/api-group-pullrequests/

- List: `GET …/pullrequests/{pull_request_id}/comments` — "The default
  sorting is oldest to newest and can be overridden with the sort query
  parameter." Oldest-first is what a marker scan needs (the first marker
  hit is the oldest comment carrying it — the GitLab `sort=asc` lesson,
  documented here as the default); the scan walks the `values` array of
  the paginated envelope (`size`, `page`, `pagelen`, `next`, `previous`,
  `values`), following `page`/`pagelen` until a short page.
- Create: `POST …/pullrequests/{pull_request_id}/comments` — body fields
  include `content` "(with `raw`, `markup`, `html`)"; the adapter sends
  `{"content": {"raw": body}}`; returns `201`.
- Update: `PUT …/pullrequests/{pull_request_id}/comments/{comment_id}` —
  the rewrite-in-place address (same `content.raw` shape).

## 5. The `commit_via_api` decline

Source: https://developer.atlassian.com/cloud/bitbucket/rest/api-group-commits/

- The Commits group's operations are reads plus approvals and comments;
  the two POST forms on `/commits` exist only for include/exclude
  parameters: "Note that this resource does NOT support new commit
  creation."
- Therefore `commit_via_api` is the documented honest decline
  (`E-FORGE-UNSUPPORTED` naming `BitbucketRestForge`): a release commit
  carries the plan's whole `EditSet` as ONE `chore: release` commit, and
  this surface offers no atomic multi-file write to carry it. The
  Bitbucket lane rides `commit_mode = "git"` (the §11.2 CAS push), like
  the Forgejo lane (ADR-0050) — and like that lane, no `base64` layer
  edge ships (no payload encoder is needed).

## 6. Rate limits and headers

Source: https://support.atlassian.com/bitbucket-cloud/docs/api-request-limits/

- "Rate limits are divided into a one-hour rolling window for both
  authenticated and unauthenticated requests"; "Anonymous requests are
  subject to a strict rate limit of 60 requests per hour across all API
  resources"; the authenticated total is 1,000 requests per hour (scaled
  plans add "10 API requests per hour per paid user" beyond 100 seats).
- Documented response headers: "X-RateLimit-Limit: Represents the total
  number of requests permitted per hour", "X-RateLimit-Resource:
  Identifies the API endpoint resource group receiving scaled rate
  limits", "X-RateLimit-NearLimit: A boolean value indicating whether
  less than 20% of the requests remain" — **no remaining-requests header
  and no `Retry-After` are documented** on this page, and the status
  code returned on exhaustion is not stated there either (deployments
  report `429` in practice).
- The adapter mapping: the standard throttle status `429` maps to
  `E-FORGE-RATE-LIMIT` honoring `retry-after` when a deployment sends
  it, else the 60 s floor every lane shares; the quota snapshot
  records `x-ratelimit-limit` when the response carries it and leaves
  `remaining` at the honest zero (no documented header — a fabricated
  remaining count is worse than an observed absence, the ADR-0049
  rule).

## 7. Not used here (recorded absences)

- **ETag conditionals** — not documented on this surface (the GitLab and
  Forgejo lanes ship the same conscious absence, ADR-0049/0050): the
  find probe re-reads unconditionally.
- **Basic auth (API tokens / repository access tokens)** — the
  documented scheme needs a second credential (the Atlassian account
  email, or the `x-token-auth` username), which the single-token env
  grammar cannot carry; the lane rides the OAuth 2.0 Bearer scheme
  (§1) and the doctor names it.
- **Draft pull requests, reviewers, close_source_branch** — the create
  endpoint documents them as optional; the Release PR sends none of
  them (a release PR is not a draft, and the source branch's fate is
  the merge's business).
- **The decline endpoint on a not-found pull** — the close path probes
  first (the find filter pins `state = "OPEN"`), so an
  already-declined/merged pull is the not-found value (`Ok(false)`)
  before any decline POST; the find→decline race window surfaces as the
  forge's own error and the re-run converges (I1: the re-run re-probes).

## 8. The pull close shape, verified for the implementation

Verified 2026-11-24 against the Pullrequests group's operation list:
the close verb on this surface is **decline** — `POST
/repositories/{workspace}/{repo_slug}/pullrequests/{pull_request_id}/decline`
("Declines the pull request.") — not a state field on the update PUT
(the GitLab `state_event` shape does not exist here; the update PUT
"can be used to change the pull request's branches or description" and
mutating an open PR is its documented scope). A `200` returns the PR
object. The adapter sends no body with the decline POST, maps every
non-2xx onto the taxonomy, and reports `Ok(true)` — the end state the
machine's close contract needs (§11.1 step 3: "close stale Release PR
if one exists").
