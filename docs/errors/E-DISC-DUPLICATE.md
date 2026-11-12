# E-DISC-DUPLICATE

**Code:** `E-DISC-DUPLICATE` · **Area:** edit (M1 slice B) · **Raised by:** the
discovery orchestrator / graph module

## What it means

Two packages in one ecosystem claim the same name (§6.3 — cargo forbids duplicate member names; npm names must be scoped uniquely).

## Cause

- two member dirs declare the same crate name; - an npm package shares an unscoped name with the root.

## Fix

Rename one package (or scope the npm name `@scope/name`).

## Command

```bash
grep -rn '^name = "<name>"' --include=Cargo.toml .
grep -rn '"name": "<name>"' --include=package.json .
```
