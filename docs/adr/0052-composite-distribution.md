# ADR-0052: the §12 composite distribution — the `action/` template, the release pipeline and the single-sourced grammar

- **Status:** accepted
- **Date:** 2026-11-24
- **Milestone:** M5.4's own-artifact slice (§17: "npm provenance, SLSA
  for our binaries, cosign signing, SBOM — `gh attestation verify`
  passes") — the half every as-built note re-homes to this slice: "the
  composite distribution (binary download, checksum + attestation)"
  (§12 note; §7.1 as-built: "the `action/` directory lands with the
  composite distribution"). It is also M4.5's remaining distribution
  half ("Composite Action, binary distribution, attestation verify").
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

The M4.2 remainder (the §11.1 step-3 content commit) landed with the
M7 step-3 session (`commit_via_api` has its caller; `MachineInput`
carries `content_commit`), which made the §12-auto session's recorded
start-point list advance: item 1 done, item 2 is this slice. The
`npm provenance` half of M5.4 shipped earlier (ADR-0033's
`NpmPublishOptions.provenance` argv rows); what the plan records as
absent is halyard's own distribution: no composite action, no release
pipeline, no asset/checksum grammar (`find . -name action.yml` empty;
`.github/workflows/` carries only `ci.yml` + `nightly.yml`; zero
attestation/cosign/SBOM references). The §12 `init --github` scaffold
already references `halyard/action@v1` ("the glue lands with its
milestone", ADR-0029) — the surface exists on the consuming side
before the produced side exists.

The acceptance's live half (an actual `v*` tag push on GitHub, a real
`gh attestation verify` over a downloaded asset) is inherently
off-sandbox — the same live-CI lane discipline as the M5.3 OIDC row:
everything in-tree ships green, the live run is recorded, never
pretended.

## Decision

- **The action is a composite, JS-free, at `action/action.yml`** (§7.1
  as-built's declared path). Its steps are §12's own five: resolve
  OS/arch → download → verify checksum **and** attestation → cache by
  version → exec `halyard action`. The step grammar is VERIFIED
  (`docs/research/own-artifact-attestation.md`): composite steps carry
  `shell` (required with `run`), `if`, `env` and `working-directory`.
- **One grammar, three consumers.** Every piece of the distribution
  that must agree across the action template, the release pipeline and
  the shipped binary is single-sourced in `halyard_action::
  distribution` (the §12 crate — the crate that owns the §12
  vocabulary): the §12 input list with each input's surface (`--mode`,
  `--config`, `--dry` flags; the `GITHUB_TOKEN`/`HALYARD_FORGE_TOKEN`
  and `HALYARD_APP_*` env pairs; the `halyard publish` executor; the
  `-C`-equivalent `working-directory`), the §12 output keys, the pinned
  release targets (Linux x64/arm64 musl, macOS x64/arm64, Windows x64
  msvc) with the `RUNNER_OS`/`RUNNER_ARCH` mapping, the asset-name
  grammar (`halyard-<triple>.tar.gz`, `.zip` for Windows), and the
  `SHA256SUMS` manifest grammar (`<digest>␣␣<file>`, lowercase hex,
  sorted — I3) with parse/render/verify/coverage.
- **The drift gate is an xtask check** (`cargo run -p xtask -- check-release`),
  the `schema-check`/`check-layers` discipline applied to the
  templates: the action's `inputs` block is parsed with a restricted
  reader (the §19 decision-2 discipline — restricted grammar, fail
  closed, no YAML dependency) and must equal `ACTION_INPUTS`; the
  platform table must resolve to the pinned set with a fail-closed
  fall-through; the manifest assembly order must equal the grammar's
  canonical order; the attestation producer/consumer pair
  (`actions/attest` → `gh attestation verify -R elcoosp/halyard`) must
  both be present. Wired into CI beside the structure gates.
- **The checksum verify branches on the runner's checker** (the
  `sha256sum`/`shasum -a 256` split for Linux/macOS; PowerShell's
  `Get-FileHash` for Windows — VERIFIED grammar, SHA256 is its default
  algorithm), all reading the same manifest the pipeline wrote.
- **The `publish` input rides the shipped §13 executor** — the
  composite action execs `halyard publish` (with `--dry` under
  `dry-run`), it does not invent a publish path.
- **`E-ACTION-DISTRIBUTION`** joins the taxonomy (page + index row):
  an asset without a manifest line, a digest mismatch, an unpinned
  runner platform, a manifest the checker would misread, or a template
  block the restricted reader cannot pin — all fail closed.
- **`sha2` joins the runtime graph** for `halyard-action` (the digest
  the manifest grammar verifies; the layer rule widens consciously,
  documented).

## Consequences

- The `init --github` scaffold's `halyard/action@v1` reference now
  names a real artifact; the scaffold text itself stays the ADR-0029
  verbatim string (the `halyard/action` spelling is the architecture's
  shorthand — the unambiguous GitHub form is `elcoosp/halyard/action@v1`,
  which the template's own docs use).
- The version posture is `latest` (the action resolves the newest
  release tag for its cache key and download URL); pinning a specific
  version rides the same grammar (`ReleaseTarget::download_url(tag)`).
- M5.4's remaining rows (cosign signing, the SBOM artifact, the live
  attested-release run) stay open, recorded — this slice lands the
  distribution's checksum+attestation contract, not the whole M5.4 row.
- The per-package `--version`/`--tag` output keys are NOT re-exposed by
  the composite action (a composite `outputs` block is a static
  declaration; the keys are per-release) — they stay readable on the
  step's own outputs; the §12 keys the block does re-expose are pinned
  by the grammar.
