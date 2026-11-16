# E-CLI-COMMIT

**Code:** `E-CLI-COMMIT` · **Area:** the §16.2 `check` commits half
(the merge-commit flow's counterpart to the §11.6 title lint) ·
**Raised by:** `halyard check --commits` — the typo gate over the
pending history's own commit subjects

## What it means

A **pending commit's type is a near-miss of a known release type** —
within edit distance 2 (the title gate's did-you-mean rule): a likely
typo (`feet:` for `feat:`, `fxi:` for `fix:`). The planner ignores
unmapped types, so the commit silently releases nothing — a lost bump
the history lint surfaces loudly instead. The same machinery, the same
rule, the same diagnosis shape as `check --pr-title`'s `E-CLI-TITLE`.

## Cause

The subject was typed wrong (`feet: …`), or the type is genuinely
nonstandard but one edit away from a known one.

## Fix

Reword the subject (`git rebase -i`, `git commit --amend` for the tip),
or — when the type is intentional — map it in
`[versioning].commit_types` so the planner conveys what it means:

```bash
halyard check --commits --format json   # the per-commit rows first
halyard check --commits --exit-code     # the release gate (exit 3) when pending
```

Mapped-to-no-bump subjects (`docs:`, `chore:` …) are informational
rows, never refusals; `fixup!`/`squash!` autosquash subjects never
reach the lint at all (the parser drops them, §8.5).
