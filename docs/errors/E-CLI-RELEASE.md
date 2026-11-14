# E-CLI-RELEASE

**Code:** `E-CLI-RELEASE` · **Area:** CLI (tag-phase slice, §11.1) ·
**Raised by:** `halyard release` (without `--dry`)

## What it means

Live release mode was requested, but the live release shell does not
exist yet: the git-side effects after the local tag creation (annotated
tag writes over the real writer, the single `git push` of the tags,
forge releases, §11.1 step 2) are not wired, and the shell declines
rather than half-executing a release.

## Cause

- `halyard release` was run without `--dry` — the only implemented mode
  is the offline tag-phase forecast over the preview writer
  (`halyard-git::tag::PreviewTagWriter`);
- the §11.1 step-2 reconciliation is live (it decides exactly what is
  owed), its git-side effects are not.

## Fix

Use the offline forecast while the live shell is pending:

```bash
halyard release --dry
halyard --format json release --dry | jq '.created, .directives'
```

The forecast renders the directives the tag phase owes (annotated tags
at the snapshot HEAD, the §11.1 fallback target), classified against the
real ref state: `would_create` / `already_tagged` / `failed` rows with
their `E-GIT-*` codes. Nothing is written (no object, no ref) and no
network is touched (I5). For the local write itself — without push or
forge calls — the `halyard-git` seam is available to integrators today:
`GixTagWriter` + `run_tag_phase` (see `examples/tag_phase.rs`).
