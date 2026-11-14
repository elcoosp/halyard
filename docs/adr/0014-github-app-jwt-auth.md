# ADR-0014: GitHub App authentication (JWT → installation token) in the REST adapter

- **Status:** accepted
- **Date:** 2026-11-14
- **Milestone:** M4.1c (the follow-up ADR-0013 decision 4 declared;
  the §11.4 as-built note names it, the M4.1b session log lists it as
  the first next start point)
- **Deciders:** autonomous implementation agent
- **Research:** `docs/research/github-rest-forge.md` §7 (the §0 rule 4
  `VERIFY` artifact for this slice; every claim is cited there)

## Context

ADR-0013 shipped the GitHub REST adapter with static-token auth only
and explicitly deferred App-JWT minting to M4.1c. §11.5 names the known
limitation that makes this more than a nicety: forge actions taken with
a static `GITHUB_TOKEN` do **not** trigger the repository's own
workflows (the classic release-please closed-loop problem) — actions
taken with a GitHub App **installation token** do. The live `pr` /
`release` forge halves are real since M4.1b, so the auth surface is the
last piece the §11.5 App-auth bullet points at.

Constraints that shape the slice:

1. **No async runtime, minimal deps (§14.2, §19).** The adapter is
   blocking `ureq` + rustls; a heavyweight JWT crate (or an RSA crate
   with its own build machinery) would bloat the graph.
2. **Fail-closed before any call** (the adapter's M4.1b contract): an
   unusable key must be a construction-time `E-FORGE-AUTH`, never a
   mid-exchange surprise.
3. **Determinism where it is observable (I3).** The auth *state*
   (which token rides which call) must be a pure function of the
   injected clock — tests pin it; the crypto signature itself is
   necessarily blinded per signing and is verified, not byte-compared.

## Decision

1. **The signer is `ring::signature::RsaKeyPair`** (RSASSA-PKCS1-v1_5
   with SHA-256). `ring` is already in the runtime graph (rustls'
   provider), has no build scripts beyond `cc`, and no default features
   to prune. `halyard-forge-github` gains the direct `ring = "0.17"`
   edge (no new *package* in the lockfile) and
   `xtask/src/layers.rs` widens the adapter's external allow-list
   consciously (`serde`, `serde_json`, `ureq`, `base64`, `ring`).
2. **The PEM grammar is parsed in-crate** (`src/app_auth.rs`): PEM
   armor (RFC 7468) → base64 → DER; PKCS#1 `RSAPrivateKey` bodies are
   validated against the nine-INTEGER structure (RFC 8017 A.1.1) and
   wrapped into the PKCS#8 `PrivateKeyInfo` shape `ring` consumes (the
   wrapper is byte-identical to `openssl pkcs8 -topk8 -nocrypt`
   output); PKCS#8 PEMs are used verbatim with the inner body extracted
   for the same validation. A mini DER reader/writer (~60 lines, no
   `der` crate) covers exactly the shapes GitHub keys come in: short +
   0x81/0x82 lengths, INTEGER/SEQUENCE/OCTET STRING. Malformed input is
   a construction-time `E-FORGE-AUTH` naming the problem.
3. **The JWT mint is pure over an injected clock**:
   `AppCredentials::mint_jwt(now)` produces
   `RS256({"alg":"RS256","typ":"JWT"})` over claims `iss` = the App ID
   (or client ID — the docs accept both), `iat = now - 60` (the
   documented drift guard), `exp = iat + 600` (the documented 10-minute
   maximum). No wall clock lives in the credentials; the adapter holds
   an `EpochClock` (`Arc<dyn Fn() -> u64 + Send + Sync>`, default
   `system_epoch`) used **only** for auth state — determinism (I3) is
   testable, and nothing else in the adapter reads the clock for auth.
