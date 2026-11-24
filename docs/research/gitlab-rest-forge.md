# GitLab REST forge adapter — verified request/response reference (M7 step 1)

> **Status:** verified 2026-11-24 against the live docs.gitlab.com REST
> reference. Every claim below was read from a cited page; nothing is
> carried over from the §11 sketch un-verified. This note is the
> `VERIFY` artifact §0 rule 4 requires before the adapter lands; the
> M7 milestone row (`docs/HALYARD_ARCHITECTURE.md` §17: "GitLab (MR
> flow, CI component) … forge conformance test-suite shared across
> adapters") is the plan source.

## 1. Base URL, auth and headers

Source: <https://docs.gitlab.com/api/merge_requests/> (curl examples)
and <https://docs.gitlab.com/api/releases/> (auth note).

- Base URL: `https://gitlab.com/api/v4` (the doc examples use
  `https://gitlab.example.com/api/v4/...`; GitLab.com serves the same
  `/api/v4` root). Self-hosted instances override the base; the
  adapter takes it explicitly (`GITLAB_API_URL` override, else the
  default).
- Auth: the `PRIVATE-TOKEN` header — every curl example reads
  `--header "PRIVATE-TOKEN: <your_access_token>"`; the releases page
  repeats it ("`PRIVATE-TOKEN` header with a personal access token, or
  `JOB-TOKEN` header with `$CI_JOB_TOKEN`"). The adapter v1 sends
  `PRIVATE-TOKEN` (a PAT; the CI-job-token form is a later slice).
  Invalid tokens return `401 Unauthorized` ("401 Unauthorized if the
  access token is invalid").
- `User-Agent`: the GitHub adapter's `halyard/<version>` convention
  carries over (GitLab does not document a mandatory UA; it is good
  HTTP citizenship and costs nothing).

## 2. Project identifier

Source: every endpoint's `id` attribute row (MR list/create/update,
releases, commits, notes).

- `id` — "The ID or URL-encoded path of the project." The adapter
  URL-encodes the full project path (`group/repo`, nested groups
  included — the path segments keep `/`, so encoding applies
  per-segment with `%2F` for the separators) and sends it as the
  `:id` path segment.
- MRs are addressed by `merge_request_iid` (the project-scoped
  counter), not by a global id — the seam's `Pr::number` carries the
  iid, and the marker comment scan walks notes under
  `.../merge_requests/:merge_request_iid/notes`.

## 3. Merge requests (the Release-PR surface, §11.1 step 3)

Source: <https://docs.gitlab.com/api/merge_requests/>.

- List (the find probe):
  `GET /projects/:id/merge_requests?source_branch=<branch>&state=opened`
  — `source_branch`: "Returns merge requests with the given source
  branch"; `state`: "Returns all merge requests (all) or just those
  that are opened, closed, locked, or merged. Defaults to all"; the
  documented combined example is
  `GET /projects/:id/merge_requests?source_branch=feature-x&state=opened`.
  Pagination: "Use the pagination parameters page and per_page to
  restrict the list of merge requests"; offset pagination is "The
  default method and available on all endpoints" (API overview).
- Create: `POST /projects/:id/merge_requests` — required
  `id`, `source_branch` ("The source branch."), `target_branch` ("The
  target branch."), `title` ("Title of MR."); optional `description`.
  The target is the §11.2 inverse mapping (`halyard/release/<base>`
  strips to `<base>`), mirroring ADR-0013 decision 6 — a non-release
  branch fails closed before any call.
- Update: `PUT /projects/:id/merge_requests/:merge_request_iid` —
  `title` ("Title of MR."), `description`, `state_event` ("New state
  (close/reopen)"). "Must include at least one non-required
  attribute" (the update body is never empty by construction here).
  Documented abuse guard: "After you reach the limit, you receive
  429 Too Many Requests" — the adapter maps `429` to
  `E-FORGE-RATE-LIMIT`.
- The MR JSON carries `iid`, `title`, `description`, `state`
  (`"opened"`), `source_branch`, `target_branch`, `web_url` — the
  field names the wire parser reads.
- **No conditional-request lane:** GitLab does not document `ETag` /
  `If-None-Match` conditionals for the MR list (unlike GitHub's
  conditional GETs). The adapter sends unconditional probes; the
  §11.5 budget on this adapter is find + one write, with no
  `304`-free path to fake. Recorded as a conscious difference from
  the GitHub adapter (ADR-0013's ETag cache has no GitLab twin).

## 4. Notes (the §11.6 check-comment surface)

Source: <https://docs.gitlab.com/api/notes/>.

- List: `GET /projects/:id/merge_requests/:merge_request_iid/notes` —
  `sort`: "Return merge request notes sorted in asc or desc order.
  Default is desc"; `order_by`: created_at/updated_at, default
  created_at. The marker scan sends `sort=asc&order_by=created_at`
  plus the global offset pagination (`per_page=100&page=N`) so the
  ascending walk is complete — GitHub's comment list is ascending by
  default; GitLab's is NOT (the adapter must ask).
- Create: `POST /projects/:id/merge_requests/:merge_request_iid/notes`
  — `body` (required, "Limited to 1,000,000 characters").
- Update: `PUT /projects/:id/merge_requests/:merge_request_iid/notes/:note_id`
  — `body` (required). The marker scan's `note.id` is the address.
- The documented note JSON: `id`, `body`, `author`, `created_at`,
  `system`, `noteable_type`, ... The docs' response example does not
  include a `web_url`; the adapter parses it when present and leaves
  the seam's `PrComment::url` empty otherwise (a URL is never
  fabricated).

## 5. Releases (the tag-phase surface, §11.1 step 2)

Source: <https://docs.gitlab.com/api/releases/>.

- Probe (GET-by-tag-first): `GET /projects/:id/releases/:tag_name`
  (`tag_name`: "The Git tag the release is associated with."). A
  missing release is the REST's `404 Not Found` (the documented
  error semantics: "404 Not Found — The requested resource doesn't
  exist") — the not-found-is-a-value convention, mirroring the
  GitHub adapter's release probe.
- Create: `POST /projects/:id/releases` (Developer-level access
  required) with `tag_name` ("The tag where the release is created
  from." — required), `description` (optional, Markdown), `name`
  optional; `ref` is "yes, if tag_name doesn't exist" — the §11.1
  tag phase creates the git tags first (git side), so `ref` is
  omitted and the release rides the existing tag.
- The release JSON has no `html_url`/`web_url` — `_links.self` is
  "HTTP URL of the release." The adapter maps `_links.self` onto the
  seam's `Release::url` (empty when absent, never composed).

## 6. Commits API (the §11.5 `commit_mode = "api"` surface)

Source: <https://docs.gitlab.com/api/commits/>.

- Create: `POST /projects/:id/repository/commits` — `branch`
  (required; "To create a new branch, also provide either
  start_branch or start_sha"), `commit_message` (required),
  `actions[]` (optional array).
- Action object: `action` — "The action to perform: create, delete,
  move, update, or chmod"; `file_path` (required); `content`
  ("required for all except delete, chmod, and move"); `encoding` —
  "text or base64. text is default." The adapter sends every payload
  as `encoding: "base64"` (byte-exact round-trip, the same engine
  choice as the embedded-plan encoding), with the action kind probed
  per file: `GET /projects/:id/repository/files/<path>?ref=<branch>`
  returns `200` when the file exists on the branch (→ `update`) and
  `404` when it does not (→ `create`); GitLab rejects an `update` on
  a missing file and a `create` on an existing one (the action kinds
  are exclusive — the probe is the documented-safe disambiguation).
- Response `201` with `id` ("SHA of the created commit") — the seam's
  commit `Oid`. The response does not echo the branch.
- The GitLab API commits the actions against the branch tip in one
  call (no tree/blob choreography like GitHub's Git Data API);
  `start_sha` is not sent — the branch tip is the intended base and
  a racing writer surfaces as a server-side rejection (mapped
  fail-closed; never retried silently).

## 7. Rate limits and headers

Source: <https://docs.gitlab.com/api/merge_requests/> (the update
limit) and <https://docs.gitlab.com/api/rest/> (pagination). The
rate-limits pages fetched did not document universal
`x-ratelimit-*`-style response headers; GitLab's throttling is
documented through status codes ("After you reach the limit, you
receive 429 Too Many Requests").

- The adapter maps `429` → `E-FORGE-RATE-LIMIT` with
  `retry_after_secs` from the standard HTTP `retry-after` response
  header (RFC 9110 §10.2.3 — a universal header, not a GitLab
  extension), else the 60 s floor the GitHub adapter uses.
- `RateInfo` rides `x-ratelimit-limit` / `x-ratelimit-remaining`
  when a deployment sends them (some self-hosted installations do);
  when absent the snapshot stays at `0/0` = "nothing observed" — the
  §11.5 summary renders an honest zero rather than a fabricated
  quota. Recorded as a conscious difference from the GitHub adapter
  (whose quota headers are always present).

## 8. Not used here (recorded absences)

- App-JWT auth: GitHub-specific (ADR-0014); GitLab has no
  installation-token equivalent on this surface. The adapter is
  token-only; `ForgeCredentials` stays a plain bearer in the CLI's
  GitLab lane.
- ETag conditionals: §3 — not documented, not faked.
- `commit_via_api` via tree/blob endpoints: §6 — the commits API's
  actions[] replaces the choreography; the per-file exists probe is
  the price of the exclusive action kinds.
