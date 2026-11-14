# E-GIT-PUSH

**Code:** `E-GIT-PUSH` · **Area:** git engine tag-push effect (§11.1
step 2 / §11.2) · **Raised by:** the push seam (`push.rs`,
[`TagPusher`]/[`GixTagPusher`])

[`TagPusher`]: https://docs.rs/halyard-git/latest/halyard_git/push/trait.TagPusher.html
[`GixTagPusher`]: https://docs.rs/halyard-git/latest/halyard_git/push/struct.GixTagPusher.html

## What it means

The tag-push effect could not run: the `git` binary is unavailable, the
configured remote could not be probed (`git ls-remote` failed —
unreachable host, auth refused, transport error), or a `git push`
produced no parseable per-ref status. The effect **never prompts** for
credentials (`GIT_TERMINAL_PROMPT=0`) and **never forces** (`--force` is
not in the argv), so an auth failure or a remote divergence surfaces as
a failure instead of hanging or overwriting.

Per-tag outcomes are report rows, not this error: a tag already on the
remote is `AlreadyOnRemote` (I1), a remote tag sitting at a different
commit is `E-GIT-TAG-CONFLICT` (release tags are never moved), and a
push rejection is a `Failed` row carrying the git summary.

## Cause

- no `git` binary on `PATH` (the planning read path never needs one; the
  push effect step does — §11.1 lists "push tags (single `git push`)" as
  a sanctioned effect);
- the remote URL is unreachable, or the credentials are missing/expired
  (the run fails closed instead of prompting — run it where
  `git push origin` works non-interactively);
- the push exited without parseable `--porcelain` rows (a broken
  transport wrapper, a foreign `receive.denyCurrentBranch` message).

## Fix

Verify the remote and credentials exactly as a plain git run would, then
re-run the release — the phase is idempotent (I1/I2): tags already on the
remote classify as `AlreadyOnRemote`, only the missing ones are sent.

## Command

```bash
# Which URL does the remote resolve to?
git -C <repo-root> remote get-url origin
# Does a plain push work (the same effect the seam runs)?
git -C <repo-root> push --porcelain origin refs/tags/<tag>:refs/tags/<tag>
# What is already on the remote (the probe the seam runs first)?
git -C <repo-root> ls-remote --tags origin
```
