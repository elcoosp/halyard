# Halyard

Halyard is a zero-config release engine for Cargo and pnpm monorepos, written
in Rust. It reads your git history, computes the next version for every
package from conventional commits, and opens a Release PR you review before
tagging and publishing. It is built for maintainers of monorepos that span
Rust and Node, where release-please, changesets, and release-plz each cover
half the problem. Every stage has a dry-run mode, so you can see exactly what
will happen before anything is written.

The release flow is five commands:

```text
git history → halyard plan → halyard pr → review & merge → halyard release → halyard publish
```

## Why Halyard

- **Polyglot by design** — one plan covers Cargo crates and npm/pnpm packages
  in the same monorepo, with a single version graph and reverse-dependency
  closure.
- **Deterministic plans** — the plan is a pure function of the git history,
  the workspace, and `halyard.toml`, hashed with blake3. Same inputs, same
  `plan_hash`; CI can gate on it.
- **Dry-run everywhere** — `plan`, `apply`, `pr`, `release`, and `publish`
  all support `--dry`, backed by preview adapters that never touch the
  network or the working tree.
- **Verified Release PRs** — the PR body embeds the plan (canonical JSON,
  DEFLATE, base64), so `halyard release` verifies the PR it releases matches
  the plan it computed.
- **Conventional-commit native** — a spec-compliant 1.0.0 parser (scopes,
  breaking-change footers, `(#NNN)` PR extraction, `fixup!`/`squash!`
  detection), plus optional `.halyard/changes/*.md` change files for
  squash-merge workflows.
- **Affected-package selection** — `halyard affected` answers "which packages
  must rebuild and test?" and renders the answer in your CI's own grammar —
  turbo, nx, bazel, GitHub Actions, GitLab, Azure, TeamCity, CircleCI,
  Buildkite, Jenkins, Bitbucket, Drone.
- **Migration path** — `halyard migrate` surveys release-please, changesets,
  knope, and release-plz configs, reports semantic differences, and writes a
  reviewed `halyard.toml` — create-only, so existing configs are never
  clobbered.

## Status

Halyard is pre-1.0 and under active development. The planning, edit, git,
forge, registry, and publish layers are landed and tested; the CLI surface is
stabilizing but may change between minor versions.

Supported platforms follow the CI matrix: Linux, macOS, and Windows.

## Where to go next

Start with the [Quickstart](quickstart.md). The [Concepts](concepts/index.md)
section explains how the workspace is layered, what the plan contract
guarantees, and how the dry-run model works. The [Reference](reference/index.md)
section lists every CLI command, every configuration key, and the full error
taxonomy with stable codes. [Contributing](contributing.md) describes how the
workspace is developed and gated.
