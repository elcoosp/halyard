# E-FORGE-BUDGET

**Code:** `E-FORGE-BUDGET` · **Area:** forge seam (M5.5, §11.5) ·
**Raised by:** the call governor (`halyard_forge::governor::Governor`)

## What it means

The run's own call budget was exhausted: the governor refused the
operation *before* it reached the forge. This is not the forge
throttling the run (that is `E-FORGE-RATE-LIMIT`) — it is the local
fail-closed policy doing what §11.5 asks ("per-run call budget") and
stopping the run from spending another call.

## Cause

- the governor's `max_calls` budget was armed (shell env
  `HALYARD_FORGE_MAX_CALLS`, M5.5 wire-up) and the forwarded-attempt
  ledger reached it — every attempt counts, including ones that failed
  with an injected error or a retry (§11.5: a dropped call is an
  observable one);
- a release with many packages legitimately needs more calls than the
  armed budget (the tag phase creates one release per pending package).

## Fix

Re-run the same command after raising or unsetting the budget — every
effect step is resumable (I2), so the run continues where it stopped
without duplicate artifacts. The ledger reconciles with the adapter's
own call log (`Forge::calls`), so the run's spend is always auditable.
If a default-shaped run hits an armed budget repeatedly, re-check the
plan size before raising the cap.

## Command

```bash
# the governor's policy and event log ride the live reports (JSON):
HALYARD_FORGE_MAX_CALLS=40 halyard pr --format json | jq '.governor'
```
