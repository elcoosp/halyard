# E-CLI-COMMENT

**Code:** `E-CLI-COMMENT` · **Area:** CLI (M4.5 step 1 `check --comment`,
§11.6) · **Raised by:** `halyard check --pr-title <TITLE> --comment`

## What it means

The §11.6 check-comment mode could not post its verdict to the PR under
review: the mode was used without a title, the credentials for a live
forge call are missing, the checked-out HEAD names no branch, or the
forge call itself failed.

## Cause

- `--comment` without `--pr-title` — the comment *is* the verdict, so
  there is nothing to post;
- no live forge credentials (`HALYARD_APP_*` App env, `GITHUB_TOKEN` or
  `HALYARD_FORGE_TOKEN` all absent/empty) — the comment posts for real,
  it is never a forecast;
- no GitHub-shaped `origin` remote — the slug the adapter addresses
  comes from the remote URL;
- detached HEAD (and no `GITHUB_HEAD_REF`) — the comment targets the PR
  found by its head branch, and a detached checkout names none;
- the forge call failed (transport, auth, budget) — the `E-FORGE-*`
  code carries the cause.

## Fix

Run the mode in a PR workflow job (`GITHUB_HEAD_REF` carries the head
branch) with credentials configured, against a GitHub-shaped origin:

```bash
halyard check --pr-title "feat(parser): accept spans" --comment
halyard --format json check --pr-title "feat: x" --comment | jq '.error.code'
halyard doctor
```

An open PR whose head is a fork (`owner2:branch` rather than
`owner1:branch`) is an honest skip — the report says so and nothing is
posted; the bot/IDE mode (M11) owns fork-aware targeting.
