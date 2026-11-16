# OIDC trusted publishing — the crates.io exchange (M5.3 step 3)

Verified per §0.4 (fetched quotes below with their sources). This note is
the grammar contract for [`halyard-publish/src/oidc.rs`](../../crates/halyard-publish/src/oidc.rs)
(ADR-0031): the GitHub Actions OIDC runtime context, the id-token request,
and the crates.io trusted-publishing token exchange.

## 1. What trusted publishing is (crates.io side)

Source: <https://blog.rust-lang.org/2025/07/11/crates-io-development-update-2025-07>
(crates.io development update, 2025-07-11) — fetched 2026-11-16.

> "Trusted Publishing eliminates the need for GitHub Actions secrets when
> publishing crates"

> "The trusted repo can 'request a short-lived API token for publishing in
> a secure way using OpenID Connect (OIDC)'"

Scope limits, quoted from the same post:

- "limited to GitHub Actions", built so it "allows other CI/CD providers
  like GitLab CI to be supported in the future" (GitLab not supported yet).
- Onboarding: "you'll need to publish your first release manually" before
  a trusted publisher can be configured on crates.io.
- The post's documented workflow shape exchanges the OIDC token for a
  **temporary access token that rides the documented env grammar**:

```yaml
permissions: id-token: write # Required for OIDC token exchange
- uses: rust-lang/crates-io-auth-action@v1
  # ...
- CARGO_REGISTRY_TOKEN: ${{ steps.auth.outputs.token }}
```

The full documented page lives at
<https://crates.io/docs/trusted-publishing> (referenced by the post; the
page itself is client-rendered — the quotes above are the post's own
text). The reference implementation of the exchange is the
`rust-lang/crates-io-auth-action` action; its source is the primary
grammar contract below.

## 2. The GitHub Actions OIDC runtime context

Source: <https://github.com/actions/toolkit>, `packages/core/src/oidc-utils.ts`
(`OidcClient`), fetched 2026-11-16 — verbatim:

```ts
private static getRequestToken(): string {
    const token = process.env['ACTIONS_ID_TOKEN_REQUEST_TOKEN']
    if (!token) {
      throw new Error(
        'Unable to get ACTIONS_ID_TOKEN_REQUEST_TOKEN env variable'
      )
    }
    return token
}

private static getIDTokenUrl(): string {
    const runtimeUrl = process.env['ACTIONS_ID_TOKEN_REQUEST_URL']
    if (!runtimeUrl) {
      throw new Error('Unable to get ACTIONS_ID_TOKEN_REQUEST_URL env variable')
    }
    return runtimeUrl
}
```

The id-token request (verbatim, same file):

```ts
let id_token_url: string = OidcClient.getIDTokenUrl()
if (audience) {
  const encodedAudience = encodeURIComponent(audience)
  id_token_url = `${id_token_url}&audience=${encodedAudience}`
}
```

- The request is a **GET** of `{ACTIONS_ID_TOKEN_REQUEST_URL}&audience={encoded audience}`
  (the runtime URL carries its own query string; the toolkit literally
  appends `&audience=`) with `Authorization: Bearer {ACTIONS_ID_TOKEN_REQUEST_TOKEN}`
  (`new BearerCredentialHandler(OidcClient.getRequestToken())`) and the
  client's own User-Agent (`'actions/oidc-client'`).
- The response body is JSON with the token in the **`value`** field
  (verbatim: `const id_token = res.result?.value` / `throw new Error(
  'Response json body do not have ID Token field')` when absent).