4. **The installation token is the adapter's own exchange**:
   `POST /app/installations/{id}/access_tokens` with the JWT as the
   bearer, recorded like any other §11.5 call; the `{ "token": … }`
   body is parsed (`E-FORGE-TRANSPORT` when unusable — never a
   fabricated credential) and cached with the adapter's **own
   conservative 45-minute TTL** (`INSTALLATION_TOKEN_TTL_SECS = 2700`).
   The wire's ISO-8601 `expires_at` is deliberately not parsed (no date
   dependency, no parsing failure mode); the 15-minute margin keeps the
   cached token strictly inside the real one-hour lifetime.
5. **The credential vocabulary is an enum**: `ForgeCredentials::{Token,
   App}` with `kind()` ("token" / "app") as the reports' observability
   surface. `GitHubRestForge::with_transport` keeps its M4.1b signature
   (token construction, unchanged behavior); the new constructors are
   `with_app_transport`, `with_credentials` and
   `with_credentials_and_clock` (the tests'/examples' determinism
   surface). Fail-closed semantics are unchanged: an empty token, or a
   key that did not parse, is `E-FORGE-AUTH` before any call.
6. **The shell resolves App credentials from the environment**
   (`HALYARD_APP_ID` + `HALYARD_APP_PRIVATE_KEY_PATH` (or inline
   `HALYARD_APP_PRIVATE_KEY`) + `HALYARD_APP_INSTALLATION_ID`): any
   `HALYARD_APP_*` variable present commits the run to App mode — a
   partial set fails closed naming what is missing, never a guessed
   fallback to the static token. Precedence: App env > `HALYARD_FORGE_TOKEN`
   > `GITHUB_TOKEN`. The wire-up covers `pr` (live), the release
   report's forge half, and `forge-smoke` (whose JSON now carries
   `auth_mode`); `doctor` reports which mode the environment resolves
   to (offline check, no network probe — I5).
7. **No hidden retries, no pacing** (unchanged from ADR-0013 decision
   5): a failed mint maps through the existing taxonomy (401/403/404 →
   `E-FORGE-AUTH`, 5xx/connection → `E-FORGE-TRANSPORT`) and the re-run
   completes (I2).

## Consequences

- The §11.5 App-auth bullet is implemented end to end: PRs, releases
  and API commits made by halyard can trigger the repository's own
  workflows when run under App credentials — the closed-loop gap that
  made `GITHUB_TOKEN`-driven Release PRs skip CI is closed.
- The `forge-smoke` nightly lane can run under App credentials in
  addition to the token contract (`HALYARD_FORGE_TOKEN` still works).
- The lockfile gains no new packages (`ring` was already there);
  the adapter's external allow-list grows by one consciously-reviewed
  entry.
- Tests: 14 App-auth contract tests (PEM shapes, fail-closed matrix,
  JWT claims + RS256 round-trip verification, mint/probe/caching/
  re-mint shapes, token-compat) plus two CLI loopback e2e tests (App
  mint+probe; partial-env fail-closed). Bench `app_auth` (JWT mint
  ≈ 535 µs at 2048 bits; scripted mint+probe pass; the cached-token
  no-mint re-run) and example `app_exchange` (deterministic, offline).
- The JWT signature is blinded per signing (`ring` requirement): tests
  verify against the public components instead of byte-comparing; the
  claims remain a pure function of the clock (pinned).

## Alternatives considered

1. **`jsonwebtoken` + `rsa` crates** — rejected: two new packages with
   real dependency trees where `ring` (already in the graph) covers the
   whole signing surface; the PEM/DER grammar GitHub keys use is ~60
   lines and now pinned by tests.
2. **Parse the wire's `expires_at`** — rejected for v1: ISO-8601
   timestamp parsing is a dependency (or a hand-rolled calendar) and a
   failure mode; our own conservative TTL is strictly safer (re-mints
   *before* expiry, never after) and keeps the adapter free of date
   logic.
3. **App auth only in the Action** (mint installation tokens outside
   halyard, feed halyard a token) — rejected: §11.5 explicitly wants the
   binary to mint natively ("JWT → installation token minted by the
   binary; no extra Action step"); the Action-only path would keep the
   CLI second-class and push the fail-closed surface into YAML.
4. **Global wall clock instead of an injected `EpochClock`** — rejected:
   the auth state must be testable and deterministic (I3); the injection
   cost is one constructor parameter.
