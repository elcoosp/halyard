# E-CHLOG-CHANGE-FILE

**Code:** `E-CHLOG-CHANGE-FILE` · **Area:** changelog change-file input
(§9.4, ADR-0028) · **Raised by:** `halyard_changelog::change_files`
(the strict front-matter grammar; surfaced by the CLI in
`plan_from_context`)

## What it means

A `.halyard/changes/*.md` file violates the documented grammar, so it
is unusable as planning input. The message names the file and what is
wrong; nothing was mutated. The grammar is strict on purpose — a
silently-skipped file would drop a bump or a note the maintainer wrote
deliberately, so one bad file refuses the whole run instead.

## Cause

- the file does not open with a `---` front-matter delimiter, or the
  front matter is never closed (a stray markdown file in
  `.halyard/changes/` is a mistake worth surfacing, never a note);
- the front matter carries anything but the one documented key:
  `packages = { name = "bump", ... }` (unknown keys, content before or
  after the table, a missing `{ ... }` inline table);
- the `packages` table is empty — a note that names no package would
  have nowhere to merge;
- a bump value is not one of the quoted `"major"`, `"minor"`,
  `"patch"` (the grammar is TOML-style: quoted strings, `=`
  assignments);
- a package name is named twice in one file, or is not a valid bare
  name (letters, digits, `@`, `/`, `.`, `_`, `-` — npm scoped names
  like `@scope/pkg` are names, whitespace is not);
- the file is not valid UTF-8 (change files are markdown text).

## Fix

Rewrite the file to the documented shape:

```markdown
---
packages = { core = "minor", "@scope/pkg" = "patch" }
---
Free-form markdown the release notes merge verbatim.
```

Then re-run the command. Note the sibling error `E-PLAN-CHANGE-FILE`
for the case where the grammar is fine but a name does not match any
discovered package.

## Command

```bash
# the run refuses before any planning happens:
halyard plan --format json
# what the parser expects:
cargo run -p halyard-changelog --example change_file
```
