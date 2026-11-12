# E-DISC-MANIFEST

**Code:** `E-DISC-MANIFEST` · **Area:** edit (M1 slice B) · **Raised by:** the
discovery orchestrator / graph module

## What it means

A manifest failed to parse or carries an impossible combination: malformed TOML/JSON/YAML, a member without `[package]`/`name`, or an inherited version with no `[workspace.package].version`.

## Cause

- syntax error in `Cargo.toml` / `package.json` / `pnpm-workspace.yaml`; - `version.workspace = true` without a workspace-level version; - member manifest is not valid UTF-8.

## Fix

Fix the manifest at the reported byte span. The span in the message points at the offending text.

## Command

```bash
cargo test -p halyard-eco-cargo
python3 -c "print(open('<manifest>').read()[:400])"
```
