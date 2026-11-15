# HALYARD

The fastest, most correct, most pleasant polyglot release engine in the
world. Zero-config for Cargo + pnpm + single-package repos, extensible to
every ecosystem, with a *Release PR* UX better than release-please and a
runtime measured in milliseconds.

**Status: M4 forge slice landed through M4.1c — M4.1a Forge seam +
`MemoryForge`, M4.1b GitHub REST adapter (live `pr`, `forge-smoke`),
M4.1c GitHub App-JWT auth (`HALYARD_APP_*`), M4.2a §11.1 Release-PR
state machine, M4.3a §11.3 Release-PR body renderer, M4.2b step 1 (`pr
--dry` forecast over the dry-run `PreviewForge`), M4.3 `check` command
(§11.6 PR-title lint), the §11.1 tag-phase reconciliation
(manifest-vs-tags) with its git-side write/push/pinpoint paths and the
live `release` shell, the §9.4 release-notes pipeline, the M2.6
close-out CLI persistent-cache wiring — and M5.5's §11.5 call governor
(pacing, `Retry-After`, deterministic backoff, per-run budget) wired
into the live forge paths (ADR-0015) — and the §8.8 smart minimal fetch
(`halyard fetch`: probe → per-package selection through the planner's
template rule → one minimal `git fetch`, shallow-deepening bounded by
the previous releases, documented full-fetch fallback; ADR-0016) and the
M5.1 registry existence-probe crate (`halyard-registry`: the `Registry`
seam over the forge transport — crates.io sparse index + npm packument
`published()`, deterministic visibility wait, `doctor --registries`
opt-in probe; ADR-0017) — and the M5.2 publish engine (`halyard-publish`:
verified-plan validation, topological layers, registry resume, the
visibility wait and the injected `PublishEffect` seam; ADR-0018) wired
into the `publish [--dry]` command (the tools' documented dry form, the
real spawn live) — the publish-auth plumbing with secret scrubbing
(M5.3 step 1, ADR-0019) and the custom-registry routing (M5.2
continued, ADR-0021: configured `[registries.<name>]` sparse indexes
join the routing map, npm `publishConfig.registry` routes per package)
— and the §11.6 check-comment mode (`check --pr-title --comment`: the
verdict posted to the PR under review through the forge seam's
marker-based comment upsert, rewrite-in-place idempotent; M4.5 step 1,
ADR-0020) — and the M4.7 step 1 migration report (`halyard migrate`:
release-please/changesets configs surveyed into mapped rows, semantic
differences and a validated draft `halyard.toml`; report-only,
ADR-0022).** The architecture and multi-year master plan lives in
[`docs/HALYARD_ARCHITECTURE.md`](docs/HALYARD_ARCHITECTURE.md); progress is
tracked in [`PROGRESS.md`](PROGRESS.md).

## What exists after M0

- **Workspace scaffold (§7.1/§7.2/§15.3)** — layered crates, pedantic+
  restriction lints, rustfmt/clippy/deny/audit configs, CI + nightly
  workflows (`.github/workflows/`).
- **`halyard-plan/v1` + `halyard.toml` v1 schemas (M0.7)** — frozen JSON
  Schemas generated from the Rust types in `halyard-core` / `halyard-config`;
  drift fails CI (`cargo run -p xtask -- schema-check`).
- **`GitFixture` DSL + 20 scenarios (M0.5)** — deterministic git repository
  builder in `halyard-test-support` (linear, merge, octopus, squash, revert,
  shallow, partial, commit-graph, packed refs, worktrees, unicode, …).
- **Synthetic repo generator (M0.4)** — `xtask gen-repo` (same seed ⇒ same
  OIDs).
- **Pinned corpus (M0.2)** — `corpus/manifest.toml` (15 repos pinned by SHA)
  + `xtask corpus fetch` (verified reproducible).
- **Competitor harness (M0.3)** — `xtask bench-competitors`; recipes contain
  only observed argv, unmeasured claims stay `pending`
  (`bench/competitors/REPORT.md`).

## What exists after the M1 foundation slice

