# E-CHLOG-TEMPLATE

**Code:** `E-CHLOG-TEMPLATE` · **Area:** changelog engine (M3.5) ·
**Raised by:** the minijinja template path
(`halyard-changelog`, user-supplied templates)

## What it means

A user-supplied changelog template failed to compile or render: the
minijinja engine rejected the template at load time (syntax) or raised
while rendering it against the plan's release data. Nothing was
mutated — the changelog path is fail-closed before any write.

## Cause

- a syntax error in the template (unclosed block, unknown filter);
- the template references a variable the release context does not carry
  (renamed or removed in the plan contract, `halyard-plan/v1`);
- a template type mismatch (e.g. rendering a list as a string).

## Fix

Fix the template against the release context the engine passes
(package, version, entries grouped per the configured type map), then
re-run. Test the template offline before wiring it into config:

```bash
# The changelog style registry renders the built-in styles without any
# template file — useful as the reference output the template must match.
cargo run -p halyard-changelog --example render
```

If the template must stay, keep it minimal and render it against one
plan snapshot first; the error message names the compile/render stage
that failed.
