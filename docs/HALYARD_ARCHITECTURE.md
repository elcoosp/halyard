# HALYARD — Architecture & Multi-Year Master Plan

> **Working name:** `halyard` (binary `halyard`, alias `hly`). Rename freely; nothing below depends on it.
> **Mission:** The fastest, most correct, most pleasant polyglot release engine in the world. Zero-config for Cargo + pnpm + single-package repos, extensible to every ecosystem, with a *Release PR* UX better than release-please and a runtime measured in milliseconds.
> **Audience of this document:** a human tech lead **and** autonomous AI coding agents. Every section is written so an agent can implement it without further questions. Where a decision is open it is marked `DECISION-NEEDED` with a default.

---

## 0. How to read / use this document (Agent Protocol)

1. **Source of truth order:** `§2 Invariants` > `§5 Data model` > `§6–§12 Subsystem specs` > `§17 Milestones` > everything else. If two sections conflict, the earlier wins and you open an ADR.
2. **Task format.** Every milestone task has an ID `M<n>.<k>`. A task is *done* only when its **Acceptance** list passes in CI, including perf gates (§14) and tests (§15).
3. **ADRs.** Any deviation or new decision → `docs/adr/NNNN-title.md` (context, decision, consequences). Never silently deviate.
4. **Never invent facts about external tools/registries.** Items tagged `VERIFY` (e.g., registry APIs, trusted-publishing details, competitor behaviour) must be checked against live docs/real behaviour before coding, and the result recorded in `docs/research/`.
5. **Determinism is a feature.** Same repo state + same config ⇒ byte-identical output. No clock, no randomness, no hash-map iteration order in any output path.
6. **Pure core, effectful shell.** Planning is a pure function (§4). Agents must not add I/O to core crates.
7. **Commit discipline for agents:** small PRs, one task per PR, conventional-commit titles (we dogfood the tool).

---

## 1. Product thesis

### 1.1 What exists, and what we take from each (hypotheses to verify in M0)

| Tool | Steal | Limit we beat |
|---|---|---|
| release-please | Living Release PR; merge ⇒ tag + GitHub Release; manifest mode | Node runtime, API-heavy commit/file inspection, slow on big repos, config-heavy for polyglot |
| release-plz | Cargo-aware cascade, registry-aware diffing, semver-checks integration | Shells out to `cargo metadata`, downloads crate tarballs, rustdoc JSON per crate ⇒ minutes |
| Knope | Hybrid conventional-commits + change files; composable steps | Different perf profile; GitHub API use for PR inspection |
| Cocogitto | Git trailers, templated changelogs, zero API for enrichment | No living Release PR |
| Sampo | Polyglot auto-discovery; action outputs; **resumable partial-failure recovery** | Manual change files as primary input |
| Changesets | Human-written release notes, pre-release mode | JS-only, no commit-derived bumps |

> The competitor weaknesses above come from the seed research and are **unverified**. Milestone M0 turns each into a measured benchmark (`bench/competitors/`). Marketing claims are only made from those numbers.

### 1.2 Differentiators (the "moat")

1. **Single-pass bitset-painted git walk** over a commit-graph, with **trie-guided tree diffing** and a **persistent per-commit cache** ⇒ no-op runs are O(new commits).
2. **Stateless reconciliation** (`manifest version` vs `tags`) instead of fragile trailer/label state ⇒ self-healing, idempotent, resumable.
3. **Plan as a first-class artifact:** `halyard plan --json` is the contract between CLI, Action, PR body, tests, and third-party integrations. Free dry-run, free `explain`.
4. **Zero-config polyglot graph** (Cargo + pnpm/npm, napi-rs/wasm-pack binding links auto-detected).
5. **Minimal forge calls** (≤2 REST calls on the no-op/update path, 0 on exit-early) with ETag/conditional requests.
6. **Smart minimal fetch** (`--shallow-exclude` tag-bounded history, blob-less) so even CI checkout time shrinks.
7. **Open plan spec** (`halyard-plan/v1` JSON schema) so others can build on it.

---

## 2. Invariants (non-negotiable)

| # | Invariant |
|---|---|
| I1 | **Idempotence:** running any command twice on unchanged state is a no-op (no new commits, tags, releases, API writes). |
| I2 | **Resumability:** every multi-step operation (tag, publish, release) can be interrupted at any instruction and re-run to completion without duplicate or corrupted artifacts. |
| I3 | **Determinism:** identical inputs ⇒ byte-identical `Plan`, file edits, PR body, changelog. |
| I4 | **Format preservation:** edits to manifests/lockfiles/changelogs are byte-range patches; untouched bytes never change (comments, ordering, whitespace, EOLs, BOM). |
| I5 | **No unsolicited network:** discovery, planning, applying locally never touch the network. Network is used only by `fetch`, `pr`, `release`, `publish`, and opt-in features. |
| I6 | **No subprocess in the hot path:** planning spawns zero processes. Subprocesses (`cargo`, `pnpm`, `git`) only in explicitly listed *effect* steps with fallbacks. |
| I7 | **Least privilege & safe-by-default:** untrusted text (PR titles, commit messages) is never interpolated into a shell. Overrides (`Release-As`) are honoured only from commits already on the protected base branch. |
| I8 | **Never publish what wasn't planned:** `publish` consumes a verified Plan (hash-checked against HEAD manifests). |
| I9 | **Forge-agnostic core:** GitHub is an adapter, not an assumption. |
| I10 | **`unsafe` is forbidden** except in an allow-listed crate (`halyard-cache` mmap) with Miri/loom coverage. |

---

## 3. System context

```
                       ┌───────────────────────────────────────────────┐
  developer / CI ────► │                 halyard CLI                    │
                       │  plan │ pr │ release │ publish │ check │ ...   │
                       └───────┬───────────────────────────────────────┘
                               │
        ┌──────────────────────┼─────────────────────────────┐
        ▼                      ▼                             ▼
  Snapshot Builder        Planner (PURE)                Effect Executors
  (discover+git walk)  Snapshot+Config → Plan        (apply files, git, forge,
        │                      │                       registries, lockfiles)
        ▼                      ▼                             ▼
  gix / fs / cache       halyard-plan/v1 JSON         GitHub·GitLab·crates.io·npm
```

---

## 4. Core architectural pattern: Functional core / imperative shell

```rust
// halyard-core — NO I/O, no clocks, no env
pub struct Snapshot { pub workspace: Workspace, pub history: HistoryDigest, pub refs: ReleaseRefs }
pub struct Plan     { /* §5.8 */ }

pub fn plan(snapshot: &Snapshot, cfg: &ResolvedConfig) -> Result<Plan, PlanError>;
```

Pipeline (each stage a separate crate/module, each independently testable and benchmarkable):

1. **discover** → `Workspace` (packages, edges, groups)
2. **history** → `HistoryDigest` (per-package list of relevant, parsed commits since base)
3. **plan** → `Plan` (versions, reasons, edits, changelog entries, tags)
4. **render** → `EditSet` (byte patches), PR body, changelog text
5. **apply** → effects (write files, commit, push, PR, tag, release, publish)

`Plan` is serializable, hashable (`blake3` over canonical JSON), and embedded in the Release PR body so later runs can verify and `release`/`publish` can trust it (I8).

---

## 5. Data model (authoritative)

### 5.1 Identity & interning

```rust
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct PkgId(u32);                 // dense index into Workspace.packages
pub struct Interner { /* arena-backed, returns &'static-like Sym(u32) */ }
```
All names/paths are interned once at discovery. Hot loops use `PkgId`/`Sym`, never `String`.

### 5.2 Ecosystem

```rust
#[non_exhaustive]
pub enum Ecosystem { Cargo, Npm /* npm|pnpm|yarn|bun share manifest */, Custom(Sym) }
```
Ecosystems implement the `EcosystemAdapter` trait (§7.3). Built-ins: `cargo`, `npm`. Later (Year 2+): `pypi`, `go`, `maven`, `nuget`, `dart`, `gem`, `docker`, `generic`.

### 5.3 Package

```rust
pub struct Package {
    pub id: PkgId,
    pub name: Sym,
    pub ecosystem: Ecosystem,
    pub root: RelPath,                 // directory, forward-slash, relative to repo root
    pub manifest: RelPath,             // e.g. crates/a/Cargo.toml
    pub version: Version,              // current version in manifest (resolved through inheritance)
    pub version_source: VersionSource, // Local | WorkspaceInherited{ file, key_path } | Dynamic(unsupported)
    pub publishable: bool,             // publish=false / private:true / config
    pub deps: Vec<DepEdge>,            // OUT edges to workspace members only
    pub tag_template: TagTemplate,
    pub changelog: Option<RelPath>,
    pub include: PathFilter,           // which files count as "changes to this package"
    pub group: GroupId,
}

pub struct DepEdge {
    pub to: PkgId,
    pub kind: DepKind,                 // Normal | Build | Dev | Peer | Optional
    pub req: ReqSpec,                  // VersionReq | WorkspaceProtocol(Star|Caret|Tilde|Exact) | Catalog | PathOnly
    pub site: ManifestSite,            // byte span(s) where the requirement is written
}
```