- **Core model (M1.1)** — `halyard-core`: `PkgId`/`Sym`/`Interner`,
  `SmallBitset` (inline ≤ 128 packages, heap spill), `Workspace` /
  `Package` / `DepEdge` / groups, `TagTemplate` (appendix A.3),
  `ReasonChain`, `RelevantCommit` / `HistoryDigest`, canonical JSON +
  `plan_hash` (blake3, ADR-0005); proptest on the bitset, snapshot-pinned
  plan hash.
- **Conventional-commit parser (M1.2)** — `halyard-conventional`:
  spec-compliant (1.0.0) grammar over bytes via `winnow`, scopes with
  escapes, `BREAKING CHANGE`/`BREAKING-CHANGE` footers, git trailers,
  `(#NNN)` PR extraction, `fixup!`/`squash!` detection; golden tests on the
  spec examples, totality proptest, fuzz targets under `fuzz/`.
- **Version algebra (M1.3)** — `halyard-version`: bump application with the
  Cargo-compatible 0.x policy, commit-type mapping, prerelease channels
  (counter from existing tags), and the appendix-A.2 requirement rewriter
  (operator + precision preserving, widen policy, `E-EDIT-UNREWRITABLE`).
- **Edit engine (M1.4)** — `halyard-edit`: `EditSet` byte-range patches,
  atomic apply (temp → fsync → rename, Windows retry), verify-before-write,
  CRLF/BOM preservation (invariant I4), roundtrip proptests.
- **Config loader (M1.8)** — `halyard-config`: strict unknown-key detection
  with did-you-mean, layered resolution (defaults < file < package metadata
  < env `HALYARD_*` < CLI), `config_hash` over canonical JSON; snapshot-
  tested diagnostics.

## What exists after the M4 forge slice

- **Forge seam (M4.1a, ADR-0011)** — `halyard-forge`: the forge-agnostic
  [`Forge`](crates/halyard-forge/src/lib.rs) trait (§11.4), its data
  vocabulary, the `E-FORGE-*` taxonomy and the deterministic
  `MemoryForge` reference adapter with call log + failpoint harness.
- **GitHub REST adapter + App auth (M4.1b/M4.1c, ADR-0013/ADR-0014)** —
  `halyard-forge-github`: the full `Forge` implementation over REST v3
  (ETag conditionals, GET-by-tag-first idempotence, the Git-Data
  `commit_via_api` chain) over the seam's `HttpTransport` (blocking
  `ureq` + rustls), plus the blocking `UreqTransport`; M4.1c adds the
  GitHub App credentials (`AppCredentials`: PKCS#1/PKCS#8 PEM → `ring`
  RS256 signer, JWT → installation-token mint on a conservative
  45-minute TTL, injected `EpochClock` for testable determinism) and the
  shell's `HALYARD_APP_*` env wire-up (`pr` live, the release forge
  half, `forge-smoke` — whose JSON carries the resolved `auth_mode`).
- **Release-PR state machine (M4.2a)** — `halyard_forge::state`:
  `run_release_machine` executes the §11.1 tag phase (GET-by-tag-first
  release creation, PROPAGATE gating) and prepare phase (find → upsert /
  no-op / close) against any `&dyn Forge`, with the §11.5 call budgets
  and the §15.2 failure-injection matrix pinned in tests. The seam gained
  `Forge::close_pr` (§11.1 step 3); the shell's git-side tag/push
  effects landed with ADR-0012 and the REST adapter with ADR-0013.
- **Release-PR body renderer (M4.3a)** — `halyard-changelog::pr_body`:
  the §11.3 stable body (plan table, changelog preview, override footer)
  with the embedded plan (canonical JSON → DEFLATE → base64) and the
  I8 verifier `extract_embedded_plan`; `E-CHLOG-PRBODY` fails closed on
  any tampered body.
- **Dry-run Release-PR forecast (M4.2b step 1)** —
  `halyard_forge::PreviewForge` (the dry-run adapter: every probe sees an
  empty forge, every mutation is a `preview://`-marked forecast, the §11.5
  call log stays observable) + `halyard pr --dry`: plan → §11.3 body →
  the full §11.1 machine pass — the tag phase (fed by the manifest-vs-tags
  reconciliation) first, the prepare phase under the PROPAGATE gate —
  reporting the effects, the call-budget forecast and the body; live mode
  runs the same machine over the M4.1b GitHub REST adapter (below).
