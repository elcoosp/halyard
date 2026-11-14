# E-FORGE-AUTH

**Code:** `E-FORGE-AUTH` · **Area:** forge seam (M4.1a, §11.4) ·
**Raised by:** every [`Forge`](../../crates/halyard-forge/src/lib.rs)
adapter; the reference [`MemoryForge`](../../crates/halyard-forge/src/memory.rs)
produces it through its failpoint harness

## What it means

The forge rejected the run's credentials: they are absent, unusable, expired,
or lack the scopes the operation needs (Release PRs need `contents: write`
and `pull-requests: write`, §16.1). Since M4.1c (ADR-0014) the GitHub REST
adapter accepts two credential modes and both fail closed with this code
**before any network call**:

- **token mode** (`GITHUB_TOKEN` / `HALYARD_FORGE_TOKEN`): the variable is
  unset or empty — every adapter method declines rather than attempt an
  anonymous run;
- **App mode** (`HALYARD_APP_ID` + `HALYARD_APP_PRIVATE_KEY`/
  `HALYARD_APP_PRIVATE_KEY_PATH` + `HALYARD_APP_INSTALLATION_ID`): the PEM
  is not a parseable PKCS#1/PKCS#8 RSA private key, the env set is partial
  (App mode is committed once any `HALYARD_APP_*` variable is present — a
  guessed fallback to the static token never happens), or the
  installation-token **mint** was rejected (`401`/`403`/`404` on
  `POST /app/installations/{id}/access_tokens`).

## Cause

- the credential environment is unset or incomplete (the CLI doctor
  surfaces this before any call is made);
- a partial `HALYARD_APP_*` set (the error names the missing variables);
- the private key is not the App's RSA PEM (re-download it from the App's
  settings page);
- the token's scopes or the App installation's permissions do not cover
  the operation;
- the credential was minted for a different installation or repository.

## Fix

Configure the GitHub App env (App actions trigger the repository's own
workflows — the closed loop a static `GITHUB_TOKEN` cannot close, §11.5) or
re-export the token with the required scopes. Never paste a token or a
private key into the command line — the error message names what is missing,
never the credential.

## Command

```bash
halyard doctor            # reports the resolved auth mode before the run
halyard forge-smoke --repo OWNER/REPO --format json   # read-only probe; the JSON error carries the code
```
