# E-GIT-OPEN

**Code:** `E-GIT-OPEN` · **Area:** git engine (M2) · **Raised by:**
`GitHistory::build*`, gix repository open

## What it means

The repository at the given path could not be opened as a git repository:
gix refused or failed the open/step-into.

## Cause

- the path is not a git repository (no `.git`, not a worktree);
- the `.git` directory is corrupted or unreadable;
- wrong working directory (the command was started outside the repo —
  use `-C <dir>`).

## Fix

Run inside the repository root (or pass `-C <dir>`); verify the checkout
is complete. For an empty directory, initialize or clone first.

## Command

```bash
halyard plan -C <repo-root>          # retry from the repository root
git -C <repo-root> rev-parse --git-dir   # git's own verdict on the checkout
```
