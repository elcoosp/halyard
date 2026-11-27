# The `generic` ecosystem's regex-edited version files — VERIFIED grammar

> M8's `generic` cell (§17 YEAR 2: *"… RubyGems, `generic` (regex-edited
> version files), Docker image tags"*). A `generic` package is a package
> whose version lives in an arbitrary text file located by a
> user-declared regular expression — the escape hatch for repositories
> no manifest grammar covers (`VERSION` files, `include/version.h`
> headers, script constants, templated docs). This note pins the three
> grammars the adapter rides: the **locator grammar** (the regex, its
> capture group and its match semantics), the **version grammar** (the
> plan contract's semver, per the ADR-0056 fail-closed discipline), and
> the **config grammar** (`halyard.toml v1`, additive optional key).
>
> Each claim below was fetched and read on 2026-11-27 against the cited
> live source (VERIFIED, the §0 discipline): the quote, the URL, and
> what it pins for halyard.

## 1. Prior art: how existing tools locate versions in arbitrary files

### 1.1 Knope — `versioned_files` with a regex (the closest grammar)

Knope's package configuration accepts arbitrary version files whose
version row is located by a user-supplied regular expression. The
capture group is **named** and **required**:

> "The regex pattern must include a named capture group called version
> using the syntax `(?<version>...)`." — *"This capture group
> identifies the version number to replace."*
> — https://knope.tech/reference/config-file/packages (section
> "Regex patterns", fetched 2026-11-27)

The replacement contract is version-shaped, not free-form:

> "The matched string must be a valid semantic version and must match
> all other versions from other versioned_files."
> — same page (fetched 2026-11-27)

And the file-list shape (`path` + `regex`):

```toml
[package]
versioned_files = [
    { path = "README.md", regex = 'https:\/\/example.com\/download\/v(?<version>\d+\.\d+\.\d+).tgz' }
]
```
— same page (fetched 2026-11-27)

**What this pins for halyard:** the locator grammar is a user-declared
pattern with a **named capture group `version`**; the captured text
must be a valid semantic version to plan (the ADR-0056 discipline:
non-parseable captures are recognized-but-non-canonical — warned,
unpublishable, no edit site, never rewritten). Knope's
*"must match all other versions"* consistency rule has no halyard
equivalent at the cell's minimal grammar: one `[[generic]]` entry is
one package with one version file (multi-file packages are the
recorded-open remainder, ADR-0062).

### 1.2 release-please — the `Generic` updater rides annotations, not user regexes

release-please's `extra-files` feature updates arbitrary files with the
`Generic` updater — and its locator is a **fixed annotation vocabulary**,
not a user-declared pattern:

> "To mark versions needing an update in those files, you will add
> annotations (usually in comments). You can annotate a line (inline)
> via: `x-release-please-version`, `x-release-please-major`, …"
> — https://github.com/googleapis/release-please/blob/main/docs/customizing.md
> (section "Updating extra files", fetched 2026-11-27)

The updater's own source defines the fixed matcher set:

```ts
const INLINE_UPDATE_REGEX =
  /x-release-please-(?<scope>major|minor|patch|version-date|version|date)/;
```
— https://github.com/googleapis/release-please/blob/main/src/updaters/generic.ts
(fetched 2026-11-27)

**What this pins for halyard:** release-please's "generic" updater
*recommits* to halyard's alternative here — it requires the user to
edit their files to carry `x-release-please-*` annotations, whereas
halyard's `generic` cell is the escape hatch for files whose shape the
user cannot or will not change. The prior art for a **user-declared
pattern** locator is Knope's `versioned_files` (§1.1), and that is the
grammar halyard adopts (the named `version` group), never the
annotation vocabulary.

## 2. The regex engine: the `regex` crate's documented semantics

The adapter compiles the declared pattern with the `regex` crate. The
syntax page pins the three facts the scanner leans on:

Named groups (the locator grammar's requirement):

> "(?<name>exp) named (also numbered) capture group (names must be
> alpha-numeric)" — "(exp) numbered capture group (indexed by opening
> parenthesis)"
> — https://docs.rs/regex/latest/regex/ (section "Grouping and flags",
> fetched 2026-11-27)

Group indexing (group 0 is the whole match):

> "The capture group at index 0 is implicit and always corresponds to
> the entire match."
> — same page (section "Example: named capture groups", fetched
> 2026-11-27)

Text (not bytes) matching:

> "A compiled regular expression for searching Unicode haystacks." —
> "Patterns themselves are only interpreted as a sequence of Unicode
> scalar values."
> — same page (sections "Structs" and "Unicode", fetched 2026-11-27)

And the exclusion halyard's validation message should name when a
pattern fails to compile (look-around and backreferences are the
common RE2/PCRE habits that do not transfer):

> "This includes, but is not limited to, look-around and
> backreferences."
> — same page (crate description, fetched 2026-11-27)

**What this pins for halyard:** the scanner parses the version file as
UTF-8 text (`std::str::from_utf8` before matching — a non-UTF-8 file
refuses `E-DISC-MANIFEST`, the rubygems posture); the capture span is
a byte span over those bytes (`Match::start()`/`end()` are byte
offsets of the UTF-8 haystack), so `version_site` points into the
original bytes (invariant I4).

## 3. The config grammar: `halyard.toml` v1 gains an additive optional key

The stability contract (M6.4, `docs/stability.md`) governs the
evolution:

> "Adding an optional key is a minor release. Renaming a key or
> changing its value grammar is breaking."
> — https://github.com/elcoosp/halyard/blob/main/docs/stability.md
> (section "Config semver policy", fetched in-tree 2026-11-27)

The `[[generic]]` array is exactly that: an additive optional key of
the v1 format, schema-regenerated via
`cargo run -p xtask -- schema-check --write` (M0.7's drift test), with
unknown-key refusal unchanged for every key halyard does not know
(the fail-closed forward-compatibility contract, untouched).

## 4. Decisions this note feeds (ADR-0062)

1. **Locator grammar:** the user's pattern must compile under the
   `regex` crate's syntax and carry **exactly one named capture group,
   `version`** — unnamed capture groups refuse at load (ambiguous
   intent; the error names `(?<version>…)` as the spelling), zero or
   multiple `version` groups refuse (Knope's own requirement, §1.1).
   The leftmost match in the file is the version row; a second match
   is consumed unmodeled with a warning (multiple-match support is
   Knope's array form — the recorded-open remainder).
2. **Version grammar:** the captured text must parse as semver to be
   canonical (ADR-0056's fail-closed discipline, the same posture every
   M8 cell ships): non-parseable captures are warned
   (`non-canonical-version`), unpublishable, no edit site, never
   rewritten; zero matches are warned (`no-version`), unpublishable.
3. **Publish posture:** a `generic` release carries **no publish
   decision** (`publish: None`, the ADR-0057 Go shape minus
   tag-only-ness) — the plan names no registry for the generic
   ecosystem, and inventing a registry name would be the one thing the
   escape hatch exists to avoid. The user's own pipeline owns
   publishing; halyard versions and tags.
4. **One file per package:** one `[[generic]]` entry = one package with
   one version file (the file IS the manifest row; `version_site`
   targets its capture span). Multi-file packages (Knope's array form)
   and shared files refuse at load — recorded-open, not silently
   re-scoped.
