# Research: competitor CLI surfaces (M0.3/M0.6, observed locally)

**Observed:** 2026-11-12 · **Method:** installed the tools and captured
`--help` output verbatim in this workspace. Nothing below is from memory or
folklore; anything not observed is marked pending.

## release-please 17.11.2 (npm)

- Top-level commands: `release-pr`, `github-release`, `manifest-pr`
  (deprecated), `manifest-release` (deprecated), `bootstrap`,
  `debug-config`.
- `release-pr --help` (captured): `--token` ("GitHub token with repo write
  permissions"), `--api-url`, `--graphql-url`, `--target-branch`,
  `--repo-url` (required), `--dry-run` ("Prepare but do not take action"),
  `--local`, `--local-path`, `--local-clone-depth`, `--release-as`,
  `--bump-minor-pre-major`, `--bump-patch-for-minor-pre-major`, `--plugin`.
- Conclusion (recorded in `tools.toml` as `needs_network = true`): `release-pr`
  requires the GitHub API even with `--dry-run`/`--local` (PR state lives
  server-side). Offline measurement impossible; pending a networked run.

## cocogitto 7.0.0 (`cog`)

- Commands: `check`, `commit`, `edit`, `log`, `verify`, `changelog`,
  `get-version`, `bump`, `init`, `install-hook`,
  `generate-completions`.
- `cog bump --help` (captured): requires one of
  `--version | --auto | --major | --minor | --patch`; flags include `--pre`,
  `--build`, `--hook-profile`, `--package` (monorepo), `--annotated`,
  `--dry-run`.
- Measured (harness run on the `fd` corpus fixture): **exit 1** —
  `cog bump --auto --dry-run` refuses when it cannot anchor a bump: stderr
  says "to bump current version … Only feature, bug fix and breaking change
  commits will trigger an automatic bump", with a suggestion link to
  conventionalcommits.org. Root cause: `--depth 1` clone carries **no tags**
  (see `shallow-exclude-support.md`). Real finding: cocogitto's plan step is
  tag-anchored; a tagless repo is an error, not a "first release" flow.

## knope 0.23.2

- Commands: `release`, `document-change`; global `--dry-run`, `--generate`,
  `--verbose`.
- `knope release --help` (captured): `--dry-run`,
  `--override-version`, `--prerelease-label` (env `KNOPE_PRERELEASE_LABEL`).
- Config: `knope --generate` produced (verbatim, now embedded in
  `tools.toml`) a `knope.toml` with `[package] versioned_files =
  ["Cargo.toml"]` and `[[workflows]]` steps of type `PrepareRelease`,
  `Command`, `Release`.
- Measured (harness run on `fd`): **exit 0**, dry-run printed a full plan:
  version bump of `Cargo.toml`, a `chore: prepare release <v>` commit, tag
  creation (`v10.5.2`), and push commands. Observation: knope's plan output
  is action-list shaped (not a data artifact); it works offline with `--dry-run`.

## release-plz / sampo

- Not installed in the authoring environment; argv **not observed** ⇒ no
  `tools.toml` entry (harness refuses to invent recipes). Both remain
  `pending` in `REPORT.md` until installed and their `--help` captured here.

## Claims impact

- §1.1 "Cocogitto … zero API for enrichment" — consistent with observation
  (pure git, offline) but the "no living Release PR" limitation is the
  interesting one and remains untested (needs a networked run).
- §1.1 "Knope … GitHub API use for PR inspection" — untested offline;
  `knope release --dry-run` made zero network calls locally (recorded), the
  API path is in `knope release-pr`-style flows not exercised here.
- All verdicts await the measured-run matrix on full corpus clones; nothing
  is marked verified/refuted on single observations.
