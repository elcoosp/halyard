# E-REG-IO

**Code:** `E-REG-IO` · **Area:** registry existence probes (§13, M5.1)
· **Raised by:** the registry clients ([`CratesIo`]/[`Npm`] in
`halyard-registry`)

[`CratesIo`]: https://docs.rs/halyard-registry/latest/halyard_registry/struct.CratesIo.html
[`Npm`]: https://docs.rs/halyard-registry/latest/halyard_registry/struct.Npm.html

## What it means

The probe could not reach the registry: the injected transport reported
a connection-level failure (DNS, TLS, refused, timeout) or an unusable
server outcome. The probe owns no HTTP client of its own — it rides the
forge transport vocabulary (ADR-0013) — so the underlying failure text
is carried verbatim.

The distinction from `E-REG-PROTOCOL` matters for the §13 resume:
a transport failure says *the registry was not heard from* (retryable,
the §13 visibility wait propagates it), while a protocol failure says
*the registry answered something uninterpretable* (never retried
blindly, never read as "not published").

## Cause

- the network is down, or the registry hostname does not resolve;
- a proxy/firewall intercepts the TLS session;
- the server answers 5xx (the transport maps unusable server outcomes
  onto this code, mirroring `E-FORGE-TRANSPORT`).

## Fix

Verify connectivity exactly as a plain HTTP client would, then re-run —
the probes are pure reads (§13 step 2) and the visibility wait is
bounded (§13 step 3), so a retry cannot duplicate anything.

## Command

```bash
# Can this machine reach the registries at all?
curl -sS -o /dev/null -w '%{http_code}\n' https://index.crates.io/se/rd/serde
curl -sS -o /dev/null -w '%{http_code}\n' https://registry.npmjs.org/react
```
