# E-DISC-NESTED-WS

**Code:** `E-DISC-NESTED-WS` · **Area:** edit (M1 slice B) · **Raised by:** the
discovery orchestrator / graph module

## What it means

A workspace member defines its own `[workspace]` — nested workspaces are rejected with guidance (§6.3).

## Cause

- a member directory was extracted from another repository; - a member was moved under a parent workspace without merging the tables.

## Fix

Split the repositories or exclude the nested root from the outer `members` list and run it as its own repo.

## Command

```bash
cargo run -p halyard-discover --example discover_workspace
grep -n '\[workspace\]' <inner>/Cargo.toml
```
