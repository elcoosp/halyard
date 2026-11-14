# E-FORGE-TRANSPORT

**Code:** `E-FORGE-TRANSPORT` · **Area:** forge seam (M4.1b, ADR-0013) ·
**Raised by:** every REST adapter's transport layer (`UreqTransport`),
the scripted `MemoryTransport` on an exhausted script, and adapters that
receive a body they cannot parse or a server-side `5xx`

## What it means

The forge exchange never produced a usable HTTP result: a
connection-level failure (DNS, TLS, connection refused, timeout), a
server-side failure (5xx), or a response the adapter cannot read. This
is never a caller bug and never a credentials problem; the run fails
closed and the re-run completes it (I2).

## Cause

- the forge endpoint is unreachable (wrong `GITHUB_API_URL`, offline
  runner, network partition);
- the forge answered `5xx` (its own outage);
- the response shape is not what the documented endpoint returns.

## Fix

Check reachability, then re-run — the machine's effects are idempotent
(GET-by-tag-first, find-by-branch), so the re-run completes the pass
with zero duplicates:

```bash
halyard forge-smoke --repo OWNER/REPO   # the read-only reachability probe
halyard doctor                          # token + remote sanity
```
