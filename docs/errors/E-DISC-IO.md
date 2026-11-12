# E-DISC-IO

**Code:** `E-DISC-IO` · **Area:** edit (M1 slice B) · **Raised by:** the
discovery orchestrator / graph module

## What it means

A filesystem step of the discovery walk failed: directory read or manifest read. The walk aborts at the first unreadable path — output is never partial.

## Cause

- unreadable/removed file mid-walk; - permission or read-only checkout; - broken symlinks into unreadable targets.

## Fix

Check the path in the message. For read-only trees, run with read access or point `-C` at a readable checkout.

## Command

```bash
cargo run -p xtask -- discover-diff --corpus cargo
ls -la <the path from the message>
```
