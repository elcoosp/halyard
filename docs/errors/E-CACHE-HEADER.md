# E-CACHE-HEADER

**Code:** `E-CACHE-HEADER` · **Area:** persistent cache (M2.5) · **Raised by:**
cache file open/validate (§8.7)

## What it means

The existing cache file's header is not a valid v1 header: wrong magic,
unknown version or bad header checksum.

## Cause

- a cache file written by a different halyard version;
- corruption (kill-during-write, filesystem damage) that survived the
  per-record checksum scan.

## Fix

Delete the file — it is safe to delete anywhere (§8.7) — or run
`halyard cache prune` when available. The next run recreates it.

## Command

```bash
rm -f <cache-file from the message>   # safe: pure memoization layer
```