### 5.4 Groups (versioning policy units)

```rust
pub enum GroupPolicy { Independent, Fixed /* same version */, Linked /* same bump level */ }
```
A `Group` also carries the tag template (e.g. `v{version}` for single-package/fixed, `{name}-v{version}` default for independent).

### 5.5 Version & bump

```rust
pub enum Bump { None, Patch, Minor, Major }          // totally ordered
pub struct VersionResolution { pub from: Version, pub to: Version, pub bump: Bump, pub reason: ReasonChain }
```
`Version` wraps `semver::Version` plus ecosystem-specific rendering (PEP 440 later). Strategy trait `VersionStrategy` (`semver`, `calver`, `simple`) — only `semver` ships in v1.

### 5.6 Reason chain (powers `explain`)

```rust
pub enum Reason {
    Commit { oid: Oid, kind: CommitKind, breaking: bool },
    ReleaseAs { oid: Oid, version: Version },
    Changeset { file: RelPath },
    Cascade { via: PkgId, edge: DepKind, why: CascadeWhy }, // RequirementUnsatisfied | PolicyAlways | LockstepGroup
    InitialRelease,
}
pub struct ReasonChain(Vec<Reason>);
```

### 5.7 History digest

```rust
pub struct RelevantCommit {
    pub oid: Oid, pub subject: BString, pub body_range: Range<u32>,
    pub parsed: ConventionalCommit,    // type, scope, breaking, description, footers
    pub pr: Option<u64>, pub author: AuthorRef, pub trailers: SmallVec<[Trailer; 4]>,
    pub touched: PkgBitset,
}
pub struct HistoryDigest { pub per_pkg: Vec<Vec<CommitIdx>>, pub commits: Vec<RelevantCommit>, pub bases: Vec<Option<BaseRef>> }
```
`PkgBitset` = `bitvec`/custom `SmallBitset` (inline up to 128 pkgs, heap beyond). **Do not use a fixed `u64`** (the seed design caps at 64 packages).

### 5.8 Plan (the public contract — `halyard-plan/v1`)

```jsonc
{
  "schema": "halyard-plan/v1",
  "base_branch": "main",
  "head": "9f2c…",                       // commit this plan was computed from
  "plan_hash": "blake3:…",
  "releases": [
    { "package": "core", "ecosystem": "cargo", "from": "0.4.1", "to": "0.5.0", "bump": "minor",
      "tag": "core-v0.5.0",
      "reasons": [ { "type": "commit", "oid": "ab12…", "kind": "feat", "breaking": false } ],
      "changelog": { "file": "crates/core/CHANGELOG.md", "entry_md": "…" },
      "publish": { "registry": "crates.io", "enabled": true } }
  ],
  "edits": [ { "file": "crates/core/Cargo.toml", "patches": [ { "start": 123, "end": 128, "text": "0.5.0" } ] } ],
  "order": ["core", "cli"],              // topological publish order
  "skipped": [ { "package": "docs", "reason": "no_changes" } ]
}
```
Schema published via `halyard schema plan` and checked in CI with `schemars`.

---

## 6. Discovery subsystem (`halyard-discover`)

### 6.1 Topology detection (zero-config)

Order, stop-and-merge semantics:

1. Find repo root = nearest ancestor with `.git` (use `gix::discover`, no subprocess).
2. Read **only if present** (single `read` each, parallel when > 1):
   - `Cargo.toml` → `[workspace] members/exclude/default-members`, `[workspace.package]`, `[workspace.dependencies]`, or single `[package]`.
   - `pnpm-workspace.yaml` → `packages` globs (negations supported) and `catalog(s)`.
   - `package.json` → `workspaces` (npm/yarn/bun) or single package.
   - `halyard.toml` / `.config/halyard.toml` (overrides).
3. Merge into one `Workspace`. If neither a workspace nor multiple packages exist → **single-package mode**: tag `v{version}`, changelog at root.

### 6.2 Performance requirements
- **Never** run `cargo metadata`. Parse manifests directly.
- Glob expansion: build a single `globset` over member patterns and walk with `ignore::WalkBuilder` **parallel**, pruning at `target/`, `node_modules/`, `.git/`, and any path excluded by `.gitignore` *only when walking the filesystem*. For committed-tree discovery (CI), prefer enumerating the **git tree at HEAD** (no FS walk at all) — see §8.6.
- Parse `Cargo.toml` with `toml_edit` (spans enabled) in a `rayon` scope only when package count > 16; otherwise sequential (thread startup ≈ cost of parse).
- Memory budget: manifests are read into one arena `Bytes` pool; edit sites are `(file_idx, Range<u32>)`.

