# E-CLI-CWD

**Code:** `E-CLI-CWD` · **Area:** CLI (M1.10) · **Raised by:**
`-C <dir>` handling in `halyard` main

## What it means

The CLI could not change its working directory to the requested `-C <dir>`.

## Cause

- the directory does not exist or is not a directory;
- permission denied on traversal;
- a dangling symlink.

## Fix

Check the path (spelling, existence, permissions) and re-run; `-C` is
resolved before any other work.

## Command

```bash
ls -ld <dir from the message>
halyard plan -C "$(git rev-parse --show-toplevel)"
```
