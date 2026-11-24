# E-GIT-CONTENT

**Code:** `E-GIT-CONTENT` · **Area:** git engine §11.1 step-3 content
machinery (M4.2 remainder) · **Raised by:** the content seam
(`content.rs`, [`content_tree_oid`]/[`remote_release_tip`])

## What it means

The prepare phase's content step could not build or probe the release
branch's content tree: a payload path is not a repo-relative file path,
a payload path walks through a file entry the base tree carries (the
plan's edit and the checkout disagree), or the remote probe (`git
ls-remote` / `git fetch` of the release branch) failed — unreachable
remote, auth refused, transport error. The machinery **never prompts**
for credentials (`GIT_TERMINAL_PROMPT=0`), so an auth failure surfaces
as this error instead of hanging CI.

The failure never guesses a tree: a content tree that cannot be built
or compared honestly is refused, not faked.

## Cause

- a payload path with an empty, `.` or `..` component, or a leading /
  trailing slash (a plan edit the plan validation should have caught);
- a payload path that crosses a file entry in the checkout — the
  manifest changed shape since the plan was computed (re-plan);
- no `git` binary on `PATH` (the planning read path never needs one;
  the content step is an effect step);
- the remote URL is unreachable, or the credentials are missing/expired
  (run it where `git fetch origin` works non-interactively);
- the fetched tip object could not be read back (a truncated or
  misconfigured fetch).

## Fix

Fix the payload source (re-plan on a stale checkout) or the remote and
credentials exactly as a plain git run would, then re-run. The step is
idempotent (I1): the content-tree OID is a pure function of the checkout
and the payloads, and the probe re-reads the remote tip, so a re-run
after any failure converges without duplicating a commit.

## Command

```bash
# Which URL does the remote resolve to?
git -C <repo-root> remote get-url origin
# Does the release branch exist remotely (the probe the seam runs)?
git -C <repo-root> ls-remote origin refs/heads/halyard/release/main
# Does a plain fetch work (the effect the seam runs)?
git -C <repo-root> fetch origin halyard/release/main
```
