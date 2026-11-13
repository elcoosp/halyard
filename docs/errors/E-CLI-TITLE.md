# E-CLI-TITLE

**Code:** `E-CLI-TITLE` · **Area:** CLI (M4.3 `check`, §11.6) · **Raised
by:** `halyard check --pr-title <TITLE>`

## What it means

The squash-merge PR title failed the release lint: merging it as-is would
not produce a release the planner can derive, or the title carries a
likely typo that would silently release nothing.

## Cause

- the title is not a conventional commit at all (`type(scope):
  description` grammar declined) — squash-merge repos derive the entire
  bump from the title, so there is nothing to map;
- the title is a `fixup!`/`squash!` autosquash subject — such commits are
  never released (§8.5);
- the title's type is not a known release type and a known type is within
  edit distance 2 (a likely typo, e.g. `feet:` → `feat:`); merging it
  would convey no bump.

## Fix

Reword the merge title to a conventional commit whose type is one of the
configured release types (`[commits.types]` in `halyard.toml`; the
diagnosis lists them). For an intentional no-release merge use a known
non-release type that is present in the mapping (e.g. `chore:`, when
mapped) instead of an unknown one.

## Command

```bash
halyard check --pr-title "feat(parser): accept spans"
halyard --format json check --pr-title "updated the parser" | jq '.error.code'
```
