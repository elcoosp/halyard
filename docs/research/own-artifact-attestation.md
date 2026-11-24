# Research: halyard's own release artifacts — checksums + attestations (VERIFY, M5.4 own-artifact slice)

**Checked:** 2026-11-24 · **Sources:** GitHub Actions docs
(docs.github.com) and Microsoft Learn (fetched live; each quoted row
below was fetched this session). One row honestly marks a fetch that
timed out in this environment.

## Verified facts

- **`gh attestation verify`** (GitHub docs, "Using artifact
  attestations to establish provenance for builds"): "You can validate
  artifact attestations for binaries and container images and validate
  SBOM attestations". Binary form, verbatim:
  `gh attestation verify PATH/TO/YOUR/BUILD/ARTIFACT-BINARY -R ORGANIZATION_NAME/REPOSITORY_NAME`.
  "These commands assume you are in an online environment."
  Container images take `oci://FQDN`; SBOM attestations need
  `--predicate-type https://spdx.dev/Document/v2.3`.
- **Attestation production** (same page): the workflow holds
  `permissions: { id-token: write, contents: read, attestations: write }`
  and a step `uses: actions/attest@v4` with
  `subject-path: 'PATH/TO/ARTIFACT'` — "The value of the
  `subject-path` parameter should be set to the path to the binary you
  want to attest". (The page documents `actions/attest@v4`;
  `attest-build-provenance` is not mentioned on it — halyard's
  pipeline pins the documented one.)
- **Composite action grammar** (GitHub docs, "Metadata syntax
  reference"): composite `runs.steps[*]` support `shell` ("Required if
  `run` is set"), `if` ("You can use the `if` conditional to prevent a
  step from running unless a condition is met"; "you can, optionally,
  omit the `${{ }}` expression syntax", with the `!`-prefix caveat),
  `env` ("Sets a `map` of environment variables for only that step"),
  `working-directory` ("Specifies the working directory where the
  command is run"), `uses`/`with`, `continue-on-error`. The
  composite-action tutorial's example shows the `inputs` grammar
  (`description`/`required`/`default`, consumed as
  `${{ inputs.<id> }}`) and `outputs` mapped from
  `steps.<id>.outputs.<name>` via `value:`.
- **PowerShell `Get-FileHash`** (Microsoft Learn,
  Microsoft.PowerShell.Utility): "By default, the `Get-FileHash`
  cmdlet uses the SHA256 algorithm" — accepted values include SHA256;
  the output object's properties are `Algorithm`, `Hash`, `Path`, and
  the documented example renders `Hash : 3CBCFD…` — **uppercase** hex,
  compared case-insensitively in the documented `-eq` example. This is
  the Windows runner's checksum checker.
- **macOS runners' checker:** macOS does not ship GNU coreutils'
  `sha256sum` in the runner default PATH; the bundled Perl `shasum`
  (`shasum -a 256`) reads the same `<digest>␣␣<file>` check grammar.
  *(Recorded as deployment knowledge, not a fetched quote — the
  action's verify step branches on `RUNNER_OS` for exactly this.)*

## Timed out / not fetched (honest rows)

- **GNU coreutils' `sha256sum` manual** (the `md5sum invocation`
  section documents the shared `-c` check grammar) — both fetch
  attempts timed out in this environment. The grammar halyard's
  distribution pins (`<64 lowercase hex>␣␣<file>`, text mode — two
  separating spaces, no `*` binary marker) is anchored instead by the
  **repo's own writer+reader contract**: the release workflow
  generates `SHA256SUMS` with `sha256sum`, the composite action and
  `ChecksumsManifest` both read/verify that exact grammar, and
  `cargo run -p xtask -- check-release` refuses any divergence. The
  example (`distribution_plan`) demonstrates the round trip.
- **`Expand-Archive`** (the Windows unpack step) is PowerShell's
  archive-module cmdlet; not fetched this session, not quoted.

## GitLab CI components (fetched 2026-11-24, feeds ADR-0053)

- **docs.gitlab.com/ci/components/**: a repo exposes CI components as
  `templates/<name>.yml` ("A top level templates/ directory that
  contains all the component configurations"; "Only the template.yml
  file is used by other projects using the component" for the
  directory form) with a `spec:` header above a `---` separator —
  allowed spec keywords are `inputs`, `description`, `component`
  ("You cannot use spec:include in components"); per-input attributes
  include `default:` and `description:`; inputs interpolate below the
  separator as `$[[ inputs.NAME ]]` (the page's minimal example, verbatim:
  `spec: { inputs: { stage: { default: test } } }` then
  `component-job: { script: echo job 1, stage: $[[ inputs.stage ]] }`).
  Component names "can contain letters, numbers, underscores (_),
  hyphens (-), and periods (.)"; a maximum of 100 components per
  project. The docs also recommend a README documenting the
  components (the repo's root README carries the row).

## Design consequences (feeds ADR-0052 / §12)

- The M5.4 acceptance "`gh attestation verify` passes" splits into an
  **in-repo producer** (`actions/attest@v4`, `id-token: write` +
  `attestations: write`) and a **consumer** (`gh attestation verify
  <asset> -R elcoosp/halyard` inside the composite action) — both land
  in-tree; the live run over a real `v*` tag is the recorded off-sandbox half.
- The checksum check must be runner-portable: `sha256sum -c` (Linux) /
  `shasum -a 256 -c` (macOS) / `Get-FileHash` + uppercase-tolerant
  compare (Windows) — one manifest grammar, three checkers.
- Composite steps' documented `if`/`env`/`working-directory` support is
  what lets the action stay JS-free while skipping the download on a
  cache hit and feeding the token/App env pairs.
