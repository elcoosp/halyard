# E-GIT-TAG

**Code:** `E-GIT-TAG` · **Area:** git engine tag phase (M4.2b, §11.1
step 2) · **Raised by:**
the tag write path (`tag.rs`, [`TagWriter`]/[`GixTagWriter`])

[`TagWriter`]: https://docs.rs/halyard-git/latest/halyard_git/tag/trait.TagWriter.html
[`GixTagWriter`]: https://docs.rs/halyard-git/latest/halyard_git/tag/struct.GixTagWriter.html

## What it means

The git-side tag phase could not carry out one directive: tag-name
validation, an unknown tag target, an incomplete tagger identity, or a
failed annotated-tag object write / reference creation. A rejected
directive writes **nothing** (fail-closed before the first side effect),
so a repository is never left half-tagged by a single failed directive.

## Cause

- the directive's tag name violates git's reference rules (empty, `..`,
  a leading `.`, a trailing `.lock`, `@{`, …);
- the directive's target oid does not name an existing commit in this
  repository (a snapshot from a different clone, or a pruned commit);
- the tagger identity is incomplete — no `user.name`/`user.email` in the
  git config and no explicit identity passed to the writer;
- the object write or the create-only ref update failed (read-only
  filesystem, ref lock contention with a concurrent process, disk full).

## Fix

Fix the input the message names, then re-run the phase — the phase is
idempotent (I1/I2): directives that already applied report
`AlreadyTagged`, the rest create.

## Command

```bash
# Which identity would the writer use?
git -C <repo-root> config user.name
git -C <repo-root> config user.email
# Does the target exist here?
git -C <repo-root> cat-file -t <target-oid>
# What does git think of the tag name?
git -C <repo-root> check-ref-format "refs/tags/<tag-name>"
```
