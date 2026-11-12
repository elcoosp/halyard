# E-DISC-GLOB

**Code:** `E-DISC-GLOB` · **Area:** edit (M1 slice B) · **Raised by:** the
discovery orchestrator / graph module

## What it means

A member pattern is malformed or an explicit member directory has no manifest in the committed tree.

## Cause

- a typo in an explicit member path; - a member directory exists but its `Cargo.toml`/`package.json` is missing (e.g. gitignored).

## Fix

Fix the pattern or add the manifest. Glob members matching zero directories are allowed; explicit members must exist.

## Command

```bash
git ls-files | grep <member-dir>
cargo run -p xtask -- discover-diff --corpus cargo
```
