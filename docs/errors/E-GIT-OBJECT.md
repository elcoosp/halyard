# E-GIT-OBJECT

**Code:** `E-GIT-OBJECT` · **Area:** git engine (M2) · **Raised by:**
walk/commit reads (`walk.rs`, `refs.rs` peeling)

## What it means

An object (commit, tag, tree) could not be read from the object database:
the ODB returned an error for an object the walk needed.

## Cause

- a truncated or corrupted pack (interrupted clone/fetch);
- a blobless/partial clone whose needed object cannot be fetched
  (offline with `--filter=blob:none`);
- a tag object that cannot be peeled.

## Fix

Re-fetch or re-clone the repository and verify with `git fsck`. Partial
clones work for planning (trees only, §8.4) but need at least one
successful fetch for missing objects.

## Command

```bash
git -C <repo-root> fsck --full
git -C <repo-root> fetch --tags --force
```
