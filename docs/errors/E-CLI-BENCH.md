# E-CLI-BENCH

**Code:** `E-CLI-BENCH` · **Area:** CLI (`bench`, §16.2, ADR-0042) ·
**Raised by:** `halyard bench --iterations 0`

## What it means

The `bench` command was asked to measure nothing: `--iterations 0` times
zero pipeline passes, so the report would carry distributions with no
basis. A measurement needs at least one timed pass — the refusal happens
before the warmup pass runs, so nothing is measured and nothing is
printed.

## Cause

- `--iterations 0` (or any value that clamps to zero) was passed
  explicitly.

## Fix

Pass a positive iteration count — the default is 8:

```bash
halyard bench --iterations 4
halyard --format json bench --iterations 0 | jq '.error.code'
```
