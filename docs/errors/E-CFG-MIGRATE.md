# E-CFG-MIGRATE

**Code:** `E-CFG-MIGRATE` · **Area:** config migration (M4.7, §16.2) ·
**Raised by:** `halyard_config::migrate` / `halyard migrate`

## What it means

A foreign release-tool configuration (release-please, changesets) could
not be surveyed: the file is syntactically invalid JSON, violates its
own documented grammar (a missing `packages` map, a wrong value type),
or the migration draft failed its strict-loader validation. The
migration is fail-closed — a half-understood foreign config is never
mapped onto `halyard.toml`, because a mis-mapped pin (a tag template, a
pre-major bump level) would silently change what releases.

## Cause

- the foreign file exists but is not valid JSON (a trailing comma, a
  truncated write);
- the file is valid JSON but violates the tool's documented grammar —
  e.g. a `release-please-config.json` without its required `packages`
  map (the release-please schema marks it required,
  `additionalProperties: false` at the root), or a changesets
  `access` value outside `public`/`restricted`;
- the file exists but cannot be read (permissions) — the shell refuses
  a file it cannot survey rather than migrating around it.

## Fix

Read the diagnostic: it names the foreign file and the exact grammar
violation. Fix or remove the foreign configuration and re-run. The
foreign grammars are recorded — quotes and defaults — in
`docs/research/migrate-config-grammars.md`; a key the v1 surface cannot
express is never an error, it is an `unmapped` row in the report with
its reason.

## Command

```bash
# survey exactly what the tool sees and print the first error:
cargo run -p halyard-config --example migrate_report
# the CLI report with the same survey:
halyard migrate --format json
```
