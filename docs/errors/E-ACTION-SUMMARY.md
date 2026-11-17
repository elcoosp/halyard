# E-ACTION-SUMMARY

**Code:** `E-ACTION-SUMMARY` · **Area:** action-output seam (§12 step
1, ADR-0037) · **Raised by:** the `halyard action` job-summary render
(`halyard-action/src/lib.rs`: the `GITHUB_STEP_SUMMARY` sink)

## What it means

The §12 job summary (the markdown table of actions taken) could not
be delivered: the writer has no `GITHUB_STEP_SUMMARY` path, or the
file could not be opened or written.

## Cause

- the run is outside a workflow step but a summary was requested
  through the real writer (`GITHUB_STEP_SUMMARY` unset while
  `GITHUB_OUTPUT` is set — half-configured step environments refuse;
  outside a step entirely, halyard renders through the preview sink
  and never raises this);
- `GITHUB_STEP_SUMMARY` set to an unwritable path.

## Fix

Make the step environment complete (`GITHUB_OUTPUT` **and**
`GITHUB_STEP_SUMMARY` are both provided by the runner), or accept the
preview posture by unsetting `GITHUB_OUTPUT` for a local render — the
outputs are the same, only the sink differs.

## Command

```bash
# See the summary without any sink:
cargo run -p halyard-cli -- action | sed -n '/>> \$GITHUB_STEP_SUMMARY/,$p'

# Check what the step environment provides:
env | grep '^GITHUB_OUTPUT\|^GITHUB_STEP_SUMMARY'
```
