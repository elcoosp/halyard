# E-GIT-HEAD

**Code:** `E-GIT-HEAD` · **Area:** git engine (M2) · **Raised by:**
`GitHistory::build*` (HEAD resolution)

## What it means

HEAD is unborn or missing: nothing has been committed yet, so there is no
history to plan against.

## Cause

- a freshly `git init`-ed repository with zero commits;
- a detached/ambiguous HEAD in a broken checkout.

## Fix

Make the initial commit first. Planning needs at least one commit
(release bases and the painter walk resolve from HEAD).

## Command

```bash
git -C <repo-root> rev-parse --verify HEAD   # fails on unborn HEAD
git -C <repo-root> log --oneline -1          # what HEAD points at
```
