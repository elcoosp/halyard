# Research: GitHub Actions outputs grammar (VERIFY, §12 step 1)

**Checked:** 2026-11-17 · Sources fetched live per §0.4.

## Workflow commands — <https://docs.github.com/en/actions/writing-workflows/choosing-what-your-workflow-does/workflow-commands-for-github-actions>

### `GITHUB_OUTPUT` (set_output)

- Single-line (Bash): `echo "{name}={value}" >> "$GITHUB_OUTPUT"` —
  the step "will need an `id` to be defined to later retrieve the
  output value".
- Multi-line: "You can set multi-line output values with the same
  technique used in the Multiline strings section" — the heredoc
  grammar:

  ```
  {name}<<{delimiter}
  {value}
  {delimiter}
  ```

  with the caution: "Make sure the delimiter you're using won't occur
  on a line of its own within the value."
- Output-name character set: **not specified anywhere on the page** —
  the grammar halyard enforces is the file format's own delimiters
  (non-empty, no control characters, no `=`), fail-closed; per-package
  keys compose the package name verbatim.

### `GITHUB_STEP_SUMMARY` (job summary)

- Basic write: `echo "{markdown content}" >> $GITHUB_STEP_SUMMARY`.
- Appending: "you can use `>>` to continuously append content for the
  current step" and "With every append operation, a newline character
  is automatically added." (halyard appends the newline itself so
  chunks never fuse headings.)
- "`GITHUB_STEP_SUMMARY` is unique for each step in a job."

### Annotations (`::warning::`)

- `::warning file={name},line={line},endLine={endLine},title={title}::{message}`
  (and the `::error::` shape) — each "will create an annotation",
  defaults `file` → `.github`, `line`/`endLine` → `1`, `title` → None.
- "Workflow command and parameter names are case insensitive."
- halyard's §12 degraded-mode warning rides the message-only form
  (`::warning::{message}`); the annotation is a single log line, so
  newlines in the message collapse to spaces.

## Decision

The env-file grammar is the action-output seam's wire format
(ADR-0037): `env_file_entry` renders the `{name}={value}` line form
for values without line breaks and the documented heredoc form for
multi-line values, with a fixed `HALYARD` delimiter (deterministic
render, I3) and a fail-closed refusal when the value carries the
delimiter on its own line (the docs' own caution — a rewritten value
is a value the downstream step gates on that halyard did not write).
