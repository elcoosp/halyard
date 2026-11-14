# E-REG-PROTOCOL

**Code:** `E-REG-PROTOCOL` · **Area:** registry existence probes (§13,
M5.1) · **Raised by:** the registry clients ([`CratesIo`]/[`Npm`] in
`halyard-registry`)

[`CratesIo`]: https://docs.rs/halyard-registry/latest/halyard_registry/struct.CratesIo.html
[`Npm`]: https://docs.rs/halyard-registry/latest/halyard_registry/struct.Npm.html

## What it means

The registry **answered**, but the answer is not something this client
can interpret: an unexpected status (neither `200` nor the documented
`404`), a non-JSON body (a proxy error page, an HTML maintenance page),
a sparse-index line that does not parse, or a packument missing the
`versions` map. Fail-closed **by design**: an unparseable answer is
never read as "not published" — §13's resume would double-publish a
version that is actually visible.

## Cause

- a captive portal / corporate proxy serving an HTML page for every
  request;
- a mirror that answers a foreign status or body shape (the client
  pins the verified npmjs.org/index.crates.io shapes — mirrors may
  differ; the `with_index_base`/`with_registry_base` constructors make
  a mirror's shape testable before it is trusted);
- a truncating middlebox (a body that ends mid-line mid-JSON).

## Fix

Look at the quoted body/status, decide whether the path to the registry
is being tampered with (proxy) or the mirror speaks a foreign shape,
and either fix the network path or pin the mirror's base and verify its
shape against the research note (`docs/research/
registry-existence-probes.md`) before probing it.

## Command

```bash
# What does the probe actually see on this network?
curl -sS https://index.crates.io/se/rd/serde | head -1
curl -sS https://registry.npmjs.org/react | head -c 200
```
