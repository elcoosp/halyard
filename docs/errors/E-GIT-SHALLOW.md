# E-GIT-SHALLOW

**Code:** `E-GIT-SHALLOW` · **Area:** git engine (M2) · **Raised by:**
`GitHistory::build*` (shallow-clone detection)

## What it means

The repository is a shallow clone; a truncated history cannot support
reliable base resolution or the painter walk (§8.1).

## Cause

- cloned with `--depth`/`--shallow-since` (CI default checkout);
- a `.git/shallow` file is present.

## Fix

Deepen the clone: `git fetch --unshallow`, or let `halyard fetch`
(M2.x) do the smart minimal fetch. On GitHub Actions, set
`fetch-depth: 0`.

## Command

```bash
git -C <repo-root> fetch --unshallow --tags
git -C <repo-root> rev-parse --is-shallow-repository
```
