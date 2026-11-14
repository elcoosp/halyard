# GitHub REST forge adapter — verified request/response reference (M4.1b)

> **Status:** verified 2026-11-14 against the live docs.github.com REST
> reference (API version `2022-11-28`). Every claim below was read from a
> cited page; nothing is carried over from the §11 sketch un-verified.
> This note is the `VERIFY` artifact §0 rule 4 requires before the
> adapter lands; ADR-0013 records the resulting decisions.

## 1. Base URL, auth and mandatory headers

Source: <https://docs.github.com/en/rest/using-the-rest-api/getting-started-with-the-rest-api?apiVersion=2022-11-28>

- Base URL: `https://api.github.com` (paths below are relative to it).
- Auth: `Authorization: Bearer <token>` ("in most cases you can use
  `Authorization: Bearer` or `Authorization: token`… if you are passing
  a JSON web token (JWT), you must use `Authorization: Bearer`") — the
  adapter always sends `Bearer`, which is also the only form that works
  for the App-JWT follow-up.
- `Accept: application/vnd.github+json` on every request.
- `X-GitHub-Api-Version: 2022-11-28` on every request.
- `User-Agent` is mandatory ("All API requests must include a valid
  `User-Agent` header… an invalid one returns a `403 Forbidden`"). The
  adapter sends `halyard/<version>`.

## 2. Rate limits (primary + secondary)

Source: <https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api?apiVersion=2022-11-28> and <https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api?apiVersion=2022-11-28>

- Response headers: `x-ratelimit-limit`, `x-ratelimit-remaining`,
  `x-ratelimit-used`, `x-ratelimit-reset` (UTC epoch seconds),
  `x-ratelimit-resource`. "The `x-ratelimit-*` headers are the
  authoritative source for your current rate limit status" — the
  adapter snapshots them per response; `GET /rate_limit` is NOT used
  (its values may disagree across regions).
- Exceeding the primary limit → `403` or `429` with
  `x-ratelimit-remaining: 0`. Retryable per the taxonomy
  (`E-FORGE-RATE-LIMIT`, exit 5): the adapter maps it with
  `retry_after_secs` from `retry-after` when present, else from
  `x-ratelimit-reset - now` — both verified: "If the `retry-after`
  response header is present, you should not retry your request until
  after that many seconds has elapsed."
- Secondary limits: same status codes, "wait for at least one minute"
  when neither header is present → the adapter falls back to 60 s.
- No in-adapter retry loop: §15.4 makes rate-limit failures exit-5
  retryable at the CI level and the governor is M5.5 ("Rate-limit
  governor, call budget, summary"). Hidden retries would also break the
  §11.5 call-log accounting (a hidden retry is an unaccounted call).

## 3. Conditional requests (ETag)

Source: <https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api?apiVersion=2022-11-28>

- "You can use the values of these headers to make conditional `GET`
  requests": save the `etag` response header, send it back as
  `if-none-match`; unchanged → `304 Not Modified`.
- "Making a conditional request does not count against your primary
  rate limit if a `304` response is returned" (with a valid
  `Authorization` header) — this is what makes the §11.1 no-op prepare
  path nearly free on the REST adapter.
- Conditionals are GET-only ("conditional requests for unsafe methods
  like `POST`, `PUT`, `PATCH`, and `DELETE` are not supported"). The
  adapter caches ETags only for `find_release_pr`'s list probe (the
  repeated call in the update path).

## 4. Endpoints used by the `Forge` trait

Sources: pulls <https://docs.github.com/en/rest/pulls/pulls?apiVersion=2022-11-28>,
releases <https://docs.github.com/en/rest/releases/releases?apiVersion=2022-11-28>,
git commits <https://docs.github.com/en/rest/git/commits?apiVersion=2022-11-28>,
git refs <https://docs.github.com/en/rest/git/refs?apiVersion=2022-11-28>,
git blobs <https://docs.github.com/en/rest/git/blobs?apiVersion=2022-11-28>,
git trees <https://docs.github.com/en/rest/git/trees?apiVersion=2022-11-28>.

| `Forge` method | REST exchange | Notes |
|---|---|---|
| `find_release_pr` | `GET /repos/{owner}/{repo}/pulls?state=open&head={owner}:{branch}` | `head` filter is "user:ref-name" (list query); first matching `head.ref == branch` row maps to `Pr` (`number`, `html_url`, `title`, `body`). ETag-conditional; repo-level `404` means the repo is invisible to the credentials → `E-FORGE-AUTH` (not a not-found value). |
| `upsert_pr` | find as above; none → `POST /repos/{owner}/{repo}/pulls` `{head, base, title, body}`; existing → `PATCH /repos/{owner}/{repo}/pulls/{number}` `{title, body}` | `base` is not in `PrSpec`; the adapter derives it by stripping the §11.2 `halyard/release/` prefix (the documented inverse mapping — zero extra calls, budget-safe). Same-repo `head` is the plain branch name. |
| `release_exists` | `GET /repos/{owner}/{repo}/releases/tags/{tag}` | `200` → true, `404` → false (documented "Resource not found" — a value, §11.4). |
| `create_release` | GET-by-tag first; `404` → `POST /repos/{owner}/{repo}/releases` `{tag_name, name, body}` | `201` → created; a `422` whose error code is `already_exists` is the GET/POST race (I1) → `E-FORGE-CONFLICT` (re-run finds it); other `422` → `E-FORGE-REQUEST`. |
| `close_pr` | conditional find; found → `PATCH /repos/{owner}/{repo}/pulls/{number}` `{state: "closed"}` | None → `false` (value, never an error). |
| `commit_via_api` | `GET /repos/{owner}/{repo}/git/ref/heads/{branch}` → `GET /repos/{owner}/{repo}/git/commits/{sha}` (tree) → `POST …/git/trees` `{base_tree, tree:[{path, mode:"100644", type:"blob", content?…}]}` → `POST …/git/commits` `{message, tree, parents:[base]}` → `PATCH …/git/refs/heads/{branch}` `{sha, force:false}` | UTF-8 file content rides the tree entry's `content` ("Cannot be combined with `sha`"); non-UTF-8 payloads go through `POST …/git/blobs` `{content: base64, encoding: "base64"}` and reference the returned `sha`. Ref update `409`/non-fast-forward `422` → `E-FORGE-CONFLICT`; `force:false` is the documented default ("will make sure you're not overwriting work") — never forced, §11.2. |

## 5. Failure-surface mapping (the `E-FORGE-*` codes)

- `401` → `E-FORGE-AUTH`; `403` with `x-ratelimit-remaining: 0` or
  `retry-after` → `E-FORGE-RATE-LIMIT`; other `403` (scope rejection) →
  `E-FORGE-AUTH` (reason text quotes the server message, never the
  token).
- `404` on repo-scoped collection endpoints → `E-FORGE-AUTH` (GitHub
  deliberately does not distinguish "missing" from "forbidden"); `404`
  on the release-by-tag and ref-read probes is the documented
  not-found *value* for those probes.
- `409` → `E-FORGE-CONFLICT`; `422` → `E-FORGE-CONFLICT` when the body's
  error codes say `already_exists` (the I1 race), else
  `E-FORGE-REQUEST`; `5xx` → `E-FORGE-TRANSPORT` (server-side; retryable
  at the run level, not a caller bug).
- Connection-level failures (DNS/TLS/refused/timeout) → the new
  `E-FORGE-TRANSPORT` code (ADR-0013): the seam's taxonomy doc already
  reserves "transport" for exactly this class; M4.1a had no producer.

## 6. Decisions recorded (details in ADR-0013)

1. HTTP client: blocking `ureq` + rustls (§19 default decision 4 —
   "smallest, no async runtime"); no tokio anywhere (§14.2).
2. The adapter lives in a new crate `halyard-forge-github`
   (`halyard-forge` keeps its §7.2 core-only layer rule, which
   `xtask/src/layers.rs` pins; the seam crate itself gains only the
   transport *vocabulary* — plain data, no HTTP).
3. Token auth only in this slice (`Bearer <token>`); App-JWT minting is
   the declared M4.1c follow-up (needs RSA/RS256 signing surface; its
   header shape is already verified above).
4. Base-branch derivation for `upsert_pr` = strip the §11.2 prefix; no
   extra repo probe.
5. Mock-forge E2E lane: a scripted in-memory transport (the seam crate's
   `MemoryTransport`, the transport analogue of `MemoryForge`) for
   contract tests, plus a loopback `std::net` HTTP server test for the
   real `UreqTransport` — no wiremock/tokio dependency.
