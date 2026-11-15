# E-CLI-PUBLISH

**Code:** `E-CLI-PUBLISH` · **Area:** the `publish` command's mode
contract (M5.2 step 2, §13) · **Raised by:** the CLI shell, before the
engine runs

## What it means

`halyard publish` was asked to run **live** (no `--dry`) while the
global `--offline` flag is set. The two contradict: the live run's
resume probes (§13 step 2) and the tool upload (§13.4) are exactly the
network traffic `--offline` promises never to touch. The command
refuses closed instead of silently downgrading the run to a forecast —
a forecast that *looks* like a publish is a lie.

## Cause

A CI lane (or a shell alias) that pins `--offline` while invoking the
real publish, most often by copying a forecast invocation and dropping
the `--dry`.

## Fix

Pick one explicitly:

- the offline forecast: `halyard publish --dry` (the tools run their
  documented dry form; the registry is never probed);
- the real run: drop `--offline` (the network the run touches is the
  §I5-sanctioned publish traffic).

## Command

```bash
halyard publish --dry          # offline forecast
halyard publish                # live: probes + uploads (§13 steps 2–4)
```
