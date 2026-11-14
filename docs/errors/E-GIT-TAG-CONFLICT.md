# E-GIT-TAG-CONFLICT

**Code:** `E-GIT-TAG-CONFLICT` · **Area:** git engine tag phase (M4.2b,
§11.1 step 2) · **Raised by:**
the tag write path (`tag.rs`, [`TagWriter`])

[`TagWriter`]: https://docs.rs/halyard_git/latest/halyard_git/tag/trait.TagWriter.html

## What it means

A release tag that the tag phase owes already exists, but at a
**different** commit than the phase's snapshot. Fail-closed by design:
the reconciliation never moves a release tag — a moved tag would
misstate what was released (and any downstream consumer that fetched the
tag would silently change meaning).

## Cause

- the manifest version was re-released onto a newer commit (a reverted
  merge re-merged, a rewritten history, an amended release commit);
- a stale snapshot: the phase ran against a HEAD older than the commit
  the tag already carries;
- two worktrees/clones of the same repository drifted apart and both ran
  a tag phase.

## Fix

Decide deliberately which commit the tag should carry, then either
re-point or delete the existing tag by hand and re-run the phase:

```bash
# What does the existing tag point at?
git -C <repo-root> rev-parse "<tag-name>^{}"
# If the old target is wrong, re-point deliberately:
git -C <repo-root> tag -f -a "<tag-name>" -m "<message>" <correct-oid>
git -C <repo-root> push --force-with-lease origin "refs/tags/<tag-name>"
# Or remove a bogus tag and let the phase recreate it:
git -C <repo-root> tag -d "<tag-name>"
```

Re-run the phase afterwards: satisfied directives report
`AlreadyTagged`, the freed name is created at the snapshot's target.
