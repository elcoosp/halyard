# E-PUB-AUTH

**Code:** `E-PUB-AUTH` · **Area:** publish auth plumbing (M5.3 step 1,
ADR-0019; the OIDC half M5.3 step 3, ADR-0031) · **Raised by:** the
[`PublishAuth`] seam — the [`EnvPublishAuth`] capture, the spawn
effect's per-target resolution, and the OIDC trusted-publishing context
detection + exchange

[`PublishAuth`]: https://docs.rs/halyard-publish/latest/halyard_publish/trait.PublishAuth.html
[`EnvPublishAuth`]: https://docs.rs/halyard-publish/latest/halyard_publish/struct.EnvPublishAuth.html

## What it means

A publish token variable is **present but empty** — the one
misconfiguration the auth seam refuses before any spawn. With a
`${VAR}` auth-file interpolation (the documented npm contract) an empty
value becomes an empty bearer token and a confusing registry 401; with
cargo it fails the tool the same way. halyard names the variable
instead of letting you diagnose it from the registry's rejection.

The refusal is a **run-level** error when the shell's capture finds it
(`halyard publish` refuses before planning the run) and a **row-level**
failure (`Failed { code: "E-PUB-AUTH" }`) when a scripted provider
resolves it per target — either way nothing is spawned after the
refusal.

Absent credentials are **not** this error: absence resolves to
[`AuthOutcome::Absent`](https://docs.rs/halyard-publish/latest/halyard_publish/enum.AuthOutcome.html)
(nothing injected — the tool's own credential store, `cargo login` or
the user-level auth file, governs the run).

## Cause

The variable is exported with an empty value — typically a CI
`${SECRETS}` placeholder that resolved to nothing, or an
`export CARGO_REGISTRY_TOKEN=` left in a shell profile.

## The OIDC trusted-publishing refusals (ADR-0031)

The same code also refuses a **misconfigured GitHub Actions OIDC
context** — detected before any network, both in live and in dry:

- **Half-present context**: `ACTIONS_ID_TOKEN_REQUEST_URL` is set but
  `ACTIONS_ID_TOKEN_REQUEST_TOKEN` is missing (or the reverse). The
  runner exposes both variables when the workflow declares
  `permissions: id-token: write`; exactly one means a half-configured
  workflow. The message names the missing variable.
- **Present-but-empty**: one of the two variables is set to the empty
  string.

A detected context is **not** an error — see the `publish-oidc` row of
`halyard doctor`. Under a detected context a live publish exchanges
the short-lived crates.io token when no documented variable covers the
registry (trusted publishing is GitHub-Actions + crates.io today;
custom registries keep the env grammar), and the exchange itself
refuses with this code on a transport failure, a non-2xx answer, a
body outside the documented shapes, or unparseable JSON. Every refusal
message is scrubbed: the context token, the id-token and the exchanged
token never ride a surfaced message.

## Fix

Give the variable the real token, or unset it entirely (absence is
honest — the tool's own credential store governs):

```bash
export CARGO_REGISTRY_TOKEN="czp+…"   # crates.io (Cargo Book)
export CARGO_REGISTRIES_<NAME>_TOKEN="…"  # other cargo registries
export NPM_TOKEN="…"                  # npm via the user-level auth file
halyard doctor                        # the publish-auth row shows what resolves
```

halyard never writes auth files (ADR-0019: pnpm's anti-exfiltration
rule is exactly why) — the token rides the environment.

## Command

```bash
# What the run would authenticate with, per registry (values never shown)
halyard doctor | grep publish-auth

# The dry forecast exercises the same capture — a misconfiguration
# refuses there before any tool spawn
halyard publish --dry --format json
```
