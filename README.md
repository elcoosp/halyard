<div align="center">
<img src="https://cdn.jsdelivr.net/gh/elcoosp/halyard@main/docs/logo.png" alt="Halyard Logo" width="300"/>
  <p>
    <strong>A zero-config release engine for Cargo and pnpm monorepos, written in Rust.</strong><br/>
    Halyard reads your git history, computes the next version for every package from conventional commits, and opens a Release PR you review before tagging and publishing. It is built for maintainers of monorepos that span Rust and Node, where release-please, changesets, and release-plz each cover half the problem. Every stage has a dry-run mode, so you can see exactly what will happen before anything is written.
  </p>
  <p>
    <a href="https://github.com/elcoosp/halyard/actions"><img src="https://img.shields.io/github/actions/workflow/status/elcoosp/halyard/ci.yml?style=flat-square&label=build" alt="Build status"/></a>
    <a href="https://github.com/elcoosp/halyard/blob/main/LICENSE"><img src="https://img.shields.io/github/license/elcoosp/halyard?style=flat-square" alt="License"/></a>
    <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/language-Rust-orange?style=flat-square" alt="Written in Rust"/></a>
    <a href="#status"><img src="https://img.shields.io/badge/status-pre--1.0-yellow?style=flat-square" alt="Pre-1.0"/></a>
  </p>
  <p>
    <a href="#quickstart">Quickstart</a> •
    <a href="#features">Features</a> •
    <a href="#usage">Usage</a> •
    <a href="docs/HALYARD_ARCHITECTURE.md">Architecture</a>
  </p>
</div>

```mermaid
flowchart LR
    history[git history] --> plan[halyard plan]
    plan --> pr[halyard pr]
    pr --> review[review & merge]
    review --> release[halyard release]
    release --> publish[halyard publish]
```

## Quickstart

Requires a stable Rust toolchain.

```bash
# install from git (pre-1.0; crates.io release pending)
cargo install --git https://github.com/elcoosp/halyard --package halyard-cli --locked

cd your-monorepo
halyard plan      # works with zero config in standard Cargo + pnpm layouts
halyard init      # optional: scaffold halyard.toml to customize
```

`halyard plan` prints each package's next version, the changelog preview, and the files it would edit. When the plan looks right, `halyard pr` opens the Release PR; after it merges, `halyard release` creates the tags and `halyard publish` ships to the registries.

<details>
<summary>Build from source instead</summary>

```bash
git clone https://github.com/elcoosp/halyard.git
cd halyard
cargo build --release
./target/release/halyard plan
```

</details>

## Features