Both variables exist only when the workflow declares
`permissions: id-token: write` (the crates.io post: "Required for OIDC
token exchange").

## 3. The crates.io token exchange

Source: <https://github.com/rust-lang/crates-io-auth-action>, `src/main.ts`,
`src/registry_url.ts`, `src/utils.ts` (fetched 2026-11-16) — verbatim:

**Audience derivation** (`registry_url.ts`):

```ts
// Extract audience from registry URL by removing `https://` or `http://`.
export function getAudienceFromUrl(url: string): string {
    const audience = url.replace(/^https?:\/\//, "");
```

(default registry URL: `const url = core.getInput("url") || "https://crates.io";`,
trailing `/` stripped — so the crates.io audience is the host `crates.io`.)

**Endpoint** (`utils.ts`):

```ts
export function getTokensEndpoint(registryUrl: string): string {
    return `${registryUrl}/api/v1/trusted_publishing/tokens`;
}
```

**The exchange request** (`main.ts`, verbatim):

```ts
const response = await fetch(tokenUrl, {
    method: "POST",
    headers: {
        "Content-Type": "application/json",
        ...userAgent,
    },
    body: JSON.stringify({ jwt: jwtToken }),
});
```

**The exchange response** — HTTP 2xx with JSON whose **`token`** field is
the temporary access token (verbatim from the same file:
`const tokenResponse = (await response.json()) as { token: string };` and
the missing-field guard `if (!tokenResponse.token) { … }`).

**Error bodies** are crates.io's JSON error shape (`utils.ts`):

```ts
interface ErrorResponse {
    errors: Array<{ detail: string }>;
}
```

the action appends every `detail` to its message. halyard maps the same
shapes onto `E-PUB-AUTH` (registry + status + the parsed `detail`s, with
the id-token and the exchanged token scrubbed from every surfaced
message — M5.3 step 1's scrub contract applies to exchanged tokens too).

**The context-permission guard** (`main.ts`) — the action refuses before
any network when the runner has no OIDC runtime URL:

```ts
if (
    process.env.ACTIONS_ID_TOKEN_REQUEST_URL === undefined ||
    !process.env.ACTIONS_ID_TOKEN_REQUEST_URL
) {
    throw new Error(
        "Please ensure the 'id-token' permission is set to 'write' in your workflow. …"
    );
}
```

**Token lifetime**: "short-lived API token" (the post, quoted above); the
exact TTL is not documented in the fetched sources and halyard never
assumes one (the exchange result is used immediately, memoized for the
run, and scrubbed everywhere). The action's post step "automatically
revokes the token when the job completes" (action README) — halyard's
runs are single-command processes; there is no post step to replicate.

## 4. The grammar halyard implements (ADR-0031)

1. **Context detection** (pure, over an injected `Fn(&str) -> Option<String>`
   reader): `ACTIONS_ID_TOKEN_REQUEST_URL` + `ACTIONS_ID_TOKEN_REQUEST_TOKEN`
   both present and non-empty → a context; both absent → `None` (the
   normal shape outside GitHub Actions); a present-but-empty or
   half-present pair is a misconfiguration and fails closed `E-PUB-AUTH`
   naming the variable (the same rule the empty-token refusal uses).
2. **Audience**: registry URL minus the `https?://` scheme (`crates.io`
   for the default).
3. **Id-token**: GET `{request_url}&audience={percent-encoded audience}`
   (RFC 3986 unreserved encoding — the toolkit's `encodeURIComponent`
   shape), `Authorization: Bearer <request token>`; 2xx body JSON
   `{"value": "<id-token>"}`.
4. **Exchange**: POST `{registry_url}/api/v1/trusted_publishing/tokens`
   with `Content-Type: application/json`, body `{"jwt": "<id-token>"}`;
   2xx body JSON `{"token": "<temporary token>"}`.
5. **Injection**: the exchanged token rides the documented cargo grammar
   as `CARGO_REGISTRY_TOKEN` (the blog's own workflow spells the same
   variable) through the existing `CredentialEnv`/`PublishAuth` seam —
   argv and env agree, scrubbing covers it, and the provenance summary
   names the source `oidc:<audience>` (names only, never values).
6. **Precedence**: the documented env grammar wins (an explicit
   `CARGO_REGISTRY_TOKEN` is never overridden by the exchange); the
   exchange applies only to the default crates.io registry (trusted
   publishing is GitHub-Actions + crates.io today; custom registries keep
   the env grammar — an honest `none`/`env:` row).
7. **Dry mode**: the forecast projects `oidc:<audience>` without any
   network (the exchange is disabled for `PublishMode::Dry`; the dry
   spawn runs the tool's documented `--dry-run` form, which needs no
   token).

Sources fetched 2026-11-16 (mainland-mirror proxies may have been used;
quotes taken from the raw.githubusercontent.com contents of the named
files): the Rust blog post, the crates.io trusted-publishing page URL,
`rust-lang/crates-io-auth-action` (`main.ts`, `registry_url.ts`,
`utils.ts`, `action.yml`, README), `actions/toolkit`
(`packages/core/src/oidc-utils.ts`).
