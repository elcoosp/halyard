# ADR-0056: the M8 step 1+2 PyPI cell — the `halyard-eco-pypi` adapter, the PEP 440 strategy, and the `uv publish` half

- **Status:** accepted
- **Date:** 2026-11-26
- **Milestone:** §17 YEAR 2, **M8** *"More ecosystems"* — the first
  listed cell, *"PyPI (`pyproject`, `uv`, PEP 440 strategy, trusted
  publishing)"*, with the acceptance row *"each: discover/edit/publish/
  resume tests + replay on 2 real repos"*. The reconciliation survey
  found the cell **missing in code** (no pypi/PEP 440/pyproject symbols
  anywhere in `crates/`; PROGRESS records no M8 row ever started).
  Step 1 lands the discovery + manifest-edit slice; step 2 lands the
  publish half (argv grammar, probe, auth, effect routing). The
  corpus-lane replay on 2 real repos stays the recorded-open remainder
  (forge/corpus lanes are token-bound, the M7 discipline).
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

M8's ecosystems are unstarted. The adapter seam
(`halyard_core::discover::EcosystemAdapter`, §7.3) carries exactly two
implementations — cargo (M1.5) and npm (M1.6) — and the plan's own
extension path is documented in the code: *"new built-ins require an
ADR"* (`Ecosystem`'s `#[non_exhaustive]` doc) and *"extending one is an
ADR"* (the publish-effect's decline message). This ADR is that ADR: the
third adapter joins the fixed §6.1 dispatch order after npm, and the
publish-effect's ecosystem table gains the `pypi` row.

The plan names four sub-grammars for this cell:

1. **`pyproject`** — PEP 621 manifests (`[project]` name/version/
   dependencies), parsed with the same span-accurate `toml_edit` parse
   the cargo adapter uses (invariant I6; never a subprocess).
2. **`uv`** — uv workspaces: `[tool.uv.workspace]` `members`/`exclude`
   globs, the root is a member, `[tool.uv.sources]` `workspace = true`
   marks proven-internal rows, the root's definitions apply to all
   members unless a member shadows the name. VERIFIED quotes in
   `docs/research/pypi-uv-grammars.md` §1 (fetched 2026-11-26).
3. **PEP 440 strategy** — the mapping between PEP 440 spellings and the
   workspace's semver-typed plan contract (§5.3 `version: Option<Version>`).
4. **trusted publishing** — `uv publish`'s own posture (below).

## Decision

### D1 — the PEP 440 strategy (lossless pairing, fail-closed elsewhere)

The plan contract's version type is semver; PEP 440 is a superset
grammar (epoch, post-, dev-, local segments, implicit release
segments, non-canonical aliases). halyard plans **exactly** the
spellings that round-trip losslessly — three-component release segments
plus the canonical `{a|b|rc}N` pre-release segment:

| PEP 440    | semver       |
| ---------- | ------------ |
| `1.2.3`    | `1.2.3`      |
| `1.2.3rc1` | `1.2.3-rc.1` |
| `1.2.3a2`  | `1.2.3-a.2`  |
| `1.2.3b10` | `1.2.3-b.10` |

Everything else the parser *recognizes* (the PEP's five segments are
parsed; `1!1.2.3`, `1.2.3.post1`, `1.2.3.dev4`, `1.2.3+local`, `1.2`,
`01.2.3`, `1.2.3rc01`, implicit `1.2.3rc`, `alpha`/`beta`/`c` aliases,
`v` prefixes) fails closed at discovery: the `non-canonical-version`
warning, an unpublishable package, no version site — never a guessed
rewrite. This is the npm adapter's invalid-semver posture carried to
PEP 440's grammar, plus the `unpublishable` flip (a package whose
version the plan contract cannot represent must not offer a release
publish).

Rejected alternative: map non-canonical spellings onto their PEP 440
normalized form (`1.2` → `1.2.0.0`-shaped rewriting, epochs into build
metadata). Rejected because the write-back would emit bytes uv/pip
accept but that are **not** what the maintainer wrote, and the tag
grammar would carry a version the manifest never did — halyard's
fail-closed convention (§15.3, I2) forbids invented semantics.

The write-back ([`halyard_eco_pypi::edit`]) renders the plan's semver
target through the strategy: `1.2.3-rc.1` arrives, `1.2.3rc1` lands —
the semver hyphen form is not valid PEP 440 and never reaches the
manifest.

### D2 — discovery scope: uv workspaces + the single-package posture

