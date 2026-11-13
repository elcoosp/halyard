# E-CACHE-IO

**Code:** `E-CACHE-IO` · **Area:** persistent cache (M2.5) · **Raised by:**
cache read/write/fsync paths (§8.7)

## What it means

A read, write or fsync syscall on the cache failed (the message names the
operation).

## Cause

- disk full (`ENOSPC`) during an append;
- a vanished or locked cache file (NFS/overlayfs oddities);
- permission loss after open.

## Fix

Check disk space and the cache path's health. Deleting the cache file is
always safe (§8.7); with `--no-cache` planning runs uncached.

## Command

```bash
df -h <cache filesystem>
rm -f <cache-file from the message>
```
