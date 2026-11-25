//! The M5.4 signing + SBOM grammar (ADR-0054): the plain-data contract
//! for halyard's own release signatures and SBOM artifact — the
//! producer (`.github/workflows/release.yml`) and the consumer
//! (`action/action.yml`) both pin, single-sourced HERE beside the
//! checksum grammar ([`crate::distribution`]) so the three artifacts
//! cannot drift apart:
//!
//! * the keyless cosign signature ([`sign_command`] /
//!   [`verify_command`] / [`bundle_name`]) — every pinned release asset
//!   and the SBOM ride the same builder, the bundle is written beside
//!   its blob under the `.sigstore.json` naming, and the consumer
//!   verifies against the repo's own workflow identity;
//! * the CycloneDX SBOM ([`sbom_command`] / [`SBOM_FILE`]) — generated
//!   over the shipped binary's crate (`halyard-cli`), one artifact per
//!   release, uploaded under the pinned asset name.
//!
//! Layer note: pure data and pure functions (string builders only), no
//! I/O and no new dependencies — the templates and the gates are the
//! executors, exactly the [`crate::distribution`] discipline.
//!
//! VERIFIED quotes live in `docs/research/cosign-sbom-grammar.md`
//! (the `cosign sign-blob`/`verify-blob` keyless flags, the
//! `--certificate-oidc-issuer` GitHub Actions issuer, the
//! cosign-installer usage, the `cargo cyclonedx` options); the
//! decisions live in `docs/adr/0054-own-release-signing-sbom.md`.

use crate::distribution::RELEASE_TARGETS;

// ---------------------------------------------------------------------------
// The pinned constants
// ---------------------------------------------------------------------------

/// The keyless OIDC issuer the consumer pins (`cosign verify-blob`'s
/// `--certificate-oidc-issuer`): GitHub Actions' own issuer, verbatim
/// from the cosign CLI reference ("e.g.
/// <https://token.actions.githubusercontent.com>").
pub const COSIGN_OIDC_ISSUER: &str = "https://token.actions.githubusercontent.com";

/// The keyless identity the consumer pins (`cosign verify-blob`'s
/// `--certificate-identity-regexp`): the Fulcio certificate's SAN is
/// the triggering workflow's own URI under the release origin, so the
/// regexp accepts exactly the workflow identities of this repository
/// and nothing else (the origin rides
/// `crate::distribution::RELEASE_REPO`).
pub const COSIGN_IDENTITY_REGEXP: &str = "^https://github.com/elcoosp/halyard/";

/// The cosign installer the producers pin (`sigstore/cosign-installer`
/// at the version the installer's README itself pins: "uses:
/// sigstore/cosign-installer@v4.1.4").
pub const COSIGN_INSTALL_ACTION: &str = "sigstore/cosign-installer@v4.1.4";

/// The bundle file suffix (`--bundle` writes "everything required to
/// verify the blob to a FILE"; the sigstore docs' own examples name it
/// `bundle.sigstore.json` / `artifact.sigstore.json` — the blob's name
/// plus this suffix).
pub const COSIGN_BUNDLE_SUFFIX: &str = ".sigstore.json";

/// The `CycloneDX` SBOM release artifact's pinned name (§15.6: "SBOM
/// (CycloneDX)"): the SBOM of the shipped binary's dependency graph,
/// uploaded beside the binaries.
pub const SBOM_FILE: &str = "halyard-cyclonedx.json";

/// The manifest the SBOM describes (the shipped binary's own crate —
/// the release assets are `halyard-cli` builds, so the SBOM describes
/// that crate's resolved graph).
pub const SBOM_MANIFEST_PATH: &str = "crates/halyard-cli/Cargo.toml";

/// The cargo-cyclonedx plugin's install line (the plugin README's own
/// documented install: `cargo install cargo-cyclonedx`), `--locked` so
/// the producer's dependency graph is pinned like every other build.
pub const CYCLONEDX_INSTALL: &str = "cargo install cargo-cyclonedx --locked";

// ---------------------------------------------------------------------------
// The command builders
// ---------------------------------------------------------------------------

/// The signature bundle's name for one blob: the blob's name plus
/// [`COSIGN_BUNDLE_SUFFIX`] — the bundle the producer writes and the
/// consumer downloads, one grammar, two consumers.
#[must_use]
pub fn bundle_name(blob: &str) -> String {
    format!("{blob}{COSIGN_BUNDLE_SUFFIX}")
}

/// The producer's keyless sign line for one blob (the cosign docs'
/// documented grammar: `cosign sign-blob <file> --bundle
/// bundle.sigstore.json`; `--yes` because "in situations where
/// automated signing is required, such as within CI/CD pipelines, the
/// --yes flag becomes essential").
#[must_use]
pub fn sign_command(blob: &str) -> String {
    format!(
        "cosign sign-blob --yes --bundle {} {blob}",
        bundle_name(blob)
    )
}

/// The consumer's keyless verify line for one blob (the cosign CLI
/// reference's documented grammar: `cosign verify-blob --bundle <path>
/// --certificate-identity <identity> --certificate-oidc-issuer <issuer>
/// <blob>`, with the regexp alternative for the workflow identity —
/// "Either --certificate-identity or --certificate-identity-regexp
/// must be set for keyless flows"). The identity regexp is
/// shell-single-quoted (it carries `^` and `/`), the issuer is a plain
/// URI token.
#[must_use]
pub fn verify_command(blob: &str) -> String {
    format!(
        "cosign verify-blob --bundle {} \
         --certificate-identity-regexp '{COSIGN_IDENTITY_REGEXP}' \
         --certificate-oidc-issuer {COSIGN_OIDC_ISSUER} {blob}",
        bundle_name(blob)
    )
}

