# Research: registry existence probes (VERIFY, M5.1)

**Checked:** 2026-11-14 · **Sources:** The Cargo Book, "Registry
Index" <https://doc.rust-lang.org/cargo/reference/registry-index.html>
(fetched live) · npm registry API
<https://github.com/npm/registry/blob/main/docs/REGISTRY-API.md> and
`docs/responses/package-metadata.md` (fetched live) · **verified live
against `index.crates.io` and `registry.npmjs.org` by direct HTTP
probes during the session** (the strongest form of VERIFY).

## Verified facts (feeds M5.1/ADR-0017)

- **crates.io sparse index URL scheme** (Cargo Book "Index files",
  quoted verbatim in the fetched page): 1-character names under `1/`,
  2-character names under `2/`, 3-character names under `3/<first>/`,
  everything else under `<first-two>/<next-two>/<lowercased-name>`
  (example from the book: `cargo` → `ca/rg/cargo`). Index filenames are
  **lowercase** even though package names are case-sensitive.
  - Live check: `index.crates.io/se/rd/serde` → 200, newline-delimited
    JSON, one line per version with a `vers` field (SemVer 2.0.0) and a
    `yanked` flag; `index.crates.io/3/l/log` → 200;
    `index.crates.io/zz/yy/zzzzz-not-a-crate` → **404**.
- **Index file shape**: "Each version of the package has a separate
  line in the file. Each line contains a JSON object that describes a
  published version" — `vers` is the version field; entries are never
  modified after being added except `yanked` (so a probe of an old
  version is stable forever).
- **npm packument endpoint** (REGISTRY-API.md): `GET /{package}`
  "responds with the package metadata document" whose `versions` field
  is "a List of all Version objects for the Package" — in the fetched
  package-metadata doc, "a mapping of version numbers to objects".
  - Live check: `registry.npmjs.org/react` → 200 JSON with a `versions`
    object keyed by version strings; **scoped names ride the URL
    unencoded** (`registry.npmjs.org/@angular/common` → 200 — probed
    during the session); a missing package → **404**
    (`registry.npmjs.org/@halyard/definitely-not-a-package-xyz` → 404).

## Open VERIFY items (must not be assumed)

1. **Third-party npm mirrors with strict URL parsing:** npmjs.org
   accepts the unencoded `@scope/name` path (verified live); the
   `%2f`-encoded form (`@scope%2fname`) is the historically documented
   fallback for stricter proxies/registries. The client pins the
   unencoded form; a mirror requiring the encoded form is a documented
   gap until probed (the `with_registry_base` constructor makes the
   mirror case testable).
2. **crates.io rate limiting on unauthenticated sparse-index GETs:**
   the live probes hit no limit during the session; the documented
   crates.io rate-limit policy for the index endpoint must be checked
   before the M5.2 publisher polls in a loop at scale (the §13
   visibility wait bounds the polls per release).
3. **yanked versions:** the index line carries `yanked`; whether a
   yanked version should count as "published" for the M5.2 resume
   (re-publishing a yanked version is forbidden) is a publisher-policy
   decision — noted, deliberately out of the probe's scope (the probe
   answers "the version row exists", the publisher decides what that
   means).

## Design consequences (feeds M5.1/ADR-0017)

- `404` ⇒ not published (both registries — verified); `200` + matching
  version ⇒ published; anything else fail-closed `E-REG-PROTOCOL`
  (an unparseable answer must never read as "not published" — that
  would double-publish, §13).
- The probe rides the forge's `HttpTransport` (ADR-0013): no new HTTP
  client, the scripted `MemoryTransport` covers the contract tests, the
  blocking `UreqTransport` covers the runtime (`doctor --registries`
  and, later, the M5.2 publisher).
- The visibility wait backoff is deterministic (base doubling, capped,
  no jitter) — the I3-deviation reasoning ADR-0015 documents applies
  verbatim: identical inputs must produce identical wait behavior.
