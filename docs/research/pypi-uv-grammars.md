# PyPI / uv / PEP grammars (M8 step 1+2, ADR-0056)

Verification record for the PyPI ecosystem adapter slice (`halyard-eco-pypi`).
Every quote below was fetched live in this session (2026-11-26) or captured
from the installed tool itself. Nothing in the adapter is shaped from memory
of these grammars.

## 1. uv workspaces — `docs.astral.sh/uv/concepts/projects/workspaces/` (fetched 2026-11-26)

§ "Getting started":

> To create a workspace, add a `tool.uv.workspace` table to a
> `pyproject.toml`, which will implicitly create a workspace rooted at that
> package.

> In defining a workspace, you must specify the `members` (required) and
> `exclude` (optional) keys, which direct the workspace to include or
> exclude specific directories as members respectively, and accept lists
> of globs

> Every directory included by the `members` globs (and not excluded by the
> `exclude` globs) must contain a `pyproject.toml` file.

> Every workspace needs a root, which is also a workspace member.

§ "Workspace sources":

> The `workspace = true` key-value pair in the `tool.uv.sources` table
> indicates the `bird-feeder` dependency should be provided by the
> workspace, rather than fetched from PyPI or another registry.

> Any `tool.uv.sources` definitions in the workspace root apply to all
> members, unless overridden in the `tool.uv.sources` of a specific
> member.

(The non-workspace source forms — `{ path = "…" }`, `{ git = "…" }` — name
external resolution; only `workspace = true` classifies a dependency row as
proven-internal for halyard.)

## 2. uv publish CLI grammar — the installed tool (`uv 0.13.9`, `uv publish --help`, captured 2026-11-26)

> Upload distributions to an index
> ...
>       --index <INDEX>              The name of an index in the configuration to use for publishing. [env: UV_PUBLISH_INDEX=]
>   -u, --username <USERNAME>        The username for the upload [env: UV_PUBLISH_USERNAME]
>   -p, --password <PASSWORD>        The password for the upload [env: UV_PUBLISH_PASSWORD]
>   -t, --token <TOKEN>              The token for the upload [env: UV_PUBLISH_TOKEN]
>       --trusted-publishing <TRUSTED_PUBLISHING>  Configure trusted publishing [possible values: automatic, always, never]
>       --publish-url <PUBLISH_URL>  The URL of the upload endpoint (not the index URL) [env: UV_PUBLISH_URL]
>       --check-url <CHECK_URL>      Check an index URL for existing files to skip duplicate uploads [env: UV_PUBLISH_CHECK_URL]
>       --dry-run                    Perform a dry run without uploading files
>       --no-attestations            Do not upload attestations for the published files [env: UV_PUBLISH_NO_ATTESTATIONS=]

Consequences recorded by ADR-0056:

- halyard's argv rides `--dry-run` for forecasts (the flag is uv's own;
  "Perform a dry run without uploading files").
- credentials ride `UV_PUBLISH_TOKEN` through the auth seam's environment
  pair (`-t, --token` documents the variable); the username/password pair
  stays the recorded-open remainder (a second credential in one resolution
  would need its own ADR-0019-conformant shape).
- halyard's argv stays bare on trusted publishing (no `--trusted-publishing`
  flag is invented): the possible-values row is uv's own posture, and the
  npm precedent (ADR-0033) is that unset means the tool decides.
- halyard publishes with the shell rooted at the package root (the same cwd
  discipline as the pnpm effect; `uv publish` uploads the package's own
  `dist/`).

## 3. `[tool.uv] package` — `docs.astral.sh/uv/projects/config/` (fetched 2026-11-26)

> Setting `tool.uv.package = false` will force a project package not to be
> built and installed into the project environment.

> Default value: true

Consequence: a member carrying `package = false` is an application, not a
publishable package — halyard treats it like `private: true` on npm
(unpublishable, §6.4's posture) and plans no release publish for it.

## 4. PEP 440 — `peps.python.org/pep-0440/` (fetched 2026-11-26)

> Public version identifiers MUST comply with the following scheme:
> up to five segments:
>   Epoch segment: N!
>   Release segment: N(.N)*
>   Pre-release segment: {a|b|rc}N
>   Post-release segment: .postN
>   Development release segment: .devN

> All numeric components MUST be non-negative integers represented as
> sequences of ASCII digits.

Consequence (the ADR-0056 PEP 440 strategy): halyard plans versions whose
PEP 440 spelling carries **only** the release segment (three components)
plus the pre-release segment — the exact set that round-trips losslessly
through the workspace's semver-typed plan contract. The mapping pairs PEP
440 pre-releases onto semver pre-release identifiers: `1.2.3rc1` ⇄
`1.2.3-rc.1`, `1.2.3a2` ⇄ `1.2.3-a.2`, `1.2.3b1` ⇄ `1.2.3-b.1` (semver
admits arbitrary dot-separated identifiers, so `-a.2`/`-b.1`/`-rc.1` are
valid; PEP 440's "Implicit pre-release number" rule makes a trailing
letter-only spelling `1.2.3rc` equal to `1.2.3rc1`). Epoch,
post-release, dev-release and local spellings, and implicit release
segments (`1.2`, leading zeros) are recognized by the parser but fail
closed with the discovery warning convention (`non-canonical-version`,
unpublishable) — never guessed. The write-back always renders the PEP 440
spelling into `pyproject.toml` (the semver hyphen form is not valid PEP
440).

## 5. Project-name normalization — `packaging.python.org/en/latest/specifications/name-normalization/` (fetched 2026-11-26)

> valid name consists only of ASCII letters and numbers, period, underscore
> and hyphen. It must start and end with a letter or number. This means
> that valid project names are limited to those which match the following
> regex (run with `re.IGNORECASE`):
>   ^([A-Z0-9]|[A-Z0-9][A-Z0-9._-]*[A-Z0-9])$

> The name should be lowercased with all runs of the characters `.`, `-`,
> or `_` replaced with a single `-` character.

Consequence: dependency rows match workspace members by the normalized
name (uv "selects the member that matches the dependency name") — halyard
normalizes both sides with this rule before the proven-internal join.

## 6. PEP 508 specifier grammar — `peps.python.org/pep-0508/` (fetched 2026-11-26)

> version_cmp = wsp * ( '<' | '<=' | '!=' | '==' | '>=' | '>' | '~=' | '===' )
> version_many = version_one ( wsp * ',' version_one ) *
> versionspec = ( '(' version_many ')' ) | version_many

(ABNF as rendered by the PEP.) Consequence: requirement rows parse per PEP
508 (name, optional `[extras]`, `versionspec`, `; marker`); the
requirement text is carried verbatim as `ReqSpec::Other` per ADR-0006's
foreign-grammar rule — the semver rewriter never rewrites it, the cascade
still bumps dependents.

## 7. PyPI JSON API — warehouse.pypa.io + verified live (2026-11-26)

Live: `GET https://pypi.org/pypi/uv/json` → `200` with an `info` object and
a `releases` map keyed by version string; `GET
https://pypi.org/pypi/uv/0.13.9/json` → `200` for a published version; the
per-version endpoint answers `404` for an unpublished version. The probe
(`halyard_registry::PyPi`) mirrors the npm packument client's contract:
`404` ⇒ not published, a release row matching the version ⇒ published,
everything else fail-closed (`E-REG-*`).