- **Polyglot by design**: one plan covers Cargo crates and npm/pnpm packages in the same monorepo, with a single version graph and reverse-dependency closure.
- **Deterministic plans**: the plan is a pure function of the git history, the workspace, and `halyard.toml`, hashed with blake3. Same inputs, same `plan_hash` — CI can gate on it.
- **Dry-run everywhere**: `plan`, `apply`, `pr`, `release`, and `publish` all support `--dry`, backed by a preview forge and a preview tag writer that never touch the network or the working tree.
- **Verified Release PRs**: the PR body embeds the plan (canonical JSON, DEFLATE, base64), so `halyard release` verifies the PR it releases matches the plan it computed.
- **Conventional-commit native**: spec-compliant 1.0.0 parser (scopes, breaking-change footers, `(#NNN)` PR extraction, `fixup!`/`squash!` detection), plus optional `.halyard/changes/*.md` change files for squash-merge workflows.
- **Affected-package selection**: `halyard affected` answers "which packages must rebuild and test?" and renders the answer in your CI's own grammar — turbo, nx, bazel, GitHub Actions, GitLab, Azure, TeamCity, CircleCI, Buildkite, Jenkins, Bitbucket, Drone.
- **PyPI/uv workspaces supported (M8 step 1+2)**: the same plan covers `pyproject.toml` PEP 621 projects in uv workspaces — `[tool.uv.sources]` workspace rows resolve like cargo path deps, the PEP 440 strategy plans canonical spellings (`1.2.3rc1` ⇄ `1.2.3-rc.1`) and fails closed on the recognized-but-non-canonical ones (epoch/post/dev/local, implicit segments — warned, unpublishable, never rewritten), `halyard publish` rides `uv publish` (the bare argv; `UV_PUBLISH_TOKEN` through the auth seam), the PyPI JSON API backs the resume/visibility probes, and the root `uv.lock` is a release stage at uv's own documented posture — the file is *"managed by uv and should not be edited manually"*, so the apply declines the bytes and refreshes natively (`uv lock --offline`), verifying that only member `version` rows moved.
- **Maven supported (M8)**: the same plan covers `pom.xml` Maven projects — recursive `<modules>` aggregation with the documented parent-coordinates match, parent inheritance through one shared edit site (the aggregator pom's `<version>`), the semver-canonical version strategy (Maven qualifier forms that parse as semver plan verbatim; two-component and property-reference spellings are warned and never rewritten), the byte-preserving pom edit with a read-back verify, and the honest publish posture — Maven Central publishing carries no argv grammar yet, so rows record the engine's disabled shape (`maven-central`, never spawned) until it lands.
- **Gradle supported (M8)**: the same plan covers `build.gradle` / `build.gradle.kts` Gradle projects (the Maven/Gradle cell's Gradle half) — every build file in the committed tree is a package row riding the Maven ecosystem (both publish to Maven Central), a sibling `settings.gradle` / `settings.gradle.kts` governs its subtree at the VERIFIED Settings grammar (the root named by `rootProject.name`, each `include`-proven member named by its path's last element, implicit parents included; an unincluded build file keeps the standalone row, warned; an ambiguous settings pair declines all provenance), the version is the documented literal `version = '…'` (Groovy) / `version = "…"` (Kotlin) assignment (Gradle's Project API: *"The version of this project. Gradle always uses the `toString()` value of the version. The version defaults to `unspecified`."*; VERIFIED in `docs/research/gradle-grammars.md`), the last assignment in document order wins (the Project setter's ordinary overwrite), only semver-parseable literals are canonical (a double-quoted value carrying `$` is GString/Kotlin-template interpolation — non-literal, warned and never rewritten; a property reference, `gradle.properties` lookup, version-catalog accessor, plugin convention, or closure is recognized but non-canonical), a top-level `dependencies { … }` block's `project(':path')` rows resolve against the include-proven member set into internal path-only edges (a path nothing proves warns and stays external; `[[link]]` remains the forced-cascade surface), the byte-preserving build-file edit with a read-back verify, and the honest publish posture — `gradle` publishing carries no argv grammar yet (the credential surface is `~/.gradle/gradle.properties` + the `publishing` block, no documented env-var grammar), so rows record the engine's disabled shape (`maven-central`, never spawned) until it lands.
- **NuGet supported (M8)**: the same plan covers SDK-style `.csproj` projects — the documented identity chain (`PackageId` → `$(AssemblyName)` → the manifest file stem), the pack-slot version strategy (`PackageVersion` → `Version` per the pack target's documented resolution; NuGet spellings like `1.0.0.0` and the `VersionPrefix`/`VersionSuffix` composite are warned and never rewritten), the path-proven internal edges (`<ProjectReference Include>` naming a member manifest; `<PackageReference>` stays registry-resolved), conditioned values consumed unmodeled (evaluation semantics), the byte-preserving csproj edit with a read-back verify, and the honest publish posture — `dotnet` publishing carries no argv grammar yet, so rows record the engine's disabled shape (`nuget.org`, never spawned) until it lands.
- **Go modules supported (M8)**: the same plan covers `go.mod` modules and `go.work` workspaces — tag-only by the grammar's own shape (the manifest carries no version; the release tag does), `use`-directive members with workspace-proven internal edges, per-package subdir tag templates (`v{version}` at the root, `{dir}/v{version}` elsewhere) through the shared template grammar, and the `/vN` major-version rule enforced at planning (a module path declaring v2 never plans a v1 tag; the refused target is a recorded skip). A Go release carries no publish decision — the tag is the artifact.
- **Dart/pub supported (M8)**: the same plan covers `pubspec.yaml` packages and pub workspaces — the root's `workspace` entries walked recursively (nested workspaces included), members gated on `resolution: workspace`, the documented `packages/*` glob expanded over the committed tree, the three-number semver version strategy (two-component spellings are warned and never rewritten), and two-proven internal edges (a dependency **naming** a workspace member resolves to the local copy regardless of its declared source; a `path:` dependency naming a member manifest is the second proof), with the byte-preserving pubspec edit and the honest publish posture — pub.dev publishing documents no token env var, so rows record the engine's disabled shape (`pub.dev`, never spawned) until an auth grammar lands.
- **RubyGems supported (M8)**: the same plan covers `*.gemspec` gems — every gemspec in the committed tree is a standalone package row (the gem is the package; the manifest is the gemspec file), the Gem::Version version strategy plans only the semver-parseable spellings (`"Versions shall be represented by three non-negative integers, separated by periods (e.g. 3.1.4)."`; two-component and four-segment spellings are warned and never rewritten), the deterministic-specs literal-assignment grammar (a computed or conditionally guarded `version` row is consumed unmodeled, never a slot), name-proven internal edges (a dependency **naming** another discovered gemspec drives the cascade; every RubyGems requirement — `~>` included — rides verbatim), the byte-preserving gemspec edit with a read-back verify, and the honest publish posture — the documented flow pushes a built `.gem` whose credential surface is the local `~/.gem/credentials` file, so rows record the engine's disabled shape (`rubygems.org`, never spawned) until the argv arm lands.
- **`generic` packages supported (M8)**: the escape hatch for repositories no manifest grammar covers — declare the package in `halyard.toml` (`[[generic]]`: a `name`, the version `file`, and a locator `pattern`) and the same plan covers it. The pattern rides the named-capture-group grammar the prior art pins (`(?<version>…)` — *"The regex pattern must include a named capture group called version"*, validated fail-closed at load with `E-CFG-GENERIC`), the capture's byte span is the edit site, only semver-parseable captures plan (every other spelling is warned and never rewritten), and the release carries **no publish decision** — the plan names no registry for the generic ecosystem, so rows version and tag and your own pipeline ships (`publish: None`, the honest shape).
- **Docker image tags supported (M8)**: the same plan covers `Dockerfile` image manifests — every `Dockerfile` in the committed tree is a standalone package row (the image is the package; the manifest is the Dockerfile), the version rides the OCI-documented `org.opencontainers.image.version` annotation (*"version of the packaged software"*; *"version MAY be Semantic versioning-compatible"*) carried by the `LABEL` instruction, only semver-parseable label values plan (every other spelling is warned and never rewritten), a `FROM` row's tag names the **base** image and is never an edit site, Docker's own override rule (*"the most-recently-applied value overrides any previously-set value"*) makes the last version label the edit site (superseded rows warn), and the honest publish posture — building and pushing an image spawns the local daemon, so rows record the engine's disabled shape (`docker.io`, never spawned) until an argv grammar exists.
- **Migration path**: `halyard migrate` surveys release-please, changesets, knope, and release-plz configs, reports semantic differences, and writes a reviewed `halyard.toml` — create-only, so existing configs are never clobbered.
- **Ship a release step, not a setup script**: the composite GitHub Action (`action/action.yml`) resolves the runner platform, downloads the pinned static release binary, verifies the checksum manifest **and** the build attestation **and** the cosign signature (`gh attestation verify` + `cosign verify-blob`, keyless, pinned to halyard's own workflow identity), caches by version, and execs `halyard action` — every piece pinned to one grammar by `cargo run -p xtask -- check-release`. The release pipeline (`/.github/workflows/release.yml`) signs every release asset keylessly and ships a signed CycloneDX SBOM of the shipped binary's dependency graph. GitLab pipelines include the same release step as a CI component (`templates/halyard.yml`), checksum-verified from the same release set.
- **GitHub and GitLab and Forgejo/Gitea and Bitbucket supported**: the Release PR flow runs on GitHub, GitLab (M7 step 1), and Forgejo/Gitea — codeberg.org and gitea.com, self-hosted via `FORGEJO_API_URL` (M7 step 2) — MRs/pulls instead of PRs, the same §11.1 machine, backed by the shared forge conformance suite every adapter passes.

## How it works

Halyard is a layered Rust workspace. `halyard-core` holds the pure types and the plan contract (no I/O); the parser, version algebra, and edit engine sit above it; ecosystem adapters (`halyard-eco-cargo`, `halyard-eco-npm`, `halyard-eco-pypi`, `halyard-eco-gomod`, `halyard-eco-maven`, `halyard-eco-nuget`, `halyard-eco-dart`, `halyard-eco-rubygems`, `halyard-eco-generic`, `halyard-eco-docker`) and the git engine (`halyard-git`) sit above those; the forge, registry, publish, and action layers are driven through narrow seams (`Forge`, `Registry`, `PublishEffect`, `PublishAuth`, `ActionWriter`) so the real network adapters and the preview adapters are interchangeable. The layering is enforced by `cargo run -p xtask -- check-layers`.

The full architecture and the milestone plan live in [`docs/HALYARD_ARCHITECTURE.md`](docs/HALYARD_ARCHITECTURE.md); progress is tracked in [`PROGRESS.md`](PROGRESS.md).

<details>
<summary>Workspace layout</summary>

| Crate                  | Role                                                                  |
| ---------------------- | --------------------------------------------------------------------- |
| `halyard-core`         | pure core types, plan contract, plan hash (no I/O)                    |
| `halyard-config`       | `halyard.toml` loader + layered resolution + foreign-config migration |
| `halyard-conventional` | conventional-commit parser (winnow, zero-copy)                        |
| `halyard-version`      | bump algebra, 0.x policy, requirement rewriter                        |
| `halyard-edit`         | byte-range patches, atomic apply, verify-before-write                 |
| `halyard-discover`     | topology + manifests → workspace                                      |
| `halyard-eco-cargo`    | Cargo adapter: discovery, manifest + lockfile patchers                |
| `halyard-eco-npm`      | npm/pnpm adapter: discovery, JSON edit, pnpm-lock patcher             |
| `halyard-eco-pypi`     | PyPI/uv adapter: PEP 621 discovery, PEP 440 strategy, pyproject edits |
| `halyard-eco-gomod`    | Go adapter: go.mod/go.work discovery, tag-only planning, subdir tags  |
| `halyard-eco-maven`    | Maven/Gradle adapter: pom.xml + build.gradle(.kts) + settings.gradle(.kts) discovery, project edges, edits |
| `halyard-eco-nuget`    | NuGet adapter: SDK-style .csproj discovery, pack-slot versions, edits |
| `halyard-eco-dart`     | Dart/pub adapter: pubspec.yaml discovery, pub workspaces, edits      |
| `halyard-eco-rubygems` | RubyGems adapter: .gemspec discovery, Gem::Version strategy, edits   |
| `halyard-eco-generic`  | `generic` adapter: config-declared packages over regex-edited files  |
| `halyard-eco-docker`   | Docker adapter: Dockerfile discovery, OCI version labels, edits      |
| `halyard-git`          | gix engine: refs, history walk, tag phase, push, smart fetch          |
| `halyard-cache`        | persistent mmap per-commit cache                                      |
| `halyard-changelog`    | changelog styles, Release PR body, change-file grammar                |
| `halyard-forge`        | `Forge` seam + `MemoryForge` + `PreviewForge` + release state machine |
| `halyard-forge-github` | GitHub REST adapter + App-JWT auth                                    |
| `halyard-forge-gitlab` | GitLab REST adapter (MR flow) — M7 step 1                             |
| `halyard-forge-forgejo` | Forgejo/Gitea REST adapter (pull flow) — M7 step 2                   |
| `halyard-registry`     | crates.io sparse index + npm packument + PyPI JSON-API probes         |
| `halyard-publish`      | publish engine: plan validation, topological layers, resume, auth     |
| `halyard-action`       | GitHub Actions outputs over `GITHUB_OUTPUT`/`GITHUB_STEP_SUMMARY`     |
| `halyard-cli`          | the `halyard` shell                                                   |

Plus `halyard-test-support` (fixtures) and `xtask` (workspace tooling).

</details>

## Usage

### Preview, open, release, publish

```bash
halyard plan            # what would change, and why
halyard pr --dry        # forecast the Release PR (no network)
halyard pr              # open or update the Release PR
halyard release --dry   # forecast tags and GitHub releases
halyard release         # create tags, push, create GitHub releases
halyard publish --dry   # forecast registry publishes
halyard publish         # publish to crates.io / npm
```

### In CI: select only the affected packages

```bash
halyard affected --base "$GITHUB_BASE_REF" --ci github-actions
```

`--ci` takes `turbo`, `nx`, `bazel`, `github-actions`, `gitlab`, `azure`, `teamcity`, `circleci`, `buildkite`, `jenkins`, `bitbucket`, or `drone`, and prints the selection in that system's matrix grammar.

### Try it on an existing release-please / changesets repo

```bash
halyard migrate         # survey existing configs, print a draft halyard.toml
halyard migrate --write # land the reviewed draft, create-only
```

### Going further

Every crate ships runnable examples:

```bash
cargo run -p halyard-forge --example release_machine
cargo run -p halyard-forge --example content_commit
cargo run -p halyard-publish --example publish_plan
cargo run -p halyard-cli --example release_pipeline
cargo run -p halyard-eco-pypi --example scan_pyproject
cargo run -p halyard-eco-pypi --example patch_uvlock
cargo run -p halyard-eco-gomod --example scan_gomod
cargo run -p halyard-eco-gomod --example plan_tags
cargo run -p halyard-eco-maven --example scan_pom
cargo run -p halyard-eco-maven --example bump_pom
cargo run -p halyard-eco-maven --example scan_settings
cargo run -p halyard-eco-maven --example gradle_deps
cargo run -p halyard-eco-nuget --example scan_csproj
cargo run -p halyard-eco-nuget --example bump_csproj
cargo run -p halyard-eco-nuget --example publish_argv
cargo run -p halyard-eco-dart --example scan_pubspec
cargo run -p halyard-eco-dart --example bump_pubspec
cargo run -p halyard-eco-rubygems --example scan_gemspec
cargo run -p halyard-eco-rubygems --example bump_gemspec
cargo run -p halyard-eco-generic --example scan_generic
cargo run -p halyard-eco-generic --example bump_generic
cargo run -p halyard-eco-docker --example scan_docker
cargo run -p halyard-eco-docker --example bump_docker
cargo run -p halyard-action --example auto_forecast
cargo run -p halyard-action --example distribution_plan
```

## Compared to alternatives

release-please, release-plz, and changesets are mature tools. If your monorepo is single-ecosystem, one of them is likely the simpler choice. Halyard is for repos that span both Rust and Node: it plans and ships Cargo crates and npm/pnpm packages from the same conventional-commit history, with one dry-run preview, one Release PR, and one publish step. The `migrate` command reads existing release-please, changesets, knope, and release-plz configs and drafts a `halyard.toml`, so you can evaluate it without committing.

## Status

> [!NOTE]
> Halyard is pre-1.0 and under active development. The planning, edit, git, forge, registry, and publish layers are landed and tested; the CLI surface is stabilizing but may change between minor versions. See [`PROGRESS.md`](PROGRESS.md) for the milestone log.

Supported platforms follow the CI matrix: Linux, macOS, and Windows.

## Learn more

- [Architecture and master plan](docs/HALYARD_ARCHITECTURE.md)
- [Milestone progress](PROGRESS.md)
- [Stability contract](docs/stability.md) — what a consumer may hold halyard to across releases (M6.4)
- [Docs site](docs/site/src/SUMMARY.md) — quickstart, concepts, and reference as a book (`mdbook build docs/site`; link-gated by `cargo run -p xtask -- check-docs-site`)
- [Runnable examples](crates/) — each crate has an `examples/` directory
