# E-FORGE-RATE-LIMIT

**Code:** `E-FORGE-RATE-LIMIT` · **Area:** forge seam (M4.1a, §11.4) ·
**Raised by:** every `Forge` adapter (the reference `MemoryForge`
produces it through its failpoint harness)

## What it means

The forge throttled the run. Retryable: §15.4 maps this code onto exit
code 5 (forge rate-limited). Real adapters are expected to honour
`Retry-After`, pace content-creating calls, and keep the run under the
per-run call budget — the design goal is ≤ 2 REST calls on the update
path (§11.5), so hitting this error usually means the budget was broken.

## Cause

- the run exceeded the forge's request budget (retry header carries the
  suggested wait);
- a secondary (content-creation) limit fired despite primary budget
  remaining.

## Fix

Re-run the same command after the suggested wait — every effect step is
resumable (I2), so the run continues where it stopped without duplicate
artifacts. If it recurs, the adapter is making more calls than the §11.5
budget; file it as a bug, not as an operational error.

## Command

```bash
halyard doctor            # reports the forge kind and credential state
```
