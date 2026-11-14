# E-GIT-FETCH

**Code:** `E-GIT-FETCH` · **Area:** git engine smart minimal fetch
(§8.8) · **Raised by:** the fetch seam (`fetch.rs`,
[`TagFetcher`]/[`GixTagFetcher`])

[`TagFetcher`]: https://docs.rs/halyard-git/latest/halyard_git/fetch/trait.TagFetcher.html
[`GixTagFetcher`]: https://docs.rs/halyard_git/latest/halyard_git/fetch/struct.GixTagFetcher.html

## What it means

The smart-minimal-fetch effect could not run: the `git` binary is
unavailable, the configured remote could not be probed (`git ls-remote
--tags` failed — unreachable host, auth refused, transport error), or
**both** fetch attempts exited non-zero (the §8.8 minimal fetch and the
documented full-fetch fallback of the same refs). The effect **never
prompts** for credentials (`GIT_TERMINAL_PROMPT=0`), so an auth failure
surfaces as this error instead of hanging CI.

Per-tag outcomes are report rows, not this error: a tag already local at
the probed target is `AlreadyLocal` (I1), a local release tag sitting at
a different commit is `E-GIT-TAG-CONFLICT` (release tags are never
moved), and a fetched tag is a `Fetched` row.

## Cause

- no `git` binary on `PATH` (the planning read path never needs one;
  the fetch effect step does — §8.8 is the effect §I5 names
  explicitly);
- the remote URL is unreachable, or the credentials are missing/expired
  (the run fails closed instead of prompting — run it where
  `git fetch origin` works non-interactively);
- both fetch attempts failed (the minimal fetch with the shallow
  machinery where applicable, then the full fetch of the same refs) —
  the error message carries the failing command and its stderr.

## Fix

Verify the remote and credentials exactly as a plain git run would, then
re-run `halyard fetch` — the phase is idempotent (I1): tags already
local at the probed target classify as `AlreadyLocal`, only the missing
ones are requested, and a partially completed fetch re-runs to
completion with zero duplicates (I2).

## Command

```bash
# Which URL does the remote resolve to?
git -C <repo-root> remote get-url origin
# Does a plain fetch work (the effect the seam runs)?
git -C <repo-root> fetch --no-tags origin <base-branch>
# What is already on the remote (the probe the seam runs first)?
git -C <repo-root> ls-remote --tags origin
```
