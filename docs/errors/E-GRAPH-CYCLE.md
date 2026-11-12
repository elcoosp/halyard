# E-GRAPH-CYCLE

**Code:** `E-GRAPH-CYCLE` · **Area:** edit (M1 slice B) · **Raised by:** the
discovery orchestrator / graph module

## What it means

A dependency cycle through non-dev edges exists (§6.6). Dev-only cycles are allowed; this one is not. The error carries the printed path (`a -> b -> a`).

## Cause

- a circular `[dependencies]`/`[build-dependencies]` chain inside the workspace; - a self-dependency.

## Fix

Break the cycle (cargo itself cannot resolve normal-dep cycles either). The printed path names every package on the loop.

## Command

```bash
cargo run -p halyard-discover --example discover_workspace
git grep -n '<first package on the path>' -- '**/Cargo.toml'
```
