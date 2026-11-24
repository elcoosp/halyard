# The plan contract

`halyard plan` computes, for every package in the workspace, the next version
and the reasons for it. The output is the **plan** — a pure function of the
git history, the discovered workspace, and the resolved configuration.

## What the plan contains

For each release row: the package name, ecosystem, the version transition
(`from` → `to`), the derived bump level, the tag it will produce, the
changelog entry preview, and the file edit sites. Skipped packages are
reported too, with the reason (release disabled, no pending commits, version
unchanged). Every row carries a **reason chain**: which commit drove the
bump, which dependency cascaded it, or which explicit input (`Release-As`
footer, change file) overrode it — `halyard explain <pkg>` renders the chain
for one package.

## Determinism and the plan hash

The canonical plan document (`halyard-plan/v1`) is serialized as canonical
JSON (sorted keys, no insignificant whitespace) and hashed with blake3. Same
inputs, same `plan_hash`. This is what makes the rest of the pipeline
verifiable:

- CI can compute a plan and gate on its hash.
- The Release PR body **embeds** the plan (canonical JSON → DEFLATE →
  base64). When `halyard release` runs, it extracts the embedded plan from
  the PR body, recomputes the hash, and refuses to tag if the PR it is about
  to release does not match the plan it just computed — a stale or edited PR
  cannot silently release something else.
- Reports are byte-stable across runs: package rows are sorted, ties are
  broken deterministically, and no wall-clock timestamps enter the output
  (work counters stand in for timing).

## The inputs that reach the plan

- Conventional commits (spec-compliant 1.0.0: scopes, breaking-change
  footers, `(#NNN)` PR extraction, `fixup!`/`squash!` handling) attributed
  per package through the git engine's tree diff.
- `.halyard/changes/*.md` change files, for squash-merge repositories: an
  explicit bump plus optional notes, validated against known package names.
- `Release-As: <version>` footers on commits reachable from the base, which
  force a target version.
- The versioning policy (types → bump mapping, 0.x pre-major remap, cascade
  mode, prerelease channels, maintenance lines) from `halyard.toml` — see
  [Configuration](../reference/config.md).

## Tags and templates

Every release row's tag comes from a tag template — the appendix-A.3 grammar
of literal text plus `{name}`, `{version}`, `{major}`, `{minor}`, `{patch}`,
`{ecosystem}` placeholders. Single-package repositories default to
`v{version}`, independent packages to `{name}-v{version}`; the same grammar
drives planning, the tag phase, and the smart fetch, so the three can never
disagree about what a tag looks like.
