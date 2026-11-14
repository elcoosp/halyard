# ADR-0013: the GitHub REST adapter crate and the transport seam

- **Status:** accepted
- **Date:** 2026-11-14
- **Milestone:** M4.1b (the §17 M4.1 slice ADR-0011 deferred; the
  author-declared next start point in the 2026-11-14 session log)
- **Deciders:** autonomous implementation agent
- **Research:** `docs/research/github-rest-forge.md` (the §0 rule 4
  `VERIFY` artifact; every endpoint/header claim is cited there)

## Context

ADR-0011 shipped the [`Forge`] seam with the deterministic `MemoryForge`
reference adapter and left "the GitHub REST adapter (ETag, backoff,
App-JWT, wiremock suites, ≤ 2-call budget asserted over real HTTP)" to
M4.1b. The live release shell (ADR-0012) has been reporting its forge
half as honestly deferred since; `halyard pr` (live) declines with
`E-CLI-PR`, and the nightly `forge-smoke` job stays latent because its
subcommand does not exist.

Three constraints shape the slice:

1. **Layer rule.** `xtask/src/layers.rs` pins `halyard-forge → core`
   with an *empty* external allow-list ("the REST adapter in M4.1b
   inherits the rule"). A real adapter needs an HTTP client and JSON —
   putting `ureq` inside the seam crate would silently widen a rule the
   table must widen consciously.
2. **No async runtime.** §14.2 forbids tokio in the planning path and
   §19 names the default: "HTTP via blocking `ureq`" (+ rustls,
   "smallest, no async runtime").
3. **The mock-forge lane must stay dependency-free.** §15.1 wants an E2E
   lane over a mock forge; wiremock is tokio-based and would drag the
   async runtime into the test graph the layer rules exclude.

## Decision

1. **The transport vocabulary lives in the seam crate** (`halyard-forge/src/transport.rs`):
   plain-data `HttpRequest`/`HttpResponse`, the object-safe
   `HttpTransport` effect seam (one `execute` — any HTTP *status* is a
   value; only connection-level failures are errors), and the scripted
   **`MemoryTransport`** reference fake (the transport analogue of
   `MemoryForge`: FIFO of recorded responses/failures + the sent-request
   log). No HTTP client, no new dependency — the seam crate's layer rule
   is unchanged.
2. **The adapter lives in a new crate `crates/halyard-forge-github/`**:
   `GitHubRestForge`, a full `Forge` implementation (all eight methods,
   no default-declines left) over REST v3 (headers, endpoints and
   failure mapping per the research note), plus **`UreqTransport`** —
   the blocking-ureq transport impl (rustls TLS, mandatory
   `User-Agent`/`Accept`/`X-GitHub-Api-Version` headers, `Bearer` auth,
   status-bearing responses). Layer rule added consciously:
   `halyard-forge-github → core + forge`, externals `serde`,
   `serde_json`, `ureq` (+ crate-local `base64`, mirroring
   halyard-changelog's use). The CLI (unrestricted) depends on it.
3. **`ForgeError::Transport { reason }`** joins the taxonomy
   (`E-FORGE-TRANSPORT`, page + index) — the taxonomy doc already
   reserves "transport" failures; M4.1a simply had no producer. Memory/
   preview adapters are unaffected; the machine fails closed on it like
   any other code.
4. **Auth scope of this slice: token (PAT/fine-grained/Actions
   `GITHUB_TOKEN`) via `Bearer`.** App-JWT minting (RSA/RS256) is the
   declared follow-up (M4.1c) — its header shape is already verified, so
   it lands without taxonomy churn. The adapter fails closed with
   `E-FORGE-AUTH` before any call when constructed without a token.
5. **No hidden retries/backoff inside the adapter.** Rate limiting maps
   to `ForgeError::RateLimited { retry_after_secs }` (from
   `retry-after`, else `x-ratelimit-reset - now`, else the documented
   60 s floor); conflicts to `ForgeError::Conflict` (I1: re-run, never
   force). A retry loop would be an unaccounted §11.5 call and the
   pacing governor is M5.5's own milestone.
6. **`upsert_pr`'s `base`** is derived by stripping the §11.2
   `halyard/release/` prefix from the branch — the documented inverse
   mapping, zero extra REST calls; a non-release-shaped branch is an
   `E-FORGE-REQUEST` caller bug (fail-closed).
7. **Wire-up (the shell):** `halyard pr` (live) runs the §11.1 machine
   over the real adapter (the E-CLI-PR decline is replaced); the live
   `release` report's forge half switches from "deferred" to real
   GET-by-tag-first release creation when credentials allow it (honest
   report otherwise); and `halyard forge-smoke` — the read-only probe
   the nightly job already invokes (`HALYARD_FORGE_TOKEN`) — arms the
   latent nightly lane. Repo coordinates come from the git remote via
   the changelog crate's remote-shape parser (the same grammar
   `compare_url` owns); `GITHUB_API_URL` overrides the base for
   GitHub Enterprise and the loopback mock tests.

## Consequences

- The §15.1 mock-forge E2E lane has its vocabulary: contract tests drive
  `GitHubRestForge` through `MemoryTransport`; one loopback test drives
  the real `UreqTransport` against a `std::net` HTTP server — no new
  test dependencies.
- The live `pr`/`release` forge effects are real now: I1/I2 hold by the
  machine's existing semantics (GET-by-tag-first, find-by-branch, PATCH
  rewrites; failures fail closed and the re-run completes).
- The `E-FORGE-*` set grows by one code; every variant still has its
  docs page before first real-network use (the ADR-0011 pattern).
- The nightly `forge-smoke` job becomes functional (still opt-in via
  `vars.FORGE_SMOKE_ENABLED`); nothing in the default CI touches the
  network (I5).

## Alternatives considered

1. **Adapter module inside `halyard-forge` (feature-gated, §7.1's
   sketch)** — rejected: it would widen the seam crate's pinned
   core-only external allow-list to make room for an HTTP client and
   feature-gate every consumer; ADR-0011 already moved the seam out of
   the §7.1 sketch once for a structural reason, and the layers table's
   own rule ("updated consciously when a crate lands") is the sanctioned
   mechanism for a new member.
2. **`reqwest` (blocking) / hyper** — rejected: §19 fixes the default
   (`ureq` + rustls, smallest, no async runtime); reqwest-blocking pulls
   tokio into the graph.
3. **Retry/backoff inside the adapter** — rejected for this slice (see
   decision 5): unaccounted calls + M5.5's governor owns pacing.
4. **wiremock for the E2E lane** — rejected: tokio-based; the scripted
   transport + loopback server cover the contract and the real client
   with zero new dependencies.
