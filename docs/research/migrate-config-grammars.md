# Foreign release-tool config grammars (M4.7 step 1) — VERIFIED

Status: **VERIFIED 2026-11-15** (per §0.4: the pages below were fetched
and the quotes are verbatim; the release-please quotes come from the
official `manifest-releaser` documentation and the checked-in config
schema; the changesets quotes come from the tool's own config
documentation page).

The migration survey (`halyard migrate`, M4.7 step 1, ADR-0022) maps a
foreign tool's *documented* grammar onto the `halyard.toml` v1 surface.
Every mapped key below is backed by a quote; keys without a verified
grammar are reported as unmapped rows, never guessed.

## release-please (googleapis/release-please)

Files (fetched: `docs/manifest-releaser.md` on main, `schemas/config.json` on main):

- `release-please-config.json` — "holds releaser specific
  configuration"; the schema marks only `$schema` and `packages`
  required, with `"additionalProperties": false` at the root.
- `.release-please-manifest.json` — "holds package version tracking".

Defaults quoted verbatim from the manifest-releaser page:

- `plugins` — "absence defaults to [] (i.e. no plugins)".
- `release-type` — "absence defaults to 'node'".
- `bump-minor-pre-major` — "absence defaults to false".
- `bump-patch-for-minor-pre-major` — "absence defaults to false".
- `draft` — "absence defaults to false and Releases are created as
  already Published".
- `prerelease` — "absence defaults to false and all versions are fully
  Published".
- `changelog-path` — "defaults to 'CHANGELOG.md'" (relative to the
  package directory).
- `changelog-sections` — "absence defaults to https://git.io/JqCZL".
- `always-link-local` — "absence defaults to true".
- `separate-pull-requests` — "absence defaults to false and one pull
  request will be raised".
- `bootstrap-sha` — "ignored once a release PR has been merged".

Schema quotes (fetched: `schemas/config.json` on main):

- `include-component-in-tag` — "When tagging a release, include the
  component name as part of the tag. Defaults to `true`."
- `include-v-in-tag` — "When tagging a release, include `v` in the
  tag. Defaults to `true`."
- `tag-separator` — "Customize the separator between the component and
  version in the GitHub tag." (type string; **no default documented in
  the schema** — the observed default `-` is fixed by the
  manifest-releaser page's documented tag shape:
  "setting it to false changes the searched tag from
  `<component-name>-v<release-version>` to `v<release-version>`").
- `versioning` — "Versioning strategy. Defaults to `default`".

Mapping consequences pinned by `crates/halyard-config/tests/migrate.rs`:

- The pre-major defaults (false/false) mean release-please bumps
  pre-1.0 breaking → major and feat → minor; halyard's §10.2 defaults
  are minor/patch, so the survey pins `versioning.pre_major`
  explicitly and the row notes which side is the foreign default.
- The tag switches compose into one appendix-A.3 template
  (`{name}-v{version}` / `v{version}` / `{name}-{version}` /
  `{version}`); a per-package disagreement is reported, never mapped
  onto the single `repo.tag_style`.

Open VERIFY items (honestly open, the report does not pretend):

- `include-v-in-tag: false` + custom `tag-separator` composition was
  verified only through the two schema descriptions above; a live
  release-please run against a fixture repo is the corpus-lane
  acceptance (M4.7 full).

## changesets (changesets/changesets)

File: `.changeset/config.json` (fetched: the tool's configuration
documentation, changesets.dev/guide/config).

Defaults quoted verbatim:

- `changelog` — default `"@changesets/cli/changelog"`.
- `commit` — default `false`.
- `fixed` / `linked` — default `[]` (`string[][]`).
- `access` — default `"restricted"` (`"restricted" | "public"`).
- `baseBranch` — default `"main"`.
- `changedFilePatterns` — default `["**"]`.
- `privatePackages` — default `{ "version": false, "tag": false }`.
- `updateInternalDependencies` — default `"patch"`
  (`"patch" | "minor"`; top-level — an earlier draft of these notes
  carried a `updateInternalDependents` spelling; that key exists only
  nested under the experimental options object).
- `ignore` — default `[]`.
- `bumpVersionsWithWorkspaceProtocolOnly` — default `false`.
- `snapshot.useCalculatedVersion` — default `false`;
  `snapshot.prereleaseTemplate` — default `"{tag}-{datetime}"`.
- `___experimentalUnsafeOptions_WILL_CHANGE_IN_PATCH.updateInternalDependents`
  — default `"out-of-range"`.
