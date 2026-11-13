# E-CACHE-LOCK

**Code:** `E-CACHE-LOCK` · **Area:** persistent cache (M2.5) · **Raised by:**
per-operation advisory locking (§8.7)

## What it means

The advisory flock for a cache operation could not be acquired.

## Cause

- another halyard (or a crashed one) holds the lock on the cache file;
- a stale lock on a filesystem whose flocks do not clear on close
  (rare, NFS without proper lock daemon).

## Fix

Wait for the concurrent run to finish, or remove the cache file (safe
anywhere, §8.7) to drop the lock; re-run planning.

## Command

```bash
fuser -v <cache-file from the message>   # who holds the flock
rm -f <cache-file from the message>      # always safe, §8.7
```
