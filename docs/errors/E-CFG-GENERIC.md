# E-CFG-GENERIC

**Code:** `E-CFG-GENERIC` · **Area:** config (M8 ADR-0062) ·
**Raised by:** `halyard_config::resolve` (the `[[generic]]` entries)

## What it means

A `[[generic]]` entry does not fit its documented grammar. One entry is
one config-declared package whose version lives in one plain-text file
at the text the declared pattern's **named `version` capture group**
spans (the VERIFIED prior-art grammar,
`docs/research/generic-version-files.md` §1.1: *"The regex pattern must
include a named capture group called version … This capture group
identifies the version number to replace."*).

## Cause

- the `pattern` does not compile under the `regex` crate's syntax —
  look-around and backreferences are the common PCRE habits that do not
  transfer (the error carries the engine's own compile message);
- the `pattern` carries zero capture groups, more than one, or unnamed
  ones (`(\d+\.\d+\.\d+)` instead of `(?<version>\d+\.\d+\.\d+)`) — the
  contract is **exactly one capture group, named `version`**; unnamed
  groups are ambiguous intent, so they fail closed and the error names
  the `(?<version>…)` spelling;
- the `file` is not a normalized repo-relative path (`../VERSION`,
  `./VERSION`, an absolute path);
- the `name` is empty;
- the same `name` or the same `file` appears in more than one entry —
  a shared file would aim two packages' edits at one span, so it
  refuses.

## Fix

Declare the pattern with the named group, e.g.

```toml
[[generic]]
name = "squid-utils"
file = "VERSION"
pattern = '(?<version>\d+\.\d+\.\d+)'
```

(single-quoted TOML literal strings keep backslashes unescaped), give
every entry a unique `name` and a unique normalized `file`, and drop
the capture-group count to exactly the one named `version` group —
non-capturing groups `(?:…)` are fine when the pattern needs structure.

## Command

```bash
# the loader refuses the value before any planning:
halyard plan --format json
```
