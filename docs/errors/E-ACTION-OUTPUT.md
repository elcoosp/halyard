# E-ACTION-OUTPUT

**Code:** `E-ACTION-OUTPUT` · **Area:** action-output seam (§12 step
1, ADR-0037) · **Raised by:** the `halyard action` forecast render
(`halyard-action/src/lib.rs`: output-name validation, the env-file
entry grammar, the `GITHUB_OUTPUT` sink)

## What it means

One of the §12 outputs could not be rendered or delivered verbatim:
the output name or value violates the `GITHUB_OUTPUT` file grammar
(non-empty ASCII name without control characters or `=`; a value
carrying the heredoc delimiter on its own line), or the
`GITHUB_OUTPUT` file could not be opened or written.

## Cause

- a name or value the env-file grammar cannot carry verbatim — for
  halyard's own render this is never the package grammars (neither
  the cargo nor the npm grammar can produce a `=`), so a refusal here
  points at a hostile or corrupted plan document;
- `GITHUB_OUTPUT` set to an unwritable path (a permission denial, a
  full disk, a read-only filesystem);
- the file was deleted or replaced between halyard's writes.

## Fix

The refusal names the offending output. Check the plan for the named
package/name; check the `GITHUB_OUTPUT` path is writable. The failure
is fail-closed by design — halyard never rewrites a value to fit, so
a downstream step never gates on bytes halyard did not write.

## Command

```bash
# Render the forecast through the preview sink (no env files touched):
cargo run -p halyard-cli -- action --format json

# Inspect what the step environment resolves to:
env | grep '^GITHUB_OUTPUT\|^GITHUB_STEP_SUMMARY'
```
