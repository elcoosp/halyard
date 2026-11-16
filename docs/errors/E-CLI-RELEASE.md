# E-CLI-RELEASE

**Status: RETIRED.** No code emits this code any more — the live
release shell (M4.2b completion) reports tag and push failures through
their engine codes (`E-GIT-TAG`, `E-GIT-TAG-CONFLICT`, `E-GIT-PUSH`)
as rows in the run report, and missing credentials are an honest skip,
not an error. The page is kept for history (the code was the §11.1
release command's decline path before the shell landed); new failures
should reference the engine codes.

**Code:** `E-CLI-RELEASE` · **Area:** CLI (release command, §11.1) ·
**Raised by:** `halyard release`

## What it means

The release run failed: a tag-phase directive or a §11.2 push row
errored, so the run exits 1 with the failed rows in the report. The
phase is resumable (I2) — fix the rows the report names and re-run; the
re-run reconciles to exactly the missing remainder with zero duplicates.

The command's forge half (forge releases over a forge adapter, §11.4)
is **deferred (M4.1b)** and is reported as such — it is never faked and
never half-executed.

## Cause

- a tag directive failed (`E-GIT-TAG` rows: invalid tag name, unknown
  target, incomplete `user.name`/`user.email` tagger identity);
- a tag exists at a *different* target than the pinpointed release
  commit (`E-GIT-TAG-CONFLICT`: a moved release tag is never re-tagged —
  resolve by hand and re-run);
- the §11.2 push failed (`E-GIT-PUSH` rows: unreachable remote, auth
  refused — the run never prompts; or `E-GIT-TAG-CONFLICT`: the remote
  already carries the tag elsewhere).

## Fix

Fix what the rows name, then re-run:

```bash
halyard release                    # git side: annotated tags + §11.2 push
halyard release --dry              # the offline forecast (no writes, no network)
halyard release --no-push          # create the tags, skip the push phase
halyard --format json release | jq '.directives, .push'
```

A local-only repository (no `remote.origin.url`) releases its tags and
renders the push phase as an honest skip — that state is not an error.
