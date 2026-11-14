# E-CLI-PR

**Code:** `E-CLI-PR` · **Area:** CLI (M4.2b + M4.1b, §11.1) · **Raised by:**
`halyard pr` (live) when the wire-up cannot build a forge adapter

## What it means

Live Release-PR mode was requested, but the shell cannot construct the
M4.1b GitHub REST adapter's coordinates: no `remote.origin.url` is
configured, or the configured origin is not a GitHub remote (the
adapter is the GitHub REST one; foreign forges are Year-2 milestones,
M7). Without credentials the failure surfaces as `E-FORGE-AUTH`
instead — an auth problem, not a missing adapter.

## Cause

- `remote.origin.url` is unset (a purely local repository);
- the origin points at a GitLab/Gitea/other remote the v1 line does
  not serve.

## Fix

Configure a GitHub-shaped origin, or stay offline:

```bash
git remote add origin https://github.com/OWNER/REPO.git
halyard pr            # live: opens/rewrites the Release PR (needs GITHUB_TOKEN)
halyard pr --dry      # the offline forecast, no network (I5)
```
