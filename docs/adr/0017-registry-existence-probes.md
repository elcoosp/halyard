# ADR-0017: the registry existence-probe crate (M5.1, §13)

- **Status:** accepted
- **Date:** 2026-11-14
- **Milestone:** M5.1 step 1 — the `Registry` seam + the crates.io/npm
  existence clients + the deterministic visibility wait (+ the `doctor
  --registries` opt-in probe); the M5.1 acceptance's "nightly live
  checks" stay deferred to the nightly lane, the M5.2 publisher is the
  declared consumer
- **Deciders:** autonomous implementation agent (§13's probe shape was
  underdetermined in three places — transport ownership, status
  semantics, backoff policy; settled per §0 rule 3 with live-verified
  facts, recorded in `docs/research/registry-existence-probes.md`)

## Context

§13's publish engine is built on one primitive: "before each publish,
`published(pkg, version)` via registry (crates.io **sparse index** …,
npm `registry.npmjs.org/<name>` conditional GET)". §17 M5.1 calls for
the registry clients as their own milestone; §7.1 reserved the
`halyard-registry` crate from the start, and §15.4 reserved the
`E-REG-*` error family (no pages existed). The M5.2 publisher needs the
primitive before the publisher itself, or the resume contract ships
untestable.

Three decisions had to be settled before the seam could be honest:

1. **Transport ownership.** The workspace already owns exactly one HTTP
   client crate (halyard-forge-github, per the §7.2 layer table's
   comment) and one transport vocabulary (ADR-0013's `HttpTransport` +
   `MemoryTransport`). A second client would duplicate timeouts, TLS
   and the test fake.
2. **Status semantics.** When is "not found" a *value* (not published)
   vs a *failure*? Both registries 404 a missing package (verified
   live); a foreign body (proxy error page, truncated JSON) must never
   read as "not published" — §13's resume would double-publish.
3. **Backoff policy.** §13 step 3 polls the index until the version is
   visible; ADR-0015 already documented why the governor's backoff is
   deterministic instead of jittered (I3: identical inputs ⇒ identical
   behavior). The same reasoning binds the visibility wait.

## Decision

1. **New crate `halyard-registry`** with the layer rule `registry →
   forge` (the transport vocabulary rides ADR-0013; the registry owns
   **no** HTTP client — the runtime injects `UreqTransport`, tests
   inject `MemoryTransport`; no `halyard-core` dependency — the probe
   speaks plain names and versions, the workspace vocabulary joins with
   M5.2). `xtask check-layers` carries the rule (17 crates).
2. **The `Registry` trait**: `published(name, version) -> Result<bool>`
   with two adapters — `CratesIo` (sparse index; the Cargo Book prefix
   scheme pinned as pure `index_path` tests, lowercased per the book)
   and `Npm` (packument `versions` map; scoped names unencoded —
   verified live). Status semantics fail closed: `404` ⇒ `Ok(false)`,
   `200` + match ⇒ `Ok(true)`, anything else ⇒ `E-REG-PROTOCOL`;
   transport failures ⇒ `E-REG-IO`; invalid names never reach the wire
   (`E-REG-NAME`). All three codes get docs pages (the §15.4 `E-REG-*`
   family's first pages).
3. **`wait_until_visible`** (§13 step 3): deterministic base-doubling
   capped backoff, no jitter; bounded by `VisibilityPolicy.max_polls`;
   the sleep sequence is observable through the injected
   `RegistrySleeper` (the ADR-0015 clock/sleeper convention). Exhaustion
   reports `Ok(false)` — the publisher renders the skip, nothing loops
   forever.
4. **Wire-up: `halyard doctor --registries`** — an opt-in probe of the
   two default registries against immutable versions (`serde@1.0.0`,
   `react@0.0.1` — registries never overwrite versions, so the probes
   are stable). The flag is the §I5 invocation; without it doctor stays
   offline. The check verifies reachability *and* protocol
   comprehension, failing with the `E-REG-*` code on any anomaly.
5. **The M5.1 acceptance's "nightly live checks"** stay deferred to the
   nightly lane (the opt-in forge-smoke pattern); the M5.2 publisher is
   the declared production consumer of the seam.

## Consequences

- `E-REG-*` pages join `docs/errors/` (the taxonomy's reserved family
  is now real); the research note records the verified facts and the
  three remaining VERIFY items (mirror URL encoding, rate limits at
  scale, yanked-version policy).
- The M5.2 publisher can be written entirely against scripted
  transports: resume (`AlreadyPublished`), visibility wait and
  `NeedsNative`-style skips are all observable offline.
- The probe is a pure read (§13 step 2's "idempotent resume" input);
  publishing itself remains M5 work and is reported as deferred, never
  faked.