- A root `pyproject.toml` with `[tool.uv.workspace]` discovers the root
  (a member per the uv grammar — the workspace is "rooted at that
  package", so the root's `[project]` is required and its absence is a
  fail-closed `E-DISC-MANIFEST`) plus the glob-expanded member
  directories (`members` required, `exclude` optional, explicit
  non-glob members error on a missing directory — the npm posture).
- No workspace table: the root `[project]` is the single package
  (§6.1 single-package mode); nested manifests are not claimed by
  anything and are skipped. PEP 621 has no workspace concept outside
  uv; multi-project non-uv layouts stay the recorded-open remainder.
- `dynamic = ["version"]` → `RawVersionSource::Dynamic`, warned,
  unpublishable (halyard plans static versions only).
- `[tool.uv] package = false` → unpublishable (uv: "will force a
  project package not to be built and installed" — the npm
  `private: true` posture at uv's grammar).
- Dependency rows: `dependencies` (Normal), `optional-dependencies`
  extras (Optional), PEP 735 `[dependency-groups]` (Dev). Names
  normalize through the PyPA rule (lowercase, `-_.` runs → `-`); the
  specifier text is `ReqSpec::Other` verbatim (ADR-0006's
  foreign-grammar rule — the semver rewriter never rewrites it, the
  cascade still bumps dependents).

### D3 — publish: bare argv, uv's own posture, the env credential

The `uv publish` grammar is VERIFIED from the installed tool
(`uv 0.13.9`, research §2): `--dry-run` forecasts, `UV_PUBLISH_TOKEN`
is the documented credential environment variable, and
`--trusted-publishing {automatic,always,never}` exists.

- The argv rides `--dry-run` for forecasts (I3) and stays bare
  otherwise — halyard never invents a flag the config did not set
  (ADR-0033's unset-is-semantic rule): under trusted publishing uv
  decides on its own, and the registry's own access default stands.
  There is deliberately **no** `UvPublishOptions` with a
  trusted-publishing field: nothing in the pipeline feeds it (the plan
  contract's frozen `halyard-plan/v1` schema carries npm-only publish
  options, ADR-0055) and a field only tests would set is dead
  scaffolding.
- Credentials ride the auth seam (ADR-0019): the `pypi` arm resolves
  `UV_PUBLISH_TOKEN` through the same env snapshot npm uses; the
  username/password pair is the recorded-open remainder (a second
  credential shape needs its own ADR-0019-conformant resolution).
- The probe (`halyard_registry::PyPi`) mirrors the npm packument
  client's contract over the PyPI JSON API (research §7, verified live:
  `GET pypi.org/pypi/{name}/json`, the `releases` map, per-version
  `404`): `404` ⇒ not published, a matching release row ⇒ published,
  everything else fail-closed `E-REG-*`. The publish router registers
  it under the plan contract's `pypi` registry name.

### D4 — layering and wire-up

- New crate `halyard-eco-pypi` (layer rules in `xtask/src/layers.rs`:
  workspace deps `halyard-core` only; external `semver` + `toml_edit` —
  the same grammar set the cargo adapter carries).
- The orchestrator (`halyard-discover`) gains the adapter in the fixed
  dispatch order and the tree walk reads `pyproject.toml` next to
  `Cargo.toml`/`package.json`.
- The apply shell (`halyard-edit/src/plan_apply.rs`) routes
  `pyproject.toml` → the `pypi` adapter; `halyard-cli`'s
  `manifest_version` probe gains the `pypi` arm (tag-target pinpoint
  walk). `uv.lock` keeps the trait's documented
  `NeedsNative`/`Unsupported` declines — no in-memory patcher, no
  native refresh; the lockfile half is the recorded-open remainder.

## Consequences

- Third adapter live end-to-end: discovery → plan → apply → publish
  forecast → publish spawn grammar; the conformance discipline (unit +
  contract + example + bench) matches the cargo/npm pattern.
- The M8 PyPI cell is **partial** by design: the discover/edit and
  publish halves ship in-tree green; the corpus-lane replay on 2 real
  repos and the uv.lock lockfile half stay recorded-open (PROGRESS
  row), like M7's corpus lane.
- The stability contract (ADR-0055) is untouched: the plan schema
  carries no new keys (`PlanPublish` stays npm-only; pypi rows carry
  `registry = "pypi"`, `access`/`provenance` = `None`), and no new
  error code was needed (discovery/edit reuse the `E-DISC-*`/`E-EDIT-*`
  pages).
- `halyard init`'s refusal message names `pyproject.toml` now that a
  pypi-only repository is plannable (docs drift fixed with the seam).