### 6.3 Cargo specifics (must all be tested)
- `version.workspace = true` → resolve through `[workspace.package].version`; edit goes to the **workspace** manifest, once, and all inheriting packages are one lockstep unit automatically.
- Dependency requirement forms: `dep = "1.2"`, `{ version, path }`, `{ workspace = true }` → edge resolved via `[workspace.dependencies]` whose requirement site is the edit location.
- Renames (`package = "real-name"`), `[target.'cfg(..)'.dependencies]`, `[build-dependencies]`, `[dev-dependencies]` (dev edges don't force cascades by default, configurable).
- Optional deps / `dep:` features, `[patch]`/`[replace]` ignored with a warning.
- `publish = false` or `publish = ["other-registry"]`, `package.metadata.halyard` overrides.
- Cargo.lock v3/v4 (§9.2). Virtual workspaces. Nested workspaces (detect, error with guidance). Packages without `version` (treated as unpublishable, warn).

### 6.4 npm/pnpm specifics
- Dependency fields: `dependencies`, `devDependencies`, `peerDependencies`, `optionalDependencies`.
- Protocols: `workspace:*`, `workspace:^`, `workspace:~`, `workspace:1.2.3`, `catalog:` / `catalog:name`, `file:`/`link:`.
- `private: true` ⇒ not publishable (still versioned if configured).
- pnpm `publishConfig`, `files`, provenance flags are *read* (not interpreted) and passed to the publish step.
- Preserve indentation (detect 2/4/tab), trailing newline, key order. Use a **lossless JSON parser** with spans (`biome_json_parser`, or a small hand-written span scanner — `DECISION-NEEDED`, default: hand-written scanner, ~400 LOC, fuzzed).

### 6.5 Polyglot linking (auto, with `explain`)
Link rules (each emits an edge `Cargo → Npm` or `Npm → Cargo`, visible via `halyard graph`):

| Rule | Heuristic |
|---|---|
| napi-rs | `package.json` has `napi` key or `@napi-rs/cli` dep **and** sibling/pointed `Cargo.toml` with `crate-type = ["cdylib"]` |
| wasm-pack / wasm-bindgen | npm package dir contains/consumes `pkg/` output of crate; or crate has `wasm-bindgen` + `package.json` in same dir |
| Explicit | `halyard.toml: [[link]] from = "crate-x" to = "@scope/pkg" mode = "lockstep" \| "cascade"` |
| Path/workspace | npm dep using `workspace:` to a package whose dir also contains a Cargo.toml |

Default mode: **cascade** (bump to the dependent is at least `patch`). `lockstep` ⇒ same version (bindings pattern). Platform sub-packages for napi (`npm/*`) are auto-grouped lockstep with the main package.

### 6.6 Output
`Workspace { packages, topo_order, scc_cycles, edges_in/out CSR, trie }` where the **path trie** (§8.4) is built here.

Cycle policy: dev-dependency cycles are allowed (Cargo allows them); other cycles → `E-GRAPH-CYCLE` with a printed cycle path.

---

## 7. Extensibility architecture

### 7.1 Crate layout (Cargo workspace)

```
halyard/
├─ crates/
│  ├─ halyard-core/        types, Plan, planner (pure) — #![no_std]-friendly where practical
│  ├─ halyard-version/     semver policy (0.x rules, prerelease channels), Bump algebra
│  ├─ halyard-conventional/ hand-written zero-alloc conventional-commit + trailer parser (winnow)
│  ├─ halyard-discover/    topology + manifests → Workspace
│  ├─ halyard-eco-cargo/   EcosystemAdapter for Cargo (toml_edit spans, Cargo.lock patcher)
│  ├─ halyard-eco-npm/     EcosystemAdapter for npm/pnpm (JSON spans, pnpm-lock handling)
│  ├─ halyard-git/         gix integration: refs, painter walk, trie tree-diff, fetch planner
│  ├─ halyard-cache/       persistent mmap per-commit cache (only crate permitted `unsafe`)
│  ├─ halyard-changelog/   minijinja templates, Keep-a-Changelog/default styles, merge of change files
│  ├─ halyard-edit/        EditSet (byte patches), atomic apply, EOL/BOM handling
│  ├─ halyard-forge/       trait Forge + github/ gitlab/ gitea/ adapters (feature-gated)
│  ├─ halyard-registry/    crates.io + npm clients (sparse index polling, existence checks)
│  ├─ halyard-publish/     topological publisher, retry/resume, provenance
│  ├─ halyard-config/      halyard.toml + JSON Schema, layered resolution, env overrides
│  ├─ halyard-cli/         clap, miette, output formats (human/json/github-actions)
│  ├─ halyard-action/      action runtime glue: outputs, summaries, annotations, token minting
│  ├─ halyard-plugin-host/ (Year 2) wasmtime component host
│  ├─ halyard-test-support/ git fixture builder, mock forge, golden helpers
│  └─ xtask/               codegen (schemas), bench orchestration, release of halyard itself
├─ bench/  corpus/  fuzz/  docs/{adr,research,spec}/  action/
```

### 7.2 Dependency rules (enforced by `cargo-deny` + a custom `xtask check-layers`)
`core` depends on nothing but `semver`, `smallvec`, `bstr`, `blake3`, `serde`. `eco-*`, `git`, `forge`, `registry` depend on `core` only. `cli` depends on all. No crate depends on `cli`.

### 7.3 Ecosystem adapter trait

As-built signature set (M3.2; the M1.9-era sketch differed — the edit
subset routes per-file patches rather than a whole-workspace pass, which
keeps the adapters pure and testable):

```rust
pub trait EcosystemAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    /// Identify member manifests given the repo's committed tree or FS. Pure parse, no process spawn.
    fn discover(&self, ctx: &DiscoverCtx) -> Result<Vec<RawPackage>, DiscoverError>;
    /// Re-apply the plan's byte patches for ONE manifest semantically (M3.1/M3.3).
    fn edit_versions(&self, file: &RelPath, content: &[u8], patches: &[PlanPatch]) -> Result<EditOutcome, EditFailure>;
    /// Patch ONE lockfile for the plan's version moves (M3.2, §9.2); decline to
    /// `NeedsNative` when cargo-identical bytes cannot be guaranteed.
    fn patch_lockfile(&self, file: &RelPath, content: &[u8], changes: &[LockfileVersionChange])
        -> Result<LockfileOutcome, LockfileFailure>;
    /// Native refresh fallback (effect step; spawns the ecosystem tool; the
    /// caller verifies only workspace members moved, §9.2 rule 4).
    fn refresh_lockfile_native(&self, ws_root: &Path) -> Result<(), NativeRefreshError>;
    // `publish`/`published` (§13) join the trait with their engines in M5:
    // a trait method without an end-to-end implementation would be a stub.
}
```
Year-2 plugin system (WASM component model, WIT interface mirroring this trait) lets third parties ship adapters without recompiling.

---

## 8. Git engine (`halyard-git`) — the performance heart

### 8.1 Requirements
- Use `gix` with **minimal features** (`default-features = false`, enable only `revision`, `blob-diff`/tree-diff as needed, `max-performance-safe`). No `git2`, no `libgit2`, no `git` subprocess in planning.
- Must handle: shallow clones (detect and emit actionable error or auto-deepen), partial (`--filter=blob:none`) clones, packed-refs, commit-graph (v1 with generation numbers v2), alternates, worktrees, submodules (ignored).

### 8.2 Resolving base refs
For each package, find the latest *release tag* reachable from HEAD:
1. Enumerate refs once (`packed-refs` + loose) under `refs/tags/`.
2. Match names against compiled tag templates using one **Aho-Corasick / prefix-trie** pass (templates like `{name}-v{version}`, `v{version}`, `@scope/name@{version}`).
3. Parse version with semver (ignore invalid), group by package.
4. Candidates sorted descending; choose first whose commit is an ancestor of HEAD (ancestor test via generation numbers; `O(1)` rejection when `gen(tag) > gen(head)`).
5. Annotated tags are peeled once; peel results cached by tag OID.
6. Packages with no tag ⇒ `Base::Initial`.

### 8.3 The painter walk (replaces the seed's "u64 bitmask until zero")

The seed algorithm terminates when each package's tag has been *encountered* while walking from HEAD. That is **incorrect on non-linear history** (merge commits can reach a tag via one parent while commits on another parent are still unreleased). The correct generalization is Git's own `rev-list HEAD ^tagA ^tagB…` computed **once for all packages** with per-commit bitsets:

```text
flags: Map<Oid, PkgBitset>     // bit i set ⇒ commit is reachable from base_i (STALE for pkg i)
heap : MaxHeap<(generation, commit_time, oid)>
push(HEAD, flags=∅)
for each pkg i with base: push(base_i, flags={i})        // paint
ALL = bits of packages that have a base
loop:
  pop c with highest (generation, time)
  if flags[c] ⊇ ALL and heap contains only commits with flags ⊇ ALL: break    // "everybody uninteresting"
  fresh_for = (ALL_PKGS \ flags[c])                       // packages for which c is NEW (includes Initial pkgs)
  if fresh_for ≠ ∅: emit c with fresh_for to the diff stage
  for parent p of c: flags[p] |= flags[c]; push p if first visit or flags grew (re-propagate)
```
Correctness requires processing in **non-increasing generation order**. With a commit-graph (gen numbers) a commit is popped only after all its children ⇒ flags final ⇒ single pass. Without commit-graph: compute generation lazily with memoized DFS over the *visited frontier* or, as fallback, use Git's date-slop heuristic and mark the result `degraded` (warn). `halyard doctor` suggests `git commit-graph write --reachable --changed-paths`.

Complexity: O(commits strictly between HEAD and the *oldest relevant* base + frontier slop). For packages with `Base::Initial` the walk goes to root; mitigated by `initial_release.max_commits` config (default 10 000) and by the cache.

### 8.4 Path → package mapping: trie-guided tree diff

The seed claimed "O(1) per file"; a prefix trie is O(path depth). We do better by **never enumerating files** for unaffected subtrees:

1. Build a trie of package roots (and *global* paths such as root `Cargo.lock`, `Cargo.toml`, `pnpm-workspace.yaml`, configured `shared_paths`).
2. For commit `c` and first parent `p` (see merge policy §8.5) compare `tree(c)` vs `tree(p)`:
   - Walk the two trees **in lockstep, restricted to trie nodes**. If entry OIDs are equal ⇒ prune. If they differ and the path is a trie node ⇒ descend. If the path is inside a package root ⇒ collect **only** differing paths there (for `include`/`exclude` filtering), else mark the package touched.
   - Entries outside any trie node (e.g. `docs/`, `.github/`) are never opened.
3. Memoize `(tree_oid_c, tree_oid_p, trie_node_id) → PkgBitset` in a small LRU (sibling commits often share subtrees).
4. If commit-graph contains **Bloom changed-path filters** (`--changed-paths`), consult them first: a negative for all package-root prefixes ⇒ skip the tree comparison entirely.
5. Per-package `include`/`exclude` globs (e.g. ignore `**/*.md`, `tests/**`, `benches/**`) compile into a `globset` evaluated only on already-changed paths inside that package.

Blobs are **never read** in this stage ⇒ works with `--filter=blob:none` clones.

### 8.5 Merge commits & history shapes (configurable `merge_policy`)
- Default `first_parent_diff = true` for squash/rebase-merge repos: each commit is diffed against its first parent. For true merge commits, the merge commit itself is **not** counted as a change (its constituent commits are walked individually); the merge *message* is parsed only if it is a conventional commit and `count_merge_messages = true`.
- Reverts: `revert:` / `Revert "…"` commits cancel a prior commit within the same window when `This reverts commit <oid>` matches; otherwise treated as `fix`.
- Squash-merge PR numbers: regex-free scan for trailing `(#NNN)` in subject (byte scan from the end).
- Fixup/autosquash commits ignored (`fixup!`, `squash!`).

### 8.6 Reading content at HEAD without a working tree
Discovery in CI reads the manifest blobs from `HEAD`'s tree via `gix` (ODB read, mmap) — uniform with local mode where the working tree is authoritative (dirty tree ⇒ warn, `--allow-dirty`).

### 8.7 Persistent cache (`halyard-cache`)
- Location: `$GIT_DIR/halyard/cache/v1/`. Append-only, mmap'd, versioned header, checksummed records.
- Key: `blake3(commit_oid ‖ layout_hash ‖ config_hash)`; Value: `{ touched: PkgBitset, parsed_commit_header_offsets }`.
- `layout_hash` = hash of trie (package roots + includes). Package layout changes invalidate cleanly.
- Safe to delete; concurrent processes use advisory locking; corrupted records are skipped, never fatal.
- In CI, optionally persisted by `actions/cache` or by the Action's built-in cache restore (§12).

### 8.8 Smart minimal fetch (`halyard fetch`, used by the Action)
Goal: make CI checkout ≈ proportional to unreleased history.
1. `git ls-remote --tags origin` (one cheap round trip) → compute per-package latest tag.
2. `git fetch --filter=blob:none --no-tags origin <base-branch> <selected tag refs>` with `--shallow-exclude=<tag>` for each selected base tag (multiple allowed). `VERIFY` server support/behaviour on GitHub, GitLab, Gitea.
3. Fall back to `--deepen` loops, then to full fetch.
4. Never use `--filter=tree:0` (planning needs trees).

---

## 9. Mutation subsystem: edits, lockfiles, changelogs

### 9.1 `EditSet` (halyard-edit)
```rust
pub struct Patch { pub start: u32, pub end: u32, pub text: SmolStr }
pub struct EditSet { files: IndexMap<RelPath, FilePatches> }   // sorted, non-overlapping, validated
```
- Patches are computed from parser spans, applied right-to-left, verified by **re-parse + semantic check** (the new version is read back and compared to the plan) before writing.
- Write protocol: write temp file in same dir → fsync → rename; Windows-safe (retry on `ERROR_ACCESS_DENIED`). Preserve file mode and line endings (CRLF detected per file).
- All edits across files are staged in memory; either all apply or none (rollback by keeping originals).

### 9.2 Cargo.lock
Direct patch rules (must match what `cargo` would write; **differentially tested** in CI against `cargo update -w --offline`):
1. For each bumped *workspace member without `source`*, rewrite its `[[package]]` `version`.
2. For dependents referencing it as `"name version"` (only emitted by Cargo when the name is ambiguous), rewrite the version string; plain `"name"` entries need no change.
3. Handle lockfile `version = 3|4`, checksum-less path packages, alt registries.
4. If anything is unrecognized or a requirement change would alter resolution (a dependency's *external* req changed), return `NeedsNative` ⇒ run `cargo update --workspace --offline` (effect step) then re-verify only workspace members changed.

### 9.3 pnpm-lock.yaml
- `workspace:` deps are recorded as `link:` with no version ⇒ typically **no lockfile change** needed. Detect precisely which importers record a `specifier` that changed (e.g., `workspace:^` replaced by explicit range configs, `catalog:` entries) and patch only those spans with a **lossless YAML span scanner** (restricted to the `importers:` and `catalogs:` sections).
- Else fall back: `pnpm install --lockfile-only --offline` **only if** at least one npm manifest changed in a way the patcher could not cover. Also support `package-lock.json`, `yarn.lock`, `bun.lock` in Year 2 (native fallback only at first).

### 9.4 Changelog (halyard-changelog)
- Template engine: `minijinja` (compiled once, embedded defaults + user override path).
- Built-in styles: `keep-a-changelog`, `conventional` (grouped by type), `compact`, `github-release-notes`.
- Sections: configurable type→section map (`feat`→"Features", `fix`→"Bug Fixes", `perf`, `refactor` (hidden by default), `docs` (hidden)…), "⚠ BREAKING CHANGES" always first.
- Enrichment **without API calls**: PR numbers from subjects, authors from commit headers, trailers (`Co-authored-by`, `Reviewed-by`, `Release-Note`, `Fixes #n`), compare links computed from tag names + remote URL.
- Insert position: after a stable marker (`<!-- halyard:insert -->`) or after the title block; idempotent (re-run replaces the unreleased section only).
- Optional **change files** `.halyard/changes/*.md` (front-matter: `packages: {core: minor}`), merged into notes, consumed on release.

---

## 10. Versioning & cascade engine (`halyard-version`, planner)

### 10.1 Bump derivation
Precedence (highest wins, per package): `Release-As` footer > change-file explicit bump > conventional commits > cascade > none.

Conventional mapping (configurable): `feat`→minor, `fix`/`perf`→patch, any `!`/`BREAKING CHANGE`→major, others→none (or patch with `bump_unknown = "patch"`).

### 10.2 0.x policy
`pre_major`: `{ breaking → minor, feat → patch }` toggles replicate release-please's `bump-minor-pre-major` / `bump-patch-for-minor-pre-major`. Default **Cargo-compatible**: in `0.y.z`, `y` is the breaking axis.

### 10.3 Prerelease channels
`1.2.0-rc.3`, `-beta.n`, `-alpha.n`; promotion `rc → stable` drops suffix. Branch-bound channels (`next` branch ⇒ `rc`). Counter derived from existing tags — no state file.

### 10.4 Cascade (dependency-aware)
Processed in **reverse topological order** with SCC handling:
- Dependent `D` of bumped `U` is bumped (`patch` default) iff **(a)** `D`'s requirement on `U` no longer admits `U.new` (e.g. `^0.4` and `U` → `0.5.0`), or **(b)** `cascade = "always"` for normal deps of publishable crates (released artifact would otherwise embed a stale internal dep), or **(c)** group policy requires it.
- `workspace:*`/path-only deps don't "mismatch" but `(b)` still applies to the *published* manifest.
- The requirement text is rewritten minimally preserving operator style: `^0.4`→`^0.5`, `=0.4.1`→`=0.5.0`, `"0.4"`→`"0.5"`, `~1.2.3`→`~1.3.0`.
- Fixpoint iteration bounded by `n_packages`; proptest asserts monotonicity and termination.

### 10.5 Optional semver verification (tiers)
| Tier | Mechanism | Cost | Ship |
|---|---|---|---|
| 0 | none (default) | 0 | v1 |
| 1 | opt-in `cargo-semver-checks` subprocess, only for crates with commits touching `src/**` and only if the plan says non-major | minutes | v1.x |
| 2 | **native public-API fingerprint** (Year 3, §17 Y3): syntax-level extraction of `pub` surface via `ra_ap_syntax`, hashed per item; diff vs previous tag *advisory* | ms | Y3 |

---

## 11. Forge & release-state machine

### 11.1 Stateless reconciliation (replaces the seed's trailer-only detection)
On every push to the base branch the **same command** (`halyard release --auto`) runs:

```text
1. Snapshot HEAD.
2. TAG PHASE (always first):
   for each package: manifest_version V, is there tag(template, V)?
        no tag AND V > latest_tag_version  →  package is "pending tag"
   if any pending:  (this is exactly the state after merging a Release PR, or after a partial failure)
        create annotated tags at the commit that introduced V (found by painter walk on manifest blob OID change; fallback HEAD),
        push tags (single `git push`), create forge releases (idempotent: GET-by-tag first),
        publish if enabled (resumable, §13), write outputs; PROPAGATE to step 3 only if nothing pending.
3. PREPARE PHASE (only when nothing pending):
   plan = plan(snapshot, cfg)
   if plan is empty: close stale Release PR if one exists; exit (0 API calls when no PR exists)
   apply EditSet in memory → build tree → compare tree OID with remote release branch tip (`ls-remote`, 0 REST)
   if equal: update PR body only if plan_hash differs; else commit & `push --force-with-lease`
   upsert Release PR (find via branch name; ≤2 REST calls)
```
A `Release-Commit: true` trailer is *emitted* for human readability but **never required**: squash merges can discard it.

### 11.2 Branches & labels
- Release branch: `halyard/release/<base>` (e.g. `halyard/release/main`). Multiple release trains (maintenance branches `1.x`) get independent branches.
- Labels `halyard: pending` → `halyard: tagged` mark PR lifecycle (cosmetic; logic is stateless).
- Concurrency: GitHub `concurrency: group: halyard-${{ github.ref }}`, plus **CAS on the release branch ref** (`--force-with-lease=<expected-oid>`), so two racers can't corrupt it.

### 11.3 Release PR body (stable, parseable)

```markdown
## 🚀 Release plan

| Package | Current | Next | Bump | Why |
|---|---|---|---|---|
| `core` | 0.4.1 | **0.5.0** | minor | `feat(parser): …` (#412) |
| `cli`  | 1.0.3 | **1.0.4** | patch | cascade: `core` 0.4→0.5 |

<details><summary>Changelog preview</summary> … </details>

> Override: add `Release-As: 2.0.0` to a commit on `main`, or edit labels. Run `halyard explain core` locally.

<!-- halyard:plan v1 blake3:… (compressed base64 plan JSON) -->
```
The embedded plan lets `release`/`publish` verify I8 and lets bots/IDEs read the PR.

### 11.4 Forge trait

```rust
pub trait Forge {
    fn find_release_pr(&self, branch: &BranchName) -> Result<Option<Pr>, ForgeError>;   // ETag-conditional
    fn upsert_pr(&self, spec: &PrSpec) -> Result<Pr, ForgeError>;
    fn release_exists(&self, tag: &str) -> Result<bool, ForgeError>;
    fn create_release(&self, spec: &ReleaseSpec) -> Result<Release, ForgeError>;       // idempotent
    fn commit_via_api(&self, spec: &ApiCommit) -> Result<Oid, ForgeError>;              // verified/signed commits
    fn rate_limit(&self) -> RateInfo;
}
```
Adapters: `github` (REST; GraphQL only where it reduces calls), later `gitlab`, `gitea/forgejo`, `bitbucket`.

### 11.5 GitHub specifics
- **Token:** default `GITHUB_TOKEN`; known limitation: PRs/pushes made with it don't trigger other workflows ⇒ support GitHub App auth natively (JWT → installation token minted by the binary; no extra Action step) and PAT.
- **Commit mode:** `git` (push, unsigned with GITHUB_TOKEN) or `api` (tree/commit via Git Data API ⇒ GitHub-signed "Verified" commits). Default `api` when App token present.
- Secondary rate limits: serialize content-creating calls with pacing, honor `Retry-After`, exponential backoff with jitter, per-run call budget with an end-of-run **API usage summary** in the job summary.
- Release notes via `generate_release_notes=false`; we supply our own markdown.

### 11.6 PR title linting — `halyard check`
Squash-merge repos get the bump from the **PR title**. `halyard check --pr-title "…"` runs as a PR workflow job (no checkout needed) and fails with a precise, friendly diagnosis. Optional PR comment showing "if merged, this will release: core minor".

---

## 12. GitHub Action (`halyard-action`)

- **Distribution:** tiny JS-free *composite* action that (1) resolves OS/arch, (2) downloads the pre-built static binary from GitHub Releases (≈5–8 MB), (3) verifies checksum **and** GitHub artifact attestation, (4) caches by version, (5) execs `halyard action`.
  Target: action overhead ≤ 300 ms warm / ≤ 1.5 s cold on `ubuntu-latest`.
- **Inputs:** `mode` (`auto|pr|release|plan|check`), `token`, `app-id`/`app-private-key`, `config`, `publish` (bool), `dry-run`, `working-directory`.
- **Outputs (superset of release-please & Sampo):**
  `released` (bool), `published` (bool), `pr_number`, `pr_url`, `releases_json` (array: name, version, tag, ecosystem, url), `plan_json` (path), plus per-package keys `core--version`, `core--tag`, `paths_released`.
- **Job summary:** markdown table of actions taken, API calls used, timing breakdown.
- **Annotations:** warnings for degraded modes (no commit-graph, shallow clone).
- **Gating example:**
  ```yaml
  - id: hly
    uses: halyard/action@v1
  - if: steps.hly.outputs.released == 'true'
    run: ./build-docker.sh
  ```

---

## 13. Publish engine (`halyard-publish`)

1. **Order:** topological layers from the dependency graph; packages within a layer publish **concurrently** (bounded).
2. **Idempotent resume (I2):** before each publish, `published(pkg, version)` via registry (crates.io **sparse index** `index.crates.io/<prefix>/<name>`, npm `registry.npmjs.org/<name>` conditional GET). Already published ⇒ skip.
3. **Visibility wait:** after `cargo publish`, poll the sparse index (backoff, max wait configurable) before publishing dependents that need to resolve it.
4. **Tooling:** shell out to `cargo publish` / `pnpm publish` (they own packaging semantics: `files`, `publishConfig`, `workspace:` rewriting). Our job: ordering, resume, auth plumbing, provenance.
5. **Auth:** prefer **OIDC trusted publishing** for crates.io and npm where supported (`VERIFY` current availability, claims, and workflow requirements), fallback to tokens. Never log tokens; env-scrub subprocesses.
6. **Provenance:** pass `--provenance` for npm; generate SLSA attestations for our own binaries.
7. **Failure model:** each package outcome ∈ `{Published, AlreadyPublished, Skipped, Failed{retryable}}`. Result persisted in `releases_json` + job summary. A rerun after failure of package *k* continues from *k* (Sampo-style).
8. **Dry run:** `cargo publish --dry-run` / `pnpm publish --dry-run`, plus packaging checks (`cargo package --list` diff vs expectations, optional).

---

## 14. Performance engineering

### 14.1 Honest budgets (measured on a 40-crate workspace, 5 000 commits since last tags, Linux, warm page cache; excludes process spawn jitter)

| Scenario | Budget (p50) | Budget (p99) |
|---|---|---|
| No-op (no new commits, release PR exists), warm cache | **≤ 8 ms** | ≤ 20 ms |
| No-op, cold cache (first run in CI) | ≤ 40 ms | ≤ 90 ms |
| Plan with 200 new commits, 40 crates | ≤ 25 ms | ≤ 60 ms |
| Plan, 1 000-package synthetic monorepo, 50 000 commits since bases | ≤ 400 ms | ≤ 900 ms |
| Apply edits (40 files) incl. fsync+rename | ≤ 15 ms | ≤ 40 ms |
| Peak RSS (all above) | ≤ 64 MB / ≤ 300 MB (1 000 pkgs) | |
| Binary size (musl, stripped) | ≤ 8 MB | |
| Process startup to first useful work | ≤ 2 ms | |

CI **fails** a PR that regresses any p50 budget by > 5 % (§15.6).

### 14.2 Techniques (checklist for agents)
- `[profile.release] lto="fat", codegen-units=1, panic="abort"` (CLI), `opt-level=3`; PGO + BOLT in the release pipeline (`xtask pgo`).
- Global allocator: `mimalloc` (musl default allocator is slow). Arena (`bumpalo`) per pipeline stage; reset between stages.
- No `tokio` in the planning path. HTTP via blocking `ureq` (or `hyper` minimal) in forge/registry crates; parallelism via `rayon`/scoped threads **only** when N > threshold; thread pool lazily created.
- Parse with `winnow`/hand-rolled scanners over `&[u8]` (`bstr`, `memchr`); **no `regex`** in hot paths; no `String` per commit (borrow from ODB mmap'd buffers; copy only emitted fields).
- `gix` configured for object-cache sizing tuned for tree reads; pack delta-cache sized from `--memory-budget`.
- Interned ids; CSR adjacency arrays; `SmallBitset`; sorted `Vec` + binary search instead of `HashMap` for small N.
- Logging via `tracing` with **compile-time disabled** `trace` in release; spans behind `HALYARD_PROFILE=1` emit Chrome-trace JSON.
- Cache (§8.7) makes steady-state O(new commits).
- Avoid `canonicalize`/`current_dir` syscalls in loops; single `openat` per file with `O_NOATIME` where permitted.

### 14.3 Profiling toolchain
`hyperfine` (end-to-end), `divan`/`criterion` (micro), `iai-callgrind` (deterministic instruction counts, used for CI gating since wall-time is noisy), `samply`/`perf` flamegraphs, `dhat` (alloc counts: **zero allocations per commit in the painter walk** is a tracked metric), `strace -c`/`bpftrace` (syscall counts; planning must show **0 `execve`**).

---

## 15. Quality engineering

### 15.1 Test pyramid

| Layer | Tools | Scope |
|---|---|---|
| Unit | `cargo nextest` | version algebra, parsers, patchers |
| Property | `proptest` | cascade monotonic/terminating; EditSet patches commute & roundtrip; conventional parser total (never panics); semver req rewrite preserves operator |
| Snapshot/golden | `insta` | PR body, changelog, plan JSON, error messages |
| Fuzz | `cargo-fuzz` + OSS-Fuzz | TOML/JSON/YAML span scanners, conventional parser, tag-template matcher, cache file reader |
| Differential | custom harness | (a) Cargo.lock patcher vs `cargo update -w --offline`; (b) our bump vs release-plz/release-please on shared fixtures (intentional diffs documented) ; (c) JSON span editor vs `serde_json` roundtrip semantic equality |
| Git fixtures | `halyard-test-support::GitFixture` | declarative DSL builds repos (linear, merge-heavy, octopus, squash, shallow, partial, commit-graph on/off, worktrees) |
| **History replay ("time machine")** | corpus runner | for N real OSS repos (tokio, serde, bevy, swc, rust-analyzer, turborepo, biome…) replay *every historical release*: run `plan` at the commit before each tag and compare to the actual released versions. Reports agreement %, drives policy defaults |
| E2E | mock forge server (`wiremock`/custom) + real git | full Release PR lifecycle incl. partial-failure injection at every step |
| Real forge smoke | dedicated GitHub org/repos, nightly | create PR, merge, tag, release, publish to crates.io *sandbox*/npm `--dry-run`/Verdaccio |
| Mutation | `cargo-mutants` | on `core`, `version`, `edit`, `conventional` (target ≥ 85 % killed) |
| Concurrency | `loom`, `shuttle` | cache, parallel diff |
| Soundness | `miri` | cache crate |
| Perf | §14 | gated |
| Cross-platform | Linux/macOS/Windows × x86_64/aarch64 | CRLF, path separators, long paths, case-insensitive FS |

### 15.2 Failure injection matrix (I2)
For each effect sequence (`write files → commit → push → create PR`; `tag → push tags → release → publish A → publish B`) a test kills the process (or returns error) **after every individual step**, reruns, and asserts the final state equals the uninterrupted run with zero duplicates.

### 15.3 Engineering standards
`#![forbid(unsafe_code)]` (except cache), `clippy::pedantic` + selected `restriction` (`unwrap_used`, `expect_used`, `panic`, `indexing_slicing` in non-test code), `rustfmt`, `cargo-deny`, `cargo-audit`, `cargo-vet` (Year 1 end), MSRV policy (N-3 stable), semver-checks on our own library crates, 100 % documented public items (`missing_docs`), error taxonomy below.

### 15.4 Error taxonomy (stable codes, documented at `docs/errors/E-XXXX.md`)
`E-CFG-*` config, `E-DISC-*` discovery, `E-GRAPH-*` graph, `E-GIT-*` (shallow, no tags, graph degraded), `E-EDIT-*`, `E-LOCK-*`, `E-FORGE-*`, `E-REG-*`, `E-PUB-*`, `E-PLAN-*`. Each has: cause, how to fix, a copy-pasteable command. Rendered with `miette` (source spans into user files). Exit codes: `0` ok / nothing to do, `1` generic failure, `2` usage/config, `3` plan has pending release (for `check --exit-code`), `4` partial publish (retryable), `5` forge rate-limited (retryable).

### 15.5 Security
Threat model in `docs/security.md`: malicious PR title/body, malicious manifests in forks (never execute, only parse), token exfiltration via subprocess env, tag/branch spoofing, dependency-confusion on publish, cache poisoning (cache keyed by content hash + checksummed; never trusted across forks). `Release-As` accepted only from commits reachable from the protected base. No `eval`/shell string building; subprocess via argv arrays. Supply chain for halyard itself: reproducible builds, SLSA L3 provenance, signed releases (cosign), SBOM (CycloneDX), `cargo-vet`.

### 15.6 CI for halyard itself
- PR: fmt, clippy, nextest (3 OS), deny, doc, schema-drift, **perf gate (iai-callgrind instruction counts ±2 %)**, size gate.
- Nightly: fuzz (time-boxed), replay corpus, differential suite, real-forge smoke, competitor benchmarks, Miri, mutation (rotating crate).
- Release: dogfood (halyard releases halyard), cross-build matrix, PGO, attest, publish.

---

## 16. Developer experience

### 16.1 Zero-config first run
```bash
$ halyard plan            # works in any Cargo/pnpm/single-package repo, no config
$ halyard explain core    # reason chain with commit links
$ halyard graph --format mermaid
```
Add the workflow (generated by `halyard init --github` ⇒ one file `.github/workflows/release.yml`):
```yaml
on: { push: { branches: [main] } }
permissions: { contents: write, pull-requests: write, id-token: write }
jobs:
  release:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0, filter: blob:none }
      - uses: halyard/action@v1
        with: { publish: true }
```
(`halyard fetch` replaces the checkout step when minimal fetch is desired.)

### 16.2 CLI surface

| Command | Purpose |
|---|---|
| `plan [--json] [--package P]` | compute and print the plan (pure, offline) |
| `explain <pkg>` | why this version; reason chain; skipped commits and why |
| `apply` | write plan edits to working tree (local release flow) |
| `pr` | prepare/update Release PR |
| `release` | tag + forge release (+ publish with `--publish`) |
| `publish [--resume]` | publish pending packages |
| `check` | lint PR title/commits; `--exit-code` if release pending |
| `status` | table: package, version, last tag, unreleased commits, pending state |
| `graph` | print dependency graph (`text|mermaid|dot|json`) |
| `fetch` | smart minimal fetch (§8.8) |
| `doctor` | env diagnostics: shallow?, commit-graph?, tags?, token scopes?, config lint |
| `migrate` | import from release-please/changesets/knope/release-plz configs, write `halyard.toml` + report of semantic differences |
| `init` | scaffold workflow/config (never required) |
| `schema` | emit JSON Schemas (config, plan) |
| `bench` | run built-in micro/macro benchmarks on the current repo |
| `completions`, `man` | shell integration |

Global flags: `--format human|json|gha`, `--color`, `-C <dir>`, `--config`, `--offline`, `--no-cache`, `--profile`.

### 16.3 DX principles
- Errors teach: every error includes *what we looked at*, *what we expected*, *the exact fix*.
- Output is skimmable (aligned tables, color with `NO_COLOR` support) and machine-readable (`--format json` stable).
- `halyard plan` always shows **what was ignored and why** (`docs:`, `chore:`, outside include paths) behind `-v`.
- First-class monorepo ergonomics: `--package`, `--since`, `--changed` selectors.
- Editor integration (Year 2): JSON Schema for `halyard.toml` published to SchemaStore, VS Code extension showing the live plan, LSP for PR title lint.

### 16.4 Configuration (`halyard.toml`, everything optional)

```toml
# All keys optional. Defaults in parentheses.
[repo]
base_branch = "main"                    # (auto: remote HEAD)
tag_style   = "auto"                    # auto | "{name}-v{version}" | "v{version}"  (single pkg ⇒ v{version})

[versioning]
policy      = "independent"             # independent | fixed | linked
pre_major   = { breaking = "minor", feat = "patch" }   # (cargo-compatible)
cascade     = "when-unsatisfied"        # always | when-unsatisfied | never

[commits]
types.feat  = "minor"
types.fix   = "patch"
types.perf  = "patch"
types.refactor = "none"
merge_policy = "first-parent"
initial_release = { max_commits = 10000 }

[changelog]
style = "keep-a-changelog"              # or path to minijinja template
hidden_types = ["chore", "docs", "ci", "test", "style"]

[publish]
enabled = false                         # opt-in
cargo = { registry = "crates.io", wait_for_index = "90s" }
npm   = { access = "public", provenance = true }

[forge]
kind = "github"                         # github | gitlab | gitea
commit_mode = "auto"                    # git | api

[[package]]                             # per-package override (match by name or path glob)
name = "internal-tool"
release = false
include = ["src/**", "Cargo.toml"]
exclude = ["**/*.md"]

[[link]]                                # explicit polyglot link
from = "my-crate"
to   = "@scope/my-crate-node"
mode = "lockstep"
```
Resolution layers (low→high): built-in defaults < `halyard.toml` < `package.metadata.halyard` / `package.json#halyard` < env `HALYARD_*` < CLI flags. `config_hash` feeds the cache key. Unknown keys are errors with did-you-mean.

---

## 17. Roadmap, milestones & work breakdown

> Time scale assumes a small core team *plus* heavy AI-agent leverage. Dates are relative (`T+months`). Each milestone ends with a **release gate** (tests, perf, docs) and a tagged version. "Agent-ready" tasks are sized ≤ ~1 500 LOC with unambiguous acceptance.

### YEAR 1 — Foundation → Parity → Speed

#### M0 — Research, spec, harness (T+0 → T+1.5 mo) · v0.0
| ID | Task | Acceptance |
|---|---|---|
| M0.1 | Repo scaffold: workspace, CI, lints, deny, ADR template, `xtask` | CI green on 3 OS |
| M0.2 | **Corpus**: pin 12+ real repos (Cargo-only, pnpm-only, polyglot napi/wasm, single-package, huge monorepo) as git submodules or shallow bundles with `corpus/manifest.toml` | `xtask corpus fetch` reproducible by commit SHA |
| M0.3 | **Competitor benchmark harness**: run release-please, release-plz, Knope, Cocogitto, Sampo on corpus; record time, API calls (via mitmproxy/recorded), memory, output versions | `bench/competitors/REPORT.md` generated; every claim in §1.1 marked verified/refuted |
| M0.4 | **Synthetic repo generator** (`xtask gen-repo --packages 1000 --commits 1e6 --shape squash|merge|octopus`) deterministic by seed | generated repo hash stable |
| M0.5 | `GitFixture` DSL in test-support | 20 fixture scenarios incl. shallow/partial |
| M0.6 | Research notes (`docs/research/*`): crates.io/npm trusted publishing `VERIFY`, `--shallow-exclude` support matrix, gix feature/perf spike (tree diff, commit-graph, Bloom), Cargo.lock grammar, pnpm-lock grammar | each note ends with decisions + links to ADRs |
| M0.7 | Freeze `halyard-plan/v1` JSON schema + `halyard.toml` v1 schema | schemas checked in, drift test |

#### M1 — Model, discovery, Cargo & npm adapters, conventional commits (T+1.5 → T+4) · v0.1-alpha
| ID | Task | Acceptance |
|---|---|---|
| M1.1 | `halyard-core` types (§5), interner, `SmallBitset`, `ReasonChain` | 100 % doc, proptest on bitset |
| M1.2 | `halyard-conventional`: spec-compliant parser + trailers + `(#123)` extractor | fuzz 1 h clean; 99.99 % agreement with `conventional-commits-parser` on 1 M real subjects (differential) |
| M1.3 | `halyard-version`: bump algebra, 0.x policy, prerelease channels, requirement rewriter | proptest suite; golden table of 300 cases |
| M1.4 | `halyard-edit`: `EditSet`, atomic apply, CRLF/BOM | roundtrip proptest; Windows CI |
| M1.5 | `eco-cargo` discovery incl. inheritance, renames, targets, workspace deps | 25 fixture workspaces + all corpus repos discovered identical to `cargo metadata` (members, versions, internal edges) — differential test |
| M1.6 | `eco-npm` discovery (npm/yarn/bun workspaces, pnpm, catalogs, protocols) + JSON span editor | corpus parity vs `pnpm m ls --json` ; fuzz span scanner |
| M1.7 | Graph: CSR, topo, SCC, polyglot links (§6.5) | `graph` command; napi/wasm fixtures link correctly |
| M1.8 | Config loader + JSON schema + `did-you-mean` | invalid configs produce snapshot-tested diagnostics |
| M1.9 | Planner v1 (pure): bumps + cascade, using a **naive** history provider (simple rev-walk) to unblock end-to-end | `plan` works on corpus; golden plans |
| M1.10 | CLI skeleton: `plan`, `explain`, `status`, `graph`, `doctor`, `schema` | `--format json` stable |

#### M2 — The fast git engine (T+4 → T+7) · v0.2-alpha
| ID | Task | Acceptance |
|---|---|---|
| M2.1 | Ref enumeration + tag-template matcher + base resolution (§8.2) | matches `git describe`-style ground truth on corpus |
| M2.2 | Painter walk (§8.3) with commit-graph generation numbers + degraded fallback | **differential vs `git rev-list HEAD ^tag…`** on 10 000 random (repo, base-set) samples = identical commit sets |
| M2.3 | Trie-guided tree diff + Bloom filter use + memo LRU (§8.4) | differential vs `git diff-tree --name-only` mapped to packages; ≥ 10× fewer tree entries read than naive diff (counted) |
| M2.4 | Merge policy, revert handling, fixup skipping | fixture matrix |
| M2.5 | Persistent cache (§8.7) incl. Miri/loom | corruption fuzz; kill-during-write test |
| M2.6 | Perf pass: allocation audit, `dhat` = 0 alloc/commit in walk; PGO prototype | §14.1 no-op & plan budgets met on 40-crate corpus |
| M2.7 | Time-machine replay v1 on 5 repos | ≥ 95 % historical version agreement for conventional-commit repos; disagreements triaged in `docs/replay.md` |

#### M3 — Mutations: lockfiles & changelogs (T+7 → T+9.5) · v0.3-alpha
| ID | Task | Acceptance |
|---|---|---|
| M3.1 | Cargo manifest patcher (own version, inherited, dependents' reqs, workspace deps) | semantic read-back equality; no-diff outside edited spans (byte compare) |
| M3.2 | Cargo.lock patcher + native fallback | differential vs `cargo update -w --offline` on 200 mutated fixtures: identical bytes, or `NeedsNative` |
| M3.3 | package.json patcher; `workspace:`/catalog handling | idempotent; style-preserving on 50 real manifests |
| M3.4 | pnpm-lock span patcher + `--lockfile-only --offline` fallback; npm/yarn/bun native-only fallback | pnpm differential on corpus |
| M3.5 | Changelog engine + styles + change-file support | golden snapshots; re-run idempotent |
| M3.6 | `apply` command (local flow) | working tree result == plan edits |

#### M4 — Release PR + tagging + GitHub + Action (T+9.5 → T+13) · v0.4-beta (**public alpha announcement**)
| ID | Task | Acceptance |
|---|---|---|
| M4.1 | Forge trait + GitHub REST adapter, ETag, backoff, App-JWT auth | wiremock suites; ≤ 2 REST calls on update path (asserted) |
| M4.2 | State machine §11.1: tag-phase + prepare-phase, CAS branch push, tree-OID compare | failure-injection matrix (§15.2) green |
| M4.3 | PR body renderer + embedded plan + labels; `check` command | snapshot tests |
| M4.4 | Releases + annotated tags + `commit_mode=api` | verified-commit e2e on real GitHub nightly |
| M4.5 | Composite Action, binary distribution, attestation verify, outputs, summary | e2e in a public sandbox repo; overhead budget met |
| M4.6 | `halyard fetch` smart minimal fetch | CI checkout time reduced ≥ 50 % on large corpus repo vs full clone; fallback paths tested |
| M4.7 | `migrate` from release-please & changesets (read configs, emit report) | 20 real-world configs migrated; diffs documented |
| M4.8 | Docs site v1 (mdBook/Starlight): quickstart, concepts, reference, errors | link-checked in CI |

#### M5 — Publishing & resilience (T+13 → T+16) · v0.5-beta
| ID | Task | Acceptance |
|---|---|---|
| M5.1 | crates.io + npm registry clients (existence, sparse index polling) | contract tests against recorded responses + nightly live checks |
| M5.2 | Publisher: topo layers, bounded concurrency, resume, visibility wait | failure-injection after each package; no duplicates |
| M5.3 | OIDC trusted publishing flows (`VERIFY`) + token fallback + secret scrubbing | e2e on sandbox; log-scan test proves no secrets |
| M5.4 | npm provenance, SLSA for our binaries, cosign signing, SBOM | `gh attestation verify` passes |
| M5.5 | Rate-limit governor, call budget, summary | simulated 403/secondary-limit scenarios pass |
| M5.6 | Prerelease channels + maintenance-branch release trains | fixtures: `next`/`rc`, `1.x` backports |

#### M6 — Hardening → 1.0 (T+16 → T+18) · **v1.0**
| ID | Task | Acceptance |
|---|---|---|
| M6.1 | Replay corpus ≥ 30 repos; fix top disagreements; defaults finalized | ≥ 97 % agreement where repos follow conventional commits |
| M6.2 | 1 000-package synthetic benchmark within budget; RSS gates | §14.1 table all green |
| M6.3 | Security review (external audit), fuzz at scale (OSS-Fuzz onboarded) | audit findings closed |
| M6.4 | Stability promises: CLI JSON, plan schema, exit codes, config semver policy | `docs/stability.md`; schema-compat tests |
| M6.5 | Publish public benchmark site (reproducible, `xtask bench-public`) | third parties can rerun and match ±10 % |
| M6.6 | Dogfood: halyard releases itself for ≥ 3 months | no manual interventions |

### YEAR 2 — Breadth & ecosystem

| Milestone | Scope | Gate |
|---|---|---|
| **M7 (T+18→T+21)** More forges | GitLab (MR flow, CI component), Gitea/Forgejo, Bitbucket; forge conformance test-suite shared across adapters | same e2e matrix passes per forge |
| **M8 (T+21→T+25)** More ecosystems | PyPI (`pyproject`, `uv`, PEP 440 strategy, trusted publishing), Go modules (tag-only, `go.work`, subdir tags), Maven/Gradle, NuGet, Dart/pub, RubyGems, `generic` (regex-edited version files), Docker image tags | each: discover/edit/publish/resume tests + replay on 2 real repos |
| **M9 (T+25→T+28)** Plugin system | WASM component model host (`wasmtime`), WIT API mirroring `EcosystemAdapter` + `ChangelogRenderer` + `Policy`; capability-based sandbox (no ambient FS/network) ; plugin registry index | third-party plugin written in Rust & JS passes conformance |
| **M10 (T+28→T+31)** Scale & monorepo | 10 000 packages, sparse checkout aware, affected-package selectors for CI (`halyard affected --base origin/main`), integration helpers for Turborepo/Nx/Bazel graphs | 10k-pkg plan ≤ 1.5 s |
| **M11 (T+31→T+34)** Bot & IDE | PR comment bot mode (optional GitHub App), VS Code extension + LSP (live plan, title lint), SchemaStore, JetBrains plugin | marketplace-published |
| **M12 (T+34→T+36)** Policy & compliance | Release gates as code (CEL expressions: required checks, CODEOWNERS approval, freeze windows, vuln scan), audit log export, org-wide config inheritance | policy conformance tests |

### YEAR 3 — Moat & platform

| Milestone | Scope | Gate |
|---|---|---|
| **M13 (T+36→T+42)** **Native Rust API differ** | `halyard-api`: syntax-level public-surface extraction (`ra_ap_syntax`, macro-aware for common derive/`pub use` patterns), per-item signature hashing, feature-gated item tracking, semver classification (add/remove/change, trait impl blast radius). Advisory tier first, then enforcing; confidence score; escape hatch to `cargo-semver-checks` for full fidelity | vs cargo-semver-checks on top-500 crates' real release pairs: ≥ 95 % recall on breaking changes, ≥ 90 % precision, ≥ 50× faster |
| **M14 (T+42→T+46)** Embeddable library & bindings | Stabilize `halyard-core`/`plan` as semver'd library; Node (napi) & Python (PyO3) bindings; WASM build for browser planning (paste repo URL → remote plan via forge API in "no-clone" read-only mode) | docs + examples; bindings published by halyard itself |
| **M15 (T+46→T+50)** Distributed/shared cache & remote mode | Optional cache sync (OCI registry / S3 / GH cache), content-addressed cache compatible with `sccache`-style backends; **no-clone planning** via forge Git protocol v2 `filter=tree:0 + fetch-by-path` experiment | cold-CI no-op ≤ 150 ms incl. fetch (target) |
| **M16 (T+50→T+54)** Release orchestration | Multi-repo release trains (cross-repo cascade), staged rollouts, canary → GA promotion, rollback assist (`yank`/`deprecate` workflows), provenance-gated promotion | demo with 3-repo dependency chain |
| **M17 (T+54→T+60)** Optional AI assist (offline-first, BYO model, never required, never in the critical path) | Draft human-quality release notes/migration guides from plan + diffs; PR title suggestion; breaking-change detector *advisory* feeding §M13; deterministic fallback always available; outputs flagged and reviewable in the PR | evaluation harness with human-rated corpus |

### YEAR 4+ — Standardization & sustainability (optional)
Open spec for `halyard-plan` as vendor-neutral release-intent format; conformance suite; governance (foundation / CNCF-style sandbox), long-term support branches, commercial support/hosted offering (strictly optional, core stays OSS).

---

## 18. Risk register

| Risk | Impact | Mitigation |
|---|---|---|
| `gix` API churn / missing features (Bloom, tree-diff memo) | Perf plan slips | M0 spike; thin `halyard-git` abstraction; ability to contribute upstream; fallback path using plumbing subprocess behind a feature flag **for diagnostics only** |
| Lockfile patcher diverges from Cargo output | Corrupt/noisy lockfiles | Differential CI on every Cargo release (beta/nightly channels), automatic `NeedsNative` fallback on any unrecognized shape |
| Shallow/partial clone variability across CI providers | Wrong plans | `doctor`, degraded-mode warnings, `halyard fetch`, tests per provider |
| Squash-merge repos lacking conventional titles | Zero bumps / confusion | `check` job, `explain` shows "ignored: non-conventional", `bump_unknown` policy, change-files as escape hatch |
| Registry/OIDC behaviour changes | Publish failures | `VERIFY` research notes, contract tests, nightly live probes, token fallback |
| GitHub secondary rate limits | Failed runs | Pacing, call budget, minimal calls by design, resumable |
| Scope explosion (ecosystems/forges) | Quality dilution | Year-1 scope fixed to Cargo+npm+GitHub; adapters gated by conformance suites; plugin system moves long-tail out of core |
| Perf claims unprovable | Credibility | Public reproducible benchmark site, fixed corpus by SHA, methodology doc |
| Single-maintainer bus factor | Project risk | ADR discipline, agent-ready tasks, docs-first, contributor ladder |

---

## 19. Open decisions (`DECISION-NEEDED`, with defaults)

1. JSON editing: hand-written span scanner vs `biome_json_parser`. **Default:** hand-written, fuzzed.
2. YAML (pnpm-lock) editing: restricted span scanner vs full lossless YAML crate. **Default:** restricted scanner + native fallback.
3. Cache format: bespoke append-only mmap vs `redb`/`sled`-like KV. **Default:** bespoke (only crate with `unsafe`).
4. HTTP client: `ureq` vs `reqwest`-blocking vs `hyper`. **Default:** `ureq` + rustls (smallest, no async runtime).
5. Tag/branch naming defaults: `halyard/release/<base>`. **Default:** keep.
6. Default `cascade`: `when-unsatisfied` vs `always`. **Default:** `when-unsatisfied` for libs; `always` for workspaces publishing binaries — determined by M6.1 replay data.
7. License: **Default:** MIT OR Apache-2.0 (Rust ecosystem norm).

---

## 20. Definition of Done (global checklist for every PR by a human or agent)

- [ ] Task ID referenced; acceptance criteria in the milestone table met and demonstrated by tests.
- [ ] No new `unwrap/expect/panic/index` outside tests; no `unsafe` outside `halyard-cache`.
- [ ] Public items documented; error variants have codes + docs page.
- [ ] Deterministic output verified (run twice, byte-compare; run with shuffled FS order).
- [ ] Added/updated: unit + property + (if parser) fuzz target + (if output) snapshot.
- [ ] Perf: relevant `iai-callgrind` benches updated; no regression > 2 % instructions; zero planning-path `execve` (strace test).
- [ ] Idempotence + failure-injection tests for any new effect step.
- [ ] Docs updated (`docs/`), ADR written if a decision was made, schema regenerated.
- [ ] `halyard plan` on `corpus/` shows no unexplained diffs vs golden.

---

## 21. Appendix A — Reference pseudocode

### A.1 Planner (pure)
```text
fn plan(snap, cfg):
  bumps[pkg] = Bump::None; reasons[pkg] = []
  # 1. direct bumps
  for pkg in snap.workspace.packages:
     for c in snap.history.per_pkg[pkg]:
         if c.release_as_for(pkg) → force(pkg, that_version, ReleaseAs)
         else bumps[pkg] = max(bumps[pkg], cfg.map(c.parsed)); push Reason::Commit
     for cf in snap.change_files targeting pkg: bumps[pkg] = max(bumps[pkg], cf.bump)
     if pkg.base == Initial and no explicit: version = cfg.initial_version(pkg)   # e.g. current manifest version unchanged, tag only
  # 2. groups (fixed/linked) normalize
  # 3. cascade, reverse topo with SCC collapse
  changed = true
  while changed:                      # bounded by |V|
     changed = false
     for U in reverse_topo(bumped):
        for D in dependents(U):
            if needs_cascade(D, U, cfg) and bumps[D] < Patch:
                 bumps[D] = Patch; push Reason::Cascade{via:U}; changed = true
  # 4. materialize versions, tags, changelog entries, edit requests
  # 5. canonicalize (sort), hash → Plan
```

### A.2 Requirement rewriter (examples that must be test vectors)
| Old req | `U` new | Result |
|---|---|---|
| `0.4` | 0.5.0 | `0.5` |
| `^0.4.1` | 0.5.0 | `^0.5.0` |
| `=0.4.1` | 0.4.2 | `=0.4.2` |
| `~1.2.3` | 1.3.0 | `~1.3.0` |
| `>=1.0, <2` | 1.4.0 | unchanged (still satisfied) |
| `>=1.0, <2` | 2.0.0 | `>=2.0, <3` (policy: widen upper bound) *or* error `E-EDIT-UNREWRITABLE` — configurable |
| `workspace:^` | any | unchanged (resolved at publish) |

### A.3 Tag template grammar
`{name}` `{version}` `{major}` `{minor}` `{patch}` `{ecosystem}`; literal text otherwise. Matching compiles each template into `(prefix, suffix)` plus a version capture validated by `semver`. Scoped npm names (`@scope/name@1.2.3`) supported with `@` literal. Single-package default `v{version}`; independent default `{name}-v{version}`.

---

## 22. Appendix B — First 10 agent tasks to start (ordered)

1. `M0.1` scaffold (workspace, lints, CI, deny, ADR template).
2. `M0.7` schemas for plan + config (so everything else types against them).
3. `M1.1` core types, interner, `SmallBitset` with proptests.
4. `M1.2` conventional-commit parser + fuzz target.
5. `M1.3` version algebra + requirement rewriter vectors (A.2).
6. `M1.4` `EditSet` atomic apply.
7. `M0.5` `GitFixture` DSL.
8. `M1.5` Cargo discovery + differential test vs `cargo metadata`.
9. `M1.6` npm discovery + JSON span editor.
10. `M1.9` Planner v1 with naive history provider → first end-to-end `halyard plan` demo.

---

*End of document. Treat this as a living spec: every change to an invariant, schema, or budget requires an ADR and a version bump of this file.*
