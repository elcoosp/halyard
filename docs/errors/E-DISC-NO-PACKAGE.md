# E-DISC-NO-PACKAGE

**Code:** `E-DISC-NO-PACKAGE` · **Area:** edit (M1 slice B) · **Raised by:** the
discovery orchestrator / graph module

## What it means

The repository contains neither a workspace nor a package: no root `[package]`/`[workspace]` in `Cargo.toml` and no root `package.json`.

## Cause

- discovery was pointed at the wrong directory; - manifests are gitignored or deleted.

## Fix

Run in the repository root, or add a root manifest. `halyard plan` needs at least one package.

## Command

```bash
ls Cargo.toml package.json
pwd
```