- **Tag-phase reconciliation (§11.1 step 2)** —
  `halyard_core::release_state`: the stateless manifest-vs-tags decision
  (Tagged / Pending / Behind) as a pure function over the workspace, the
  enumerated tags (`GitHistory::tags()`) and the planner's own template
  rule and `release = false` skip set — the tag phase and the plan can
  never disagree about a tag's name or a skipped package; `pr --dry`
  forecasts the tag phase it decides.
- **Tag-phase write path + forecast (§11.1 step 2, git side)** —
  `halyard_git::tag`: the git engine's first write surface — the
  `TagWriter` seam, the real `GixTagWriter` (annotated tag objects +
  create-only ref updates, idempotent `AlreadyTagged`, fail-closed
  `E-GIT-TAG-CONFLICT`, `E-GIT-TAG` with docs pages) and the
  `PreviewTagWriter` (probes real, mutations forecast, zero writes) +
  `halyard release --dry`: reconcile → pending → directives →
  forecast, and the **live release shell (git side)**: `halyard release`
  runs the same executor over the real writer and pushes the created
  tags through `halyard_git::push` (the §11.2 effect — one
  `git push --porcelain`, argv arrays, no force, no prompt; ADR-0012);
  the forge half (GET-by-tag-first release creation) runs over the M4.1b
  adapter when credentials allow it and is an honest skip otherwise.
- **PR-title lint (M4.3 `check`, §11.6)** — `halyard_conventional::lint_title`
  (the `TitleBumpRule` seam: grammar in the leaf, policy injected) +
  `halyard check --pr-title` (planner-exact prediction with the §10.2
  pre-major remap per package, did-you-mean on typo'd types, `E-CLI-TITLE`
  diagnosis) and `halyard check --exit-code` (§15.4 exit code 3 when a
  release is pending).
- **CLI persistent-cache wiring (M2.6 close-out, §8.7)** — the planning
  pipeline opens `$GIT_DIR/halyard/cache/v1/` and feeds it to
  `GitHistory::build_with_options` with the trie `layout_hash` and the
  resolved `config_hash` (a layout or policy change invalidates the
  cached attributions cleanly); `--no-cache` really skips it and a cache
  that cannot open degrades to an uncached run with a warning (the cache
  is an optimization, never a correctness surface).

## Layout

```
crates/
  halyard-core           pure core types + plan contract + plan hash (no I/O)
  halyard-config         halyard.toml model + loader + layered resolution +
                         foreign-config migration report (M4.7)
  halyard-conventional   conventional-commit parser (winnow, zero-copy scan)
  halyard-version        bump algebra + 0.x policy + requirement rewriter
  halyard-edit           EditSet byte patches + atomic apply + plan apply
  halyard-discover       topology + manifests → Workspace (§6)
  halyard-eco-cargo      Cargo adapter: discovery, manifest + lockfile patchers
  halyard-eco-npm        npm/pnpm adapter: discovery, JSON edit, pnpm-lock patcher
  halyard-git            gix engine: refs, painter walk, trie tree-diff, tag phase,
                         tag-target pinpoint, tag push, smart minimal fetch (§8, §11.1)
  halyard-cache          persistent mmap per-commit cache (§8.7, only unsafe crate)
  halyard-changelog      changelog styles (§9.4) + §11.3 Release-PR body
  halyard-forge          Forge seam (§11.4) + MemoryForge + PreviewForge + §11.1 machine
  halyard-forge-github   GitHub REST adapter (M4.1b) + App-JWT auth (M4.1c) + ureq transport
  halyard-registry       Registry existence probes (M5.1): crates.io sparse index + npm
                         packument `published()` + deterministic visibility wait (§13)
  halyard-publish        Publish engine (M5.2): plan validation (I8), topo layers, resume,
                         visibility wait, injected PublishEffect (§13, ADR-0018) +
                         publish-auth plumbing (M5.3 step 1, ADR-0019): the PublishAuth
                         seam, the VERIFIED env grammar, E-PUB-AUTH fail-closed, secret
                         scrubbing + custom-registry routing (M5.2 continued, ADR-0021):
                         configured `[registries.<name>]` sparse indexes join the probe map,
                         npm `publishConfig.registry` routes per package (`TargetRouter`)
  halyard-test-support   GitFixture DSL + scenarios + mock-forge re-exports
  xtask                  check-layers, schema-check, schema-check-plan, perf-gate,
                         size-gate, corpus, gen-repo, bench-competitors,
                         discover-diff, git-diff, replay, bench-public, pgo
  halyard-cli            the user shell (plan/apply/check/pr/release/publish/fetch/
                         migrate/explain/status/graph/doctor/schema/completions)
schemas/                 frozen JSON Schemas (M0.7)
corpus/manifest.toml     pinned real-world corpus (M0.2)
bench/                   budgets (§14.1) + competitor harness (M0.3)
fuzz/                    cargo-fuzz targets (conventional parser, tag templates)
docs/                    architecture, ADRs, research notes, error pages
```

