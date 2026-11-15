# E-PUB-PLAN

**Code:** `E-PUB-PLAN` · **Area:** publish engine plan validation (M5.2,
§13 step 1 / I8) · **Raised by:** [`plan_publication`]

[`plan_publication`]: https://docs.rs/halyard-publish/latest/halyard_publish/fn.plan_publication.html

## What it means

The plan handed to the publish engine cannot be published from this
workspace. The engine validates before anything is probed or spawned —
zero side effects precede this error. The message states which check
failed:

- the plan's `plan_hash` does not match its own content (invariant I8:
  publish consumes a **verified** plan only — an edited or
  truncated plan is never half-honored);
- a publish-enabled release names no workspace package;
- a publish-enabled release targets a package the workspace marks not
  publishable (`publish = false` / `private: true`, §6.3/§6.4);
- a publish target is absent from the plan's topological `order`
  (an internally inconsistent document, §5.8);
- the plan releases one package twice.

## Cause

The plan artifact was edited after it was computed, the plan was
computed against a different workspace than the one at HEAD, or the
configuration contradicts the manifests (publish enabled for a
`publish = false` member).

## Fix

Recompute the plan at HEAD — publish never repairs or second-guesses a
plan artifact:

```bash
halyard plan --json   # the fresh, hash-consistent plan
```

If the contradiction is configuration-side, align the `[[package]]`
overrides (§16.4) with the manifests' `publish`/`private` fields and
re-plan.

## Command

```bash
# Which package does the plan claim, and what does the workspace say?
halyard plan | grep -A6 <package>
git log --oneline -3 -- <package-root>/Cargo.toml
```
