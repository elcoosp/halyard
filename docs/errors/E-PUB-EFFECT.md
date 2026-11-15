# E-PUB-EFFECT

**Code:** `E-PUB-EFFECT` · **Area:** publish engine tool effect (M5.2,
§13 step 4 / §13.7) · **Raised by:** the [`PublishEffect`] seam's
implementations

[`PublishEffect`]: https://docs.rs/halyard-publish/latest/halyard_publish/trait.PublishEffect.html

## What it means

The publish tool for one package failed — the spawned
`cargo publish` / `pnpm publish` (or the scripted stand-in in tests)
returned an error. This is a **row-level** outcome by design (§13.7):
the run persists the failure and continues with the packages that do
not depend on it; the CLI exits non-zero and the re-run continues from
the first failure (Sampo-style resume).

The message carries the ecosystem and what the tool reported; the row
additionally classifies the failure as retryable or not (a transport-
class upload failure is retryable; a rejected request is not).

Note the gating consequence: dependents of the failed package are
*skipped* (`upstream_failed:<pkg>` in their row), because publishing a
dependent of an artifact the registry does not carry would ship a
broken release.

## Cause

Anything the tool can raise: authentication, a rejected upload, a
packaging error, an interrupted connection. The tool's own stderr is
quoted in the row message.

## Fix

Fix the underlying tool condition, then re-run publish — already
published packages are probed and skipped (`AlreadyPublished`, I1), and
the run resumes exactly where it stopped:

```bash
halyard publish            # live; resumes from the failure
halyard publish --dry      # offline forecast of what remains
```

## Command

```bash
# Re-run only one package's tool step by hand, if needed
cargo publish --dry-run --manifest-path <package-root>/Cargo.toml
pnpm publish --dry-run   # npm-package roots (§13.4: the tool owns packaging)
```