## Daily commands

```bash
cargo fmt --all --check
# pedantic + restriction lints live in [workspace.lints]; the CLI
# carries only -D warnings (a CLI -W would override the curated allows)
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test --workspace --benches
cargo run -p xtask -- check-layers     # §7.2 layer rules
cargo run -p xtask -- schema-check     # M0.7 schema drift
cargo run -p xtask -- perf-gate        # §14.1 budgets (populated from M2.6)
cargo run -p xtask -- size-gate        # §14.1 binary size (armed from M1.10)
cargo run -p xtask -- git-diff         # tree-diff attribution vs git ground truth
cargo run -p halyard-test-support --example fixture_tour
cargo run -p halyard-core --example workspace_demo
cargo run -p halyard-core --example pending_releases
cargo run -p halyard-conventional --example parse_commit
cargo run -p halyard-conventional --example lint_title
cargo run -p halyard-version --example bump_algebra
cargo run -p halyard-edit --example apply_patch
cargo run -p halyard-config --example resolve_layers
cargo run -p halyard-config --example migrate_report
cargo run -p halyard-discover --example discover_workspace
cargo run -p halyard-eco-cargo --example parse_manifest
cargo run -p halyard-eco-cargo --example patch_lockfile
cargo run -p halyard-eco-cargo --example resolve_registry
cargo run -p halyard-eco-npm --example scan_package_json
cargo run -p halyard-eco-npm --example resolve_publish_registry
cargo run -p halyard-eco-npm --example patch_pnpm_lockfile
cargo run -p halyard-git --example walk_history
cargo run -p halyard-git --example cached_walk
cargo run -p halyard-git --example tag_phase
cargo run -p halyard-git --example tag_forecast
cargo run -p halyard-git --example tag_push
cargo run -p halyard-git --example tag_target
cargo run -p halyard-git --example fetch_plan
cargo run -p halyard-registry --example probe_registries
cargo run -p halyard-cache --example dump
cargo run -p halyard-cli --example release_pipeline
cargo run -p halyard-changelog --example render
cargo run -p halyard-changelog --example render_pr_body
cargo run -p halyard-changelog --example render_release_notes
cargo run -p halyard-changelog --example render_user_template
cargo run -p halyard-changelog --example render_check_comment
cargo run -p halyard-publish --example publish_plan
cargo run -p halyard-publish --example publish_auth
cargo run -p halyard-forge --example release_lifecycle
cargo run -p halyard-forge --example release_machine
cargo run -p halyard-forge --example pr_preview
cargo run -p halyard-forge --example governed_release
cargo run -p halyard-forge-github --example forge_exchange
cargo run -p halyard-forge-github --example app_exchange
```

Crate layering is enforced (`xtask check-layers`): `halyard-core` depends on
nothing but `semver`/`smallvec`/`bstr`/`blake3`/`serde`/`serde_json`
(ADR-0005) plus the feature-gated schema dep of ADR-0001; the parser and
algebra crates take `core` at most; adapters depend on `core` only; nothing
depends on the CLI.
