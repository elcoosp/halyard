# E-CHLOG-FILE

**Code:** `E-CHLOG-FILE` · **Area:** changelog (M3.x) · **Raised by:**
`render_plan` (existing-document insertion)

## What it means

The existing changelog text cannot be updated: there is no
`## [Unreleased]` section, no `<!-- halyard:insert -->` marker and no
header block to anchor the insertion (§I1 idempotent insertion).

## Cause

- the changelog does not follow keep-a-changelog structure at all;
- the file is empty or contains only prose;
- the file was truncated after the header.

## Fix

Add an `## [Unreleased]` heading (or the `<!-- halyard:insert -->`
marker) at the intended insertion point, or let halyard create a fresh
document.

## Command

```bash
head -20 CHANGELOG.md          # check the header/Unreleased structure
grep -n "halyard:insert" CHANGELOG.md
```
