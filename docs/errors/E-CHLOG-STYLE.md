# E-CHLOG-STYLE

**Code:** `E-CHLOG-STYLE` · **Area:** changelog (M3.x) · **Raised by:** the M2 `changelog` stage in `apply` via
`StyleSource::from_config` (a style name that is not shipped, or a
non-path value that is not a known name) and
`render_plan`, `render_release`

## What it means

The requested changelog style is not shipped; the built-in styles are
`keep-a-changelog`, `conventional`, `compact` and
`github-release-notes`; user templates are recognized by shape — a
separator (`/`, `\\`) makes the value a template *path* the shell
reads (`E-CHLOG-TEMPLATE` when unreadable) (M3.5b + the notes pipeline).

## Cause

- a config `changelog.style` (or API argument) naming an unknown style;
- a typo (`keepachangelog`, `keep-a-change-log`).

## Fix

Use one of the built-in styles. `github-release-notes` renders
context-free through the style registry; the forge context (the compare
link derived from the configured remote) rides `NotesContext` into
`render_release_notes` — the per-release notes pipeline the machine's
`ReleaseSpec.notes` consumes. A minijinja template path goes through
`render_plan_template`, not a style name.

## Command

```bash
grep -n "style" halyard.toml          # find the configured style
cargo run -p halyard-changelog --example render   # the built-in style in action
```
