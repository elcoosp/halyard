# E-CHLOG-PRBODY

**Code:** `E-CHLOG-PRBODY` · **Area:** changelog (M4.3a, §11.3) ·
**Raised by:** `extract_embedded_plan`, `render_release_body`

## What it means

A Release-PR body is not usable as the §11.3 plan carrier: the
machine-readable marker (`<!-- halyard:plan v1 … -->`) is missing, its
payload does not decode (base64 → DEFLATE → canonical JSON), or the
decoded plan fails the hash checks. Invariant I8 fails **closed** — a
tampered or truncated body yields an error, never a plan.

## Cause

- the body was edited by hand and the marker was stripped or split;
- the payload was truncated by a tool that re-wraps long lines (the
  payload is one base64 line);
- the marker hash and the embedded plan disagree (the body was rendered
  from one plan and its marker pasted from another);
- the embedded document's own `plan_hash` field does not match its
  content (tampering, or a non-halyard writer).

## Fix

Never edit a rendered Release-PR body by hand: re-render it from a fresh
plan (`halyard plan --json` → `render_release_body`, or the `halyard pr`
command when it lands). The body is deterministic (I3) — the same plan
always renders byte-identically, so re-rendering costs nothing and
repairs every mode above.

## Command

```bash
cargo run -p halyard-changelog --example render_pr_body   # render + extract roundtrip
cargo test -p halyard-changelog --test pr_body            # the fail-closed matrix
```
