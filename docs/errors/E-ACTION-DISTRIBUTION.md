# E-ACTION-DISTRIBUTION

**Code:** `E-ACTION-DISTRIBUTION` · **Area:** §12 composite
distribution (M5.4 own-artifact slice, ADR-0052) · **Raised by:** the
distribution grammar (`halyard-action/src/distribution.rs`: the
checksum manifest, the target resolution, the template reader) and the
template gates (`cargo run -p xtask -- check-release`, the CLI
contract test)

## What it means

A distribution artifact, checksum line, or template shape cannot be
interpreted as the §12 distribution grammar pins it — the release's
checksum/attestation contract, or the composite template's pinned
block, is broken, and proceeding would either ship a release the
checkers cannot verify or run an action the binary never declared.

## Cause

- a checksum manifest line is not `<64 lowercase hex>␣␣<file>` (two
  separating spaces), names a file twice, or is not sorted (I3);
- a downloaded asset's bytes do not digest to its manifest line (a
  tampered or partial download);
- the manifest does not cover one of the pinned release targets (a
  release that ships without a platform's binary);
- a runner platform (`RUNNER_OS`/`RUNNER_ARCH`) with no pinned target
  — the composite action fails closed instead of guessing an asset;
- a template `inputs` block the restricted reader cannot pin (an
  unknown sub-key, a missing entry, a drifted default) or a release
  workflow whose matrix/manifest order drifts from the grammar.

## Fix

For manifests: regenerate `SHA256SUMS` with the release pipeline (the
assembly step's canonical order) — never hand-edit a checksum line.
For platform resolution: run on a runner the pinned target set covers
or extend `RELEASE_TARGETS` consciously (an ADR-backed change — the
set is what the pipeline builds and attests). For templates: run
`cargo run -p xtask -- check-release` first; it names the drifted
location, and the grammar (`halyard_action::distribution`) is the
single source every template must match.

## Command

```console
$ cargo run -p xtask -- check-release
xtask: action/action.yml input `mode`: default = 'auto', the grammar pins 'plan'
```

The gate names the file, the field and the grammar's pinned value.
