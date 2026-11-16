# ADR-0031: OIDC trusted publishing over the `PublishAuth` seam

- **Status:** accepted
- **Date:** 2026-11-16
- **Milestone:** M5.3 step 3 — the declared remainder of the publish-auth
  milestone (step 1's scrubbing and step 2's provenance summary landed
  with ADR-0019; the §13 as-built note has said since M5.2 that "OIDC
  trusted publishing … stays M5.3-later and is absent rather than
  stubbed")
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

`halyard publish` resolves credentials exclusively through the
documented env grammar (ADR-0019): `CARGO_REGISTRY_TOKEN` et al.,
captured once, injected per spawn, scrubbed everywhere. In the one CI
where halyard is most likely to run a release — GitHub Actions with
`permissions: id-token: write` — the long-lived token is exactly what
crates.io's trusted publishing (RFC 3691, live since the 2025-07
crates.io update) exists to avoid: the runner mints a short-lived
registry token from an OIDC id-token, and no secret ever lives in the
repository. The grammar for that exchange is documented twice over
(VERIFIED in `docs/research/oidc-trusted-publishing.md`): the actions
toolkit's `OidcClient` (the `ACTIONS_ID_TOKEN_REQUEST_*` variables, the
`&audience=` GET, the `{"value": …}` body) and the reference
`rust-lang/crates-io-auth-action` (the audience-is-the-URL-sans-scheme
rule, the `POST {url}/api/v1/trusted_publishing/tokens` exchange with a
`{"jwt": …}` body, the `{"token": …}` answer, the
`{"errors":[{"detail":…}]}` failure shape).

Constraints that shaped the design:

1. **The dry forecast must be able to talk about auth without doing
   any.** The forecast (I5: the dry run never probes, never mutates)
   renders an auth-provenance summary; if the exchange lived anywhere
   on the summary's path, `publish --dry` would fire network calls. The
   provenance has to be *projectable* — derivable from the provider
   alone.
2. **The env grammar keeps precedence.** An explicit
   `CARGO_REGISTRY_TOKEN` is a maintainer's deliberate choice; the
   exchange is a fallback, not an override. A present-but-empty token
   keeps refusing the run (E-PUB-AUTH) exactly as before — the OIDC
   fallback must not paper over a misconfiguration.
3. **The transport must stay injectable.** The engine crates own no
   HTTP client; the exchange rides the same plain-data
   `HttpTransport` vocabulary the registry clients use, so the whole
   contract matrix runs offline over the scripted `MemoryTransport`
   and the runtime gets the blocking `UreqTransport`.
4. **Scope honesty.** Trusted publishing is GitHub-Actions + crates.io
   today (the crates.io post: "limited to GitHub Actions"; custom
   registries keep the env grammar). A provider that silently claimed
   OIDC for npm or for `[registries.<name>]` targets would be guessing.

## Decision

- **`halyard-publish/src/oidc.rs`** — the OIDC half of the auth
  plumbing:
  - `gha_oidc_context(&dyn Fn(&str) -> Option<String>)` detects the
    runtime context over an injected reader (never the process env in
    tests). Both variables present → `Some(OidcContext)`; both absent →
    `None` (the normal shape outside Actions); present-but-empty or
    half-present → `E-PUB-AUTH` naming the variable — the same
    fail-closed rule the empty-token refusal uses.
  - `OidcExchanger` performs the two documented calls (id-token GET
    with the bearer header and the percent-encoded audience; the JSON
    `{"jwt"}` exchange POST) over `Arc<dyn HttpTransport>`, mapping
    every failure — transport, non-2xx (with the parsed
    `errors[].detail` details), missing `value`/`token` fields,
    unparseable bodies — onto `E-PUB-AUTH`.
  - `OidcPublishAuth` composes the fallback: the base env grammar
    resolves first; a crates.io target with nothing documented and a
    detected context exchanges once (memoized per registry — one
    exchange serves the whole run) and injects the result as
    `CARGO_REGISTRY_TOKEN` through the existing `CredentialEnv`, so
    the spawn effect's injection and scrubbing work unchanged. The
    context request token and the exchanged token are scrub-listed by
    `PublishAuth::secrets`.
- **The `PublishAuth` trait gains `provenance`** (default: derive
  `env:<VARIABLE>` / `none` from `credentials`) and `auth_summary`
  rides it. The default keeps `EnvPublishAuth` untouched; the OIDC
  provider overrides it to spell `oidc:<audience>` *without firing the
  exchange* — the summary is a projection, in dry and in live, so the
  forecast says what a live run would do and no summary ever performs
  network work. (A conscious, additive trait widening, the ADR-0018 §5
  pattern.)
- **The dry forecast disables the exchange** (`exchange_enabled =
  false`): credential resolution stays honestly `Absent` while the
  projection still spells `oidc:<audience>`. The dry spawn runs the
  tool's documented `--dry-run` form, which needs no token.
- **The CLI wire-up** (`cmd_publish`): the context detection joins the
  auth-capture refusal point — a half-configured OIDC context refuses
  both modes *before any plan, probe or spawn*, offline-testable; a
  live run builds the provider over `UreqTransport`, a dry run over
  the same shape with the exchange disabled.
- **The doctor** gains a `publish-oidc` row (context present / absent
  / refusal — read-only, no network) and the `publish-auth` row now
  lists the documented candidate variables per registry (the
  `variables_for` doc's "the grammar is data" claim is finally literal).
- **The layer rule widens consciously**: `halyard-publish` now depends
  on `halyard-forge` (the transport vocabulary) and `serde_json` (the
  two documented body shapes). The crate still owns no HTTP client.
  The check-layers rule and this ADR record the widening.

## Alternatives rejected

- **Let cargo's own trusted publishing govern** (inject nothing, let
  the tool exchange): ties the forecast to the packaged cargo's
  version and behavior, and the provenance summary could no longer say
  deterministically what would authenticate. halyard's own exchange
  works with any cargo and makes the summary exact.
- **Reuse the forge-github App auth's JWT signer** (mint the id-token
  ourselves): wrong shape — the runner *is* the identity provider; the
  id-token must come from the metadata service, not be self-signed.
- **A new stable code (`E-PUB-OIDC`)**: every OIDC refusal is an auth
  failure (misconfigured context, unexchangeable token) — the
  `E-PUB-AUTH` page's family. One code, one page, the taxonomy stays
  70 codes.

## Consequences

- A `publish` run in a GHA workflow with `id-token: write` and no
  `CARGO_REGISTRY_TOKEN` authenticates by trusted publishing; the
  provenance row reads `oidc:crates.io`; no long-lived secret needed.
- The two p50-type guarantees are preserved: determinism (the memo,
  the name-sorted secrets, the projected summary) and no network in
  dry (I5).
- Real-GHA end-to-end remains a nightly lane concern (the forge-smoke
  pattern); the sandbox contract runs offline over the scripted
  transport, as every REST adapter before it did.
- npm-side OIDC (provenance / `--provenance` argv support) stays
  absent-not-stubbed — its own VERIFY slice when it lands.