- `format` — default `"auto"` (`"auto" | "prettier" | … | false`; the
  docs' `prettier` spelling is a *value* of `format`, not a key).

Open VERIFY items (the survey says so instead of guessing):

- **The git-tag grammar was NOT verified.** The search snippet quoting
  "the git tags in monorepos are created in the format … name@version …"
  points at a docs page that no longer exists at `main` (404 on the
  blob and on the raw URL); the survey therefore carries an honest
  advisory row — set `repo.tag_style` against the repository's existing
  tags (appendix A.3) — and never maps a changesets-derived template.
- `access: restricted` for npm (scoped packages / paid accounts) rides
  the row note; a live changesets→npm publish check stays open.

## knope (knope-dev/knope)

Files (fetched: the tool's own documentation site,
`knope-dev.github.io/knope` — `config/config.html`,
`config/packages.html`, `config/step/Release.html`):

- `knope.toml` — read from the working directory; the page documents
  no dot-prefixed variant. Optional: "For basic workflows, you don't
  need a config file!" Scaffolded with `knope --generate`.
- Top-level sections quoted verbatim from the config page's example:
  `[package]` — "Defined set of files to bump using semantic versioning
  and conventional commits."; `[[workflows]]` — an array-of-tables,
  each with a `name` key; `[jira]`; `[github]`.

Package shapes (fetched: `config/packages.html`):

- One package: a `[package]` section. Multiple packages: named
  sub-tables, *not* `[[package]]` arrays — `[packages."<name>"] # where
  you replace <name> with the name of the package`.
- `[package]` keys quoted verbatim:
  - `versioned_files` — "an optional array of files you'd like to bump
    the version of"; all files "must have the same version"; supported
    names: `Cargo.toml`, `pyproject.toml`, `package.json`, `go.mod`.
  - `changelog` — "the (optional) Markdown file you'd like to add
    release notes to".
  - `scopes` — "an optional array of conventional commit scopes".
  - `extra_changelog_sections` — "an optional array of extra sections
    that can be added to the changelog".
  - `assets` — "a list of files that should be included in the release"
    (each with optional `path` and `name`).
  - A `name` key and an `scm` key do not appear on the page; for a
    single-package setup "the package name \"default\" will be used for
    changesets".

Git tag grammar (fetched: `config/step/Release.html`, quoted verbatim):

- One package: "this tag will be v{version} (e.g., v1.0.0 or
  v1.2.3-rc.4)".
- Multiple packages: "each package gets its own tag in the format
  {package_name}/v{version}" — "this is the syntax required for Go
  modules".
- No tag-format configuration exists; the shapes above apply
  automatically based on the package count.

Mapping consequences pinned by `crates/halyard-config/tests/migrate.rs`:

- The single-`[package]` shape maps `repo.tag_style = "v{version}"`;
  the `[packages]` shape maps `repo.tag_style = "{name}/v{version}"` —
  both compose into one appendix-A.3 template because the documented
  rule is uniform per shape.
- `versioned_files` rides the package survey's notes (halyard rewrites
  manifests per the discovered ecosystem, §9); `changelog`, `scopes`,
  `extra_changelog_sections` and `assets` are unmapped rows with the
  documented reasons; `[[workflows]]` names ride one advisory row.
- Both package shapes at once refuse fail-closed (`E-CFG-MIGRATE`) —
  the two shapes are the two documented layouts, and a config carrying
  both is broken, not a pick.
- A knope config with no package section carries an honest advisory
  row (set `repo.tag_style` against the existing tags) and maps no
  template.

## release-plz (release-plz/release-plz)

Files (fetched: the tool's configuration reference,
`release-plz.dev/docs/config`):

- Search order quoted from the reference: release-plz "checks these
  paths top-down, using the first that exists" relative to "the
  directory it runs in": `release-plz.toml`, `.release-plz.toml`,
  `.config/release-plz.toml`. The file is optional — the tool "is
  designed to work out of the box with sensible defaults".

Defaults quoted verbatim from the reference:

- `changelog_update` — "If true, update the changelog of the crates.
  (Default)." (the changelog is rendered in the Keep a Changelog
  format unless a custom `[changelog]` header/body overrides it).
- `git_tag_enable` — `true`.
- `git_tag_name` — a Tera template; the workspace default is
  `"{{ package }}-v{{ version }}"`, the single-crate default is
  `"v{{ version }}"`.
- `publish` — "If true, release-plz runs cargo publish. (Default)."
- `release` — `true`; `release_always` — `true`.
- `git_release_enable` — "If true, release-plz creates a git release
  for the created tag. (Default)."
- `features_always_increment_minor` — "If false (default), feature
  commits will only bump the minor version starting with 1.x releases."
- `publish_timeout` — "By default, this timeout is set to 30m."
- `[[package]]` entries: `name` is required; `release` and
  `git_tag_name` are overridable per package; the reference lists
  `version_group` ("Packages that share the same highest version") as
  package-only.

Mapping consequences pinned by `crates/halyard-config/tests/migrate.rs`:

- The Tera tag template translates onto the appendix-A.3 grammar by
  placeholder substitution (`{{ package }}` → `{name}`, `{{ version }}`
  → `{version}`); a template with any other Tera variable or block
  survives as an unmapped row — the migration never half-translates.
  Only the documented `{{ package }}`/`{{ version }}` spelling (with
  the inner spaces) translates; other spacings are honest unmapped
  rows.
- The workspace default (`{{ package }}-v{{ version }}`) maps with the
  note naming the rule halyard's own `auto` applies — the survey cannot
  observe the workspace shape, so the single-crate default rides the
  note instead of a guess.
- Per-package `release = false` becomes halyard's own
  `[[package]] release = false` override (the one key the v1 surface
  expresses exactly).
- The documented changelog default maps onto
  `changelog.style = "keep-a-changelog"` (matches halyard's built-in
  default; pinned explicitly); `changelog_update = false` reports the
  difference instead of pinning.
- `publish` is always an unmapped row: release-plz publishes by
  default, halyard publishing stays opt-in (`publish.enabled` is never
  set by a migration — invariant I7).
- `version_group` is a review row (halyard's `versioning.policy =
  "linked"` is workspace-wide, §5.4).
- Per-package tag disagreements are reported, never guessed onto the
  single `repo.tag_style`.

Open VERIFY items (honestly open, the report does not pretend):

- The Tera translation covers the two documented placeholders; a
  release-plz run against a fixture repo with a custom Tera template is
  the corpus-lane acceptance (M4.7 full).
- knope's `[github]`/`[jira]`/`[variables]` sections are mapped as
  unmapped rows from the config page's section list; their inner keys
  are not enumerated (the report's generic fallback names the note).
