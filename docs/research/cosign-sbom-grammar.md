# Research: halyard's own release signatures + SBOM (VERIFY, M5.4 cosign/SBOM rows)

**Checked:** 2026-11-25 · **Sources:** docs.sigstore.dev, the cosign
CLI reference (`sigstore/cosign`, `doc/cosign_verify-blob.md` /
`doc/cosign_sign-blob.md`), the cosign-installer README
(`sigstore/cosign-installer`), and the cargo-cyclonedx README
(`CycloneDX/cyclonedx-rust-cargo`, `cargo-cyclonedx/README.md`) — each
quoted row below was fetched this session. The two rows honestly
marked are recorded, not pretended (the repo's live-CI lane
discipline).

## Verified facts

- **`cosign sign-blob`** (docs.sigstore.dev "Signing Blobs"):
  `cosign sign-blob <file> --bundle bundle.sigstore.json`; the bundle
  "contains verification metadata, including an artifact's signature,
  certificate and proof of transparency log inclusion"; "You need to
  store the bundle for verification"; bundling is "the recommended way
  of signing a blob". Keyless is "identity-based signing, associating
  an ephemeral signing key with an identity from an OpenID Connect
  provider". On automation: "In situations where automated signing is
  required, such as within CI/CD pipelines, the --yes flag becomes
  essential." The CLI reference pins the flag grammar: `--bundle
  string — write everything required to verify the blob to a FILE`,
  and `--oidc-provider` enumerates `github-actions` among its options.
- **`cosign verify-blob`** (cosign CLI reference, verbatim): example
  `cosign verify-blob --bundle artifact.sigstore.json
  --certificate-identity foo@example.com --certificate-oidc-issuer
  https://accounts.google.com <blob>`. Flag rows: `--bundle string —
  path to bundle FILE`; `--certificate-identity-regexp string — A
  regular expression alternative to --certificate-identity … Either
  --certificate-identity or --certificate-identity-regexp must be set
  for keyless flows.`; `--certificate-oidc-issuer string — The OIDC
  issuer expected in a valid Fulcio certificate, e.g.
  **https://token.actions.githubusercontent.com** or
  https://oauth2.sigstore.dev/auth.` — the GitHub Actions issuer
  appears verbatim in the reference. GitHub-workflow claim flags
  (`--certificate-github-workflow-*`) exist for narrowing further.
- **The cosign installer** (sigstore/cosign-installer README,
  verbatim): `uses: sigstore/cosign-installer@v4.1.4` — the README's
  own example pins that exact version; "The used Cosign version only
  changes when cosign-installer is upgraded"; the README's check step
  is `cosign version`.
- **`cargo cyclonedx`** (cargo-cyclonedx README, verbatim options):
  `--manifest-path <PATH> — Path to Cargo.toml`; `-f, --format
  <FORMAT> — Output BOM format: json, xml`; `--describe … crate:
  Describe the entire crate in a single SBOM file, with Cargo targets
  as subcomponents. (default)`; `--all-features — Activate all
  available features`; `--override-filename <FILENAME> — Custom string
  to use for the output filename`. Default output: "produces a
  `bom.xml` file adjacent to every `Cargo.toml` file that exists in
  the workspace". Install: `cargo install cargo-cyclonedx`. The tool
  "sources data both from `Cargo.lock` and from `cargo metadata`, and
  can "Honor a particular combination of enabled Cargo features,
  matching your build configuration" and "Omit dev-dependencies" — the
  reasons it describes the shipped binary's graph more faithfully than
  a bare `Cargo.lock` scan.
- **GitHub attestations vs SBOMs** (the repo's earlier research note,
  `own-artifact-attestation.md`, verbatim): `gh attestation verify`
  "can validate artifact attestations for binaries and container
  images and validate SBOM attestations" — with the SBOM predicate
  documented for **SPDX** (`--predicate-type
  https://spdx.dev/Document/v2.3`). The CycloneDX SBOM artifact is
  therefore shipped as a signed release artifact (its own cosign
  bundle), not as a `gh` SBOM attestation — the documented predicate
  grammar on this surface is SPDX-only.

## Recorded, not fetched (honest rows)

- **The Fulcio SAN form for GitHub Actions workflow runs** (the exact
  identity the `--certificate-identity-regexp` matches): the sigstore
  docs' regexp examples and the cosign reference's GitHub-workflow
  claim flags anchor the shape (`https://github.com/<repo>/…` for
  workflow-triggered runs), but no page fetched this session spelled
  the full SAN grammar verbatim. The pinned regexp
  (`^https://github.com/elcoosp/halyard/`) is anchored by the
  reference's issuer quote + the regexp alternative's documented
  purpose; the exact match at the live run is the recorded row (the
  first release's verify output names the certificate identity, and a
  mismatch refuses loudly — fail-closed by design).
- **`--override-filename`'s exact extension behavior** (whether the
  custom string is used verbatim or suffixed with the format
  extension): the README's option row (quoted above) documents the
  flag's purpose, not its edge semantics; the flag's documentation
  path on docs.rs exposes the `FilenameOverride` type without prose.
  The pipeline pins the argv row verbatim (`--override-filename
  halyard-cyclonedx.json` + `-f json`); the produced file name at the
  live run is the recorded row — the upload line names the exact file,
  so a divergence fails the release visibly, never silently.

## Design consequences (feeds ADR-0054 / §15.6)

- The M5.4 cosign row splits into a **producer** (`cosign sign-blob
  --yes --bundle <blob>.sigstore.json <blob>` over every release blob,
  riding the release job's `id-token: write`) and a **consumer**
  (`cosign verify-blob --bundle … --certificate-identity-regexp
  '^https://github.com/elcoosp/halyard/' --certificate-oidc-issuer
  https://token.actions.githubusercontent.com <blob>` inside the
  composite action) — both land in-tree; the live keyless run over a
  real `v*` tag is the recorded off-sandbox half.
- The SBOM row lands as a **release artifact** (`halyard-cyclonedx.json`,
  CycloneDX JSON, describing the shipped binary's crate graph) signed
  like every other blob — the SPDX-only predicate row above is why it
  is not a `gh` SBOM attestation.
- The verification identity is repo-scoped by construction: the
  regexp's `elcoosp/halyard` path segment matches
  `distribution::RELEASE_REPO`, one grammar, both consumers.
