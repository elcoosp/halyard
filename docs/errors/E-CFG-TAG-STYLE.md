# E-CFG-TAG-STYLE

**Code:** `E-CFG-TAG-STYLE` · **Area:** config (M1.8) · **Raised by:**
`halyard_config::resolve` (`[repo].tag_style`)

## What it means

`tag_style` is neither the literal `auto` nor a valid tag template. Valid
templates contain only the placeholders `{name}`, `{version}`, `{major}`,
`{minor}`, `{patch}`, `{ecosystem}` (appendix A.3), require exactly one
`{version}`, and must not repeat a placeholder.

## Cause

- an unsupported placeholder (`v{nope}`, `{semver}`);
- a missing `{version}` (`release-{name}`);
- a duplicated placeholder (`{version}-{version}`).

## Fix

Pick `auto` (default) or a template that renders the version, e.g.
`v{version}`, `{name}-v{version}`, or `@scope/{name}@{version}` for scoped
npm packages.

## Command

```bash
# validate a candidate template against the grammar:
cargo test -p halyard-core tag_template -- --nocapture
```