/// The producer's `CycloneDX` SBOM line (the cargo-cyclonedx README's
/// documented options): the shipped binary's crate (`--manifest-path`),
/// JSON output (`-f json`), the build's own feature set
/// (`--all-features` — the SBOM must describe what the release built),
/// and the pinned artifact name via the documented override
/// (`--override-filename`, "Custom string to use for the output
/// filename").
#[must_use]
pub fn sbom_command() -> String {
    format!(
        "cargo cyclonedx --manifest-path {SBOM_MANIFEST_PATH} -f json --all-features \
         --override-filename {SBOM_FILE}"
    )
}

/// Every blob the release pipeline signs, in the checksum manifest's
/// canonical order (the five pinned target assets) followed by the
/// SBOM artifact — the sign step's list is the grammar's own, never a
/// hand-maintained copy.
#[must_use]
pub fn signed_blobs() -> Vec<String> {
    let mut blobs: Vec<String> = RELEASE_TARGETS
        .iter()
        .map(crate::distribution::ReleaseTarget::asset_name)
        .collect();
    blobs.sort();
    blobs.push(SBOM_FILE.to_owned());
    blobs
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_names_carry_the_sigstore_suffix() {
        let asset = "halyard-x86_64-unknown-linux-musl.tar.gz";
        assert_eq!(
            bundle_name(asset),
            "halyard-x86_64-unknown-linux-musl.tar.gz.sigstore.json"
        );
        assert_eq!(
            bundle_name(SBOM_FILE),
            "halyard-cyclonedx.json.sigstore.json",
            "the SBOM is signed like every other release blob"
        );
    }

    #[test]
    fn sign_lines_follow_the_documented_grammar() {
        let line = sign_command("halyard-aarch64-apple-darwin.tar.gz");
        assert!(
            line.starts_with("cosign sign-blob --yes --bundle "),
            "{line}"
        );
        assert!(
            line.contains("--bundle halyard-aarch64-apple-darwin.tar.gz.sigstore.json"),
            "the bundle is written under the pinned naming: {line}"
        );
        assert!(
            line.ends_with(" halyard-aarch64-apple-darwin.tar.gz"),
            "the blob is the line's last operand: {line}"
        );
    }

    #[test]
    fn verify_lines_pin_the_identity_and_issuer() {
        let line = verify_command("halyard-x86_64-pc-windows-msvc.zip");
        assert!(line.starts_with("cosign verify-blob --bundle "), "{line}");
        assert!(
            line.contains(&format!(
                "--certificate-identity-regexp '{COSIGN_IDENTITY_REGEXP}'"
            )),
            "the identity regexp is shell-quoted and repo-scoped: {line}"
        );
        assert!(
            line.contains(&format!("--certificate-oidc-issuer {COSIGN_OIDC_ISSUER}")),
            "the issuer is the GitHub Actions OIDC issuer, verbatim: {line}"
        );
        assert!(
            line.ends_with(" halyard-x86_64-pc-windows-msvc.zip"),
            "the blob is the line's last operand: {line}"
        );
    }

    #[test]
    fn the_identity_regexp_scopes_the_release_origin() {
        // The regexp accepts the repository's own workflow identities
        // (the SAN form `https://github.com/<repo>/...`) and refuses
        // everything else — the `elcoosp/halyard` path segment is the
        // boundary, matching RELEASE_REPO.
        assert!(COSIGN_IDENTITY_REGEXP.starts_with("^https://github.com/"));
        assert!(
            COSIGN_IDENTITY_REGEXP.contains("elcoosp/halyard"),
            "the regexp scopes the workflow identity to the release origin"
        );
    }

    #[test]
    fn the_sbom_line_pins_crate_format_and_name() {
        let line = sbom_command();
        assert!(
            line.contains(&format!("--manifest-path {SBOM_MANIFEST_PATH}")),
            "the SBOM describes the shipped binary's crate: {line}"
        );
        assert!(line.contains("-f json"), "CycloneDX JSON output: {line}");
        assert!(
            line.contains("--all-features"),
            "the SBOM matches the build's features: {line}"
        );
        assert!(
            line.contains(&format!("--override-filename {SBOM_FILE}")),
            "the artifact lands under the pinned release-asset name: {line}"
        );
    }

    #[test]
    fn the_signed_list_covers_every_pinned_target_and_the_sbom() {
        let blobs = signed_blobs();
        for target in RELEASE_TARGETS {
            let asset = target.asset_name();
            assert!(
                blobs.contains(&asset),
                "the sign list covers the pinned target {asset:?}"
            );
        }
        assert!(
            blobs.contains(&SBOM_FILE.to_owned()),
            "the sign list covers the SBOM artifact"
        );
        let signed: Vec<String> = blobs.iter().map(|blob| sign_command(blob)).collect();
        assert_eq!(signed.len(), blobs.len(), "one sign line per blob");
        // The five target assets keep the manifest's canonical (sorted)
        // order; the SBOM is the list's tail.
        let (targets, tail) = blobs.split_at(blobs.len() - 1);
        let mut sorted = targets.to_vec();
        sorted.sort();
        assert_eq!(
            targets, &sorted,
            "I3: the sign list's target half is sorted"
        );
        assert_eq!(tail, &[SBOM_FILE.to_owned()], "the SBOM signs last");
    }
}
