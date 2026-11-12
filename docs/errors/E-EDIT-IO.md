# E-EDIT-IO

**Code:** `E-EDIT-IO` · **Area:** edit (M1.4) · **Raised by:**
`atomic_write`, `apply_to_file*`

## What it means

A filesystem step of the atomic apply protocol failed: read, temp-file
creation, write, fsync, mode preservation or rename. The temp file is
cleaned up on failure; the destination is either untouched or fully
replaced (never partially written).

## Cause

- missing permissions or a read-only checkout;
- the directory of the target does not exist;
- (Windows) `ERROR_ACCESS_DENIED` from an antivirus/indexer lock — the
  write is retried with backoff before surfacing this error.

## Fix

Check the path in the message. For read-only trees, run with write access
or point `--config`/`-C` at a writable checkout.

## Command

```bash
ls -la <the directory from the message>   # leftover temp files are cleaned automatically
cargo test -p halyard-edit --test atomic
```
