# ADR-0054: the M5.4 cosign/SBOM rows — keyless release signatures and the CycloneDX SBOM artifact

- **Status:** accepted
- **Date:** 2026-11-25
- **Milestone:** §17 M5.4's remaining rows ("npm provenance, SLSA for
  our binaries, cosign signing, SBOM — `gh attestation verify`
  passes"): ADR-0052 recorded "M5.4's remaining rows (cosign signing,
  the SBOM artifact, the live attested-release run) stay open,
  recorded"; ADR-0033 had already shipped the npm-provenance half, and
  ADR-0052 shipped the distribution's checksum + attestation contract.
  This ADR lands the cosign and SBOM rows — the two halves every
  as-built note re-homes to the release pipeline's own artifacts
  (§15.6: "signed releases (cosign), SBOM (CycloneDX)").
- **Deciders:** autonomous implementation agent (per §0 rule 3)

## Context

The distribution grammar (`halyard_action::distribution`, ADR-0052)
single-sources everything the composite action, the release pipeline
and the shipped binary must agree on: the §12 inputs/outputs, the
pinned targets, the asset names, the checksum manifest, and the
attestation producer/consumer pair. What it does not cover — and what
ADR-0052 explicitly recorded as open — is the release set's **cosign
signatures** and the **SBOM artifact**:

```
$ find . -iname '*cosign*' -o -iname '*sbom*' -o -iname '*.sigstore*'
  (no hits in code, workflows, or templates; only the recorded-open
   mentions in ADR-0052, §15.6 and the research note)
```

The `check-release` gate pins the attestation producer
(`actions/attest`) and consumer (`gh attestation verify`) but pins no
signature and no SBOM: a release could ship with unmodified-looking
checksums yet no cryptographic identity binding the artifacts to this
repository's own workflow. The research note
(`docs/research/cosign-sbom-grammar.md`, VERIFIED this session) pins
the `cosign sign-blob`/`verify-blob` keyless grammar, the GitHub
Actions OIDC issuer (verbatim from the cosign CLI reference), the
pinned cosign-installer version, and the `cargo cyclonedx` options;
its two honest rows (the Fulcio SAN form, the override's extension
behavior) are the recorded live-run halves.

## Decision

- **Keyless cosign signatures over every release blob.** The release
  job installs cosign from the pinned installer
  (`sigstore/cosign-installer@v4.1.4`, the README's own pin) and signs
  the five pinned target assets **plus the SBOM** — the blob list is
  the grammar's `signed_blobs()` (the manifest's canonical order, then
  the SBOM), never a hand-maintained copy. Each blob rides one
  `sign-blob --yes --bundle <blob>.sigstore.json <blob>` line; the
  keyless identity rides the job's `id-token: write` permission (the
  same permission block the attestation producer already needs —
  declared on the release job, which the workflow-level block
  complements).
- **The composite action verifies the signature.** It downloads the
  bundle beside the asset (`.sigstore.json`, the bundle naming the
  docs' own examples use), installs cosign from the same pinned
  installer, and runs `cosign verify-blob --bundle …
  --certificate-identity-regexp '^https://github.com/elcoosp/halyard/'
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
  <asset>` — the regexp's repo path segment is the release origin
  (`distribution::RELEASE_REPO`), so the action trusts exactly this
  repository's workflow identities and nothing else. The verify steps
  ride the download's cache-hit guard (`steps.cache.outputs.cache-hit
  != 'true'`), as the download and unpack steps already do: the cache
  key is the version + target pair, and the bytes under a given key
  were verified by the run that populated it. The attestation-verify
  step gains the same guard — it reads `halyard-dl/`, which does not
  exist on a cache hit; the omission was ADR-0052's, fixed here where
  the signing consumer lives.
- **The SBOM is a signed release artifact.** The release job checks
  out the source, installs cargo-cyclonedx `--locked`, and runs
  `cargo cyclonedx --manifest-path crates/halyard-cli/Cargo.toml -f
  json --all-features --override-filename halyard-cyclonedx.json` —
  the shipped binary's own crate, the build's own feature set, the
  pinned artifact name. The artifact uploads beside the binaries and
  is signed like them. It is **not** a `gh` SBOM attestation: the
  documented SBOM predicate on that surface is SPDX-only
  (`--predicate-type https://spdx.dev/Document/v2.3`, the repo's
  earlier research note) — the CycloneDX artifact rides its own cosign
  bundle instead, and consumers verify it with the same keyless
  grammar.
- **The grammar is single-sourced beside the checksum grammar.**
  `halyard_action::signing` (new module, same crate — the §12 crate
  owns the §12 distribution vocabulary; no new dependencies, the
  builders are pure string functions): the pinned constants
  (`COSIGN_OIDC_ISSUER` verbatim from the CLI reference,
  `COSIGN_IDENTITY_REGEXP`, `COSIGN_INSTALL_ACTION`,
  `COSIGN_BUNDLE_SUFFIX`, `SBOM_FILE`, `SBOM_MANIFEST_PATH`,
  `CYCLONEDX_INSTALL`), the builders (`bundle_name`, `sign_command`,
  `verify_command`, `sbom_command`), and `signed_blobs()` (the blob
  list derived from `distribution::RELEASE_TARGETS`). The
  `check-release` gate gains `check_signing`: the pipeline must carry
  the pinned installer, one sign line per grammar blob (a dropped line
  refuses — a release that ships one unsigned blob is never silently
  partial), the pinned SBOM install + generation argv, the SBOM's
  upload line; the action must carry the installer, the
  `verify-blob --bundle` line and the identity/issuer/suffix tokens.
- **The GitLab CI component stays checksum-only on this lane.** The
  component's attestation verify is already the GitHub lane's recorded
  decline (ADR-0053); the cosign verify joins that recorded decline —
  the component runs on `alpine:3.20` where the sigstore installer
  action is not available, and the lane's verify surface is the
  SHA256SUMS manifest. Recorded, not silently re-scoped.

## Consequences

- A release tag now produces, per asset: the binary, its checksum
  line, its build attestation (`actions/attest`), and its signature
  bundle (`.sigstore.json`) — plus one SBOM artifact covering the
  dependency graph of every shipped binary. Consumers can verify
  checksum (sha256sum), provenance (`gh attestation verify`), and
  identity (`cosign verify-blob`) — three independent guarantees over
  the same bytes.
- The live half stays recorded: the first real `v*` push verifies the
  two honest research rows (the Fulcio SAN the regexp must match; the
  override's produced file name) — both fail loudly and visibly if the
  recorded assumption is wrong, never silently.
- The composite action's cache-hit path now skips the verify steps
  explicitly instead of failing on the missing `halyard-dl/` files;
  the cache key (`halyard-<tag>-<target>`) binds the cached bytes to
  the version they were verified at.
- `cargo install cargo-cyclonedx --locked` adds a compile step to the
  release job (minutes, once per release) — accepted: the SBOM must
  describe the real lockfile, and the alternative (a hand-maintained
  SBOM) is exactly the drift the grammar forbids.
- No new error code: the grammar is pure data (no failure paths), the
  gate refuses with its own message, and the runners' refusals surface
  as step failures — the existing `E-ACTION-DISTRIBUTION` taxonomy
  covers the in-binary surfaces it already covered.
