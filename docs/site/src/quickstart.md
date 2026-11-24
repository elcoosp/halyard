# Quickstart

Halyard requires a stable Rust toolchain.

## Install

```bash
# install from git (pre-1.0; crates.io release pending)
cargo install --git https://github.com/elcoosp/halyard --package halyard-cli --locked
```

To build from source instead:

```bash
git clone https://github.com/elcoosp/halyard.git
cd halyard
cargo build --release
./target/release/halyard plan
```

## First run

```bash
cd your-monorepo
halyard plan      # works with zero config in standard Cargo + pnpm layouts
halyard init      # optional: scaffold halyard.toml to customize
```

`halyard plan` prints each package's next version, the changelog preview, and
the files it would edit. Planning is pure and offline — no network, no
subprocess during planning (the git engine is in-process `gix`) — and
consults a persistent per-commit cache, so repeated runs cost only the new
commits.

## The release flow

```bash
halyard plan            # what would change, and why
halyard pr --dry        # forecast the Release PR (no network)
halyard pr              # open or update the Release PR
halyard release --dry   # forecast tags and GitHub releases
halyard release         # create tags, push, create GitHub releases
halyard publish --dry   # forecast registry publishes
halyard publish         # publish to crates.io / npm
```

When the Release PR looks right, review and merge it; `halyard release` then
creates the tags and `halyard publish` ships to the registries. The `release`
step verifies the PR it releases matches the plan it computed — the PR body
embeds the plan, and the plan hash is recomputed before any tag is written.

## In CI: select only the affected packages

```bash
halyard affected --base "$GITHUB_BASE_REF" --ci github-actions
```

`--ci` takes `turbo`, `nx`, `bazel`, `github-actions`, `gitlab`, `azure`,
`teamcity`, `circleci`, `buildkite`, `jenkins`, `bitbucket`, or `drone`, and
prints the selection in that system's own matrix grammar.

## Try it on an existing release-please / changesets repo

```bash
halyard migrate         # survey existing configs, print a draft halyard.toml
halyard migrate --write # land the reviewed draft, create-only
```

The survey reads release-please, changesets, knope, and release-plz configs
and reports what maps, what differs semantically, and what has no equivalent.
The `--write` form is create-only: it never clobbers an existing
`halyard.toml`.

## Going further

- Every command's flags are listed in [CLI commands](reference/cli.md).
- Customization lives in `halyard.toml`; see
  [Configuration](reference/config.md).
- Every stage has a dry-run mode — see
  [Dry-run and preview everywhere](concepts/dry-run.md).
- When something refuses, the error carries a stable code — look it up in
  [Error codes](reference/errors.md#exit-codes).
