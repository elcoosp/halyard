# E-CACHE-OPEN

**Code:** `E-CACHE-OPEN` · **Area:** persistent cache (M2.5) · **Raised by:**
`Cache::open` (§8.7)

## What it means

The cache directory or cache file could not be created or opened.

## Cause

- unwritable cache root (permissions, read-only filesystem);
- the configured cache path is a file where a directory is needed;
- disk full or path too long.

## Fix

Point the cache at a writable location (or remove the offending path).
The cache is always safe to delete: it is a pure memoization layer
(§8.7) — deleting it never invalidates planning results.

## Command

```bash
ls -la <cache-path from the message>
rm -rf <cache-path>   # always safe to delete, §8.7
```
