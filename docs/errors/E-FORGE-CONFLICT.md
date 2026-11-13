# E-FORGE-CONFLICT

**Code:** `E-FORGE-CONFLICT` · **Area:** forge seam (M4.1a, §11.4) ·
**Raised by:** every `Forge` adapter (the reference `MemoryForge`
produces it through its failpoint harness)

## What it means

A compare-and-swap precondition failed: the forge state the run based its
write on moved under us — the release branch tip advanced (the §11.2
force-with-lease expectation), or the tag/release the run was about to
create appeared concurrently.

## Cause

- two racers ran the release flow at once (the §11.2 concurrency group
  and CAS-on-branch are the primary defence; this is the backstop);
- a human force-pushed the release branch between the find and the write.

## Fix

Re-plan from the new state and retry — never force. A `Conflict` after a
tag phase may leave the run half-finished; the next run's tag phase
completes it (every step is resumable, I2, and `create_release` is
GET-by-tag-first idempotent).

## Command

```bash
halyard plan --json       # re-derive the plan from current state, review, re-run
```
