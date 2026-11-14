# E-REG-NAME

**Code:** `E-REG-NAME` · **Area:** registry existence probes (§13, M5.1)
· **Raised by:** the registry clients ([`CratesIo`]/[`Npm`] in
`halyard-registry`)

[`CratesIo`]: https://docs.rs/halyard-registry/latest/halyard_registry/struct.CratesIo.html
[`Npm`]: https://docs.rs/halyard-registry/latest/halyard_registry/struct.Npm.html

## What it means

The package name is not valid for the registry it is being probed on:
empty, whitespace, a `/` in the wrong place (more than one, or an empty
scope/package half), or a character outside the registry's grammar.
Fail-closed by design: an invalid name **never reaches the wire** — a
probe of a name the registry would misinterpret could silently answer
the wrong package.

## Cause

- a crate name with leading/trailing separators or characters outside
  the crate grammar (ASCII letters, digits, `-`, `_`, starting with a
  letter);
- an npm name that is neither `name` nor `@scope/name` (a bare `@name`
  without a slash, an empty scope or package half, whitespace).

## Fix

Correct the name in the plan/config the probe was driven from. The
error message quotes the offending name and the grammar it violated.

## Command

```bash
# Sanity-check the name exactly as the probe resolves it
cargo search <crate-name>            # crates.io grammar
npm view <package-name> version      # npm grammar
```
