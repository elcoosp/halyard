# E-CFG-MIGRATE

**Code:** `E-CFG-MIGRATE` · **Area:** config migration (M4.7, §16.2) ·
**Raised by:** `halyard_config::migrate` / `halyard migrate` (survey and
`--write`)

## What it means

A foreign release-tool configuration (release-please, changesets) could
not be surveyed, or the migration write step refused: the file is
syntactically invalid JSON, violates its own documented grammar (a
missing `packages` map, a wrong value type), the migration draft failed
its strict-loader validation, or the `--write` pre-flight refused to
guess or clobber. The migration is fail-closed — a half-understood
foreign config is never mapped onto `halyard.toml`, and an ambiguous or
destructive write never happens, because a mis-mapped pin (a tag
template, a pre-major bump level) or a silently discarded draft would
change what releases.

## Cause

- the foreign file exists but is not valid JSON (a trailing comma, a
  truncated write);
- the file is valid JSON but violates the tool's documented grammar —
  e.g. a `release-please-config.json` without its required `packages`
  map (the release-please schema marks it required,
  `additionalProperties: false` at the root), or a changesets
  `access` value outside `public`/`restricted`;
- the file exists but cannot be read (permissions) — the shell refuses
  a file it cannot survey rather than migrating around it;
- **`--write`, target exists** — `halyard.toml` is already there; the
  write is create-only (ADR-0023) and the existing config is left
  byte-identical: merging the draft is human work, never a clobber;
- **`--write`, two tools configured** — release-please and changesets
  both draft a (different) `halyard.toml`; the refusal names every
  candidate and asks for the foreign configs to be removed or merged
  first, because picking one would silently discard the other's
  mappings;
- **`--write`, the atomic write fails** (unwritable directory, disk
  full) — no partial file is left behind: the temp sibling is removed
  and the target does not exist after the refusal.

## Fix

Read the diagnostic: it names the foreign file (or the write target)
and the exact violation. For survey failures: fix or remove the foreign
configuration and re-run. For write refusals: remove or merge the
foreign configs (the two-draft case), or merge the draft into the
existing `halyard.toml` by hand and re-run once it is gone. The foreign
grammars are recorded — quotes and defaults — in
`docs/research/migrate-config-grammars.md`; a key the v1 surface cannot
express is never an error, it is an `unmapped` row in the report with
its reason.

## Command

```bash
# survey exactly what the tool sees and print the first error:
cargo run -p halyard-config --example migrate_report
# the CLI report with the same survey:
halyard migrate --format json
# the write step's contract (plan → atomic create-only write → refusal):
cargo run -p halyard-config --example migrate_write
# the CLI write (refuses an existing halyard.toml, refuses to guess):
halyard migrate --write
```
