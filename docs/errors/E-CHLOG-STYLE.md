# E-CHLOG-STYLE

**Code:** `E-CHLOG-STYLE` · **Area:** changelog (M3.x) · **Raised by:**
`render_plan`, `render_release`

## What it means

The requested changelog style is not shipped in v1; the only built-in
style is `keep-a-changelog`.

## Cause

- a config `changelog.style` (or API argument) naming an unknown style;
- a typo (`keepachangelog`, `keep-a-change-log`).

## Fix

Use `keep-a-changelog`, or wait for the minijinja template path
(M3.5b) for custom styles.

## Command

```bash
grep -n "style" halyard.toml          # find the configured style
cargo run -p halyard-changelog --example render   # the built-in style in action
```
