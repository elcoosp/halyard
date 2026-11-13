# E-CHLOG-STYLE

**Code:** `E-CHLOG-STYLE` · **Area:** changelog (M3.x) · **Raised by:**
`render_plan`, `render_release`

## What it means

The requested changelog style is not shipped; the built-in styles are
`keep-a-changelog`, `conventional` and `compact` (M3.5b).

## Cause

- a config `changelog.style` (or API argument) naming an unknown style;
- a typo (`keepachangelog`, `keep-a-change-log`).

## Fix

Use one of the built-in styles. `github-release-notes` stays unshipped
(it needs forge context — compare links, remote URL) until the forge
milestone lands; a minijinja template path arrives with it.

## Command

```bash
grep -n "style" halyard.toml          # find the configured style
cargo run -p halyard-changelog --example render   # the built-in style in action
```
