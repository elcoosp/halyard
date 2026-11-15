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
