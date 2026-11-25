//! The M5.4 signing + SBOM grammar (ADR-0054), demonstrated end-to-end
//! offline: the keyless cosign signature the release pipeline produces
//! for every release blob (the pinned target assets plus the
//! `CycloneDX` SBOM) and the verification the composite action performs against
//! this repository's own workflow identity — the producer/consumer
//! pair, over the same single-sourced grammar the checksum contract
//! rides (`halyard_action::distribution`). The templates
//! (`.github/workflows/release.yml`, `action/action.yml`) are pinned to
//! this grammar by `cargo run -p xtask -- check-release`; this example
//! prints the contract a release actually rides.

use halyard_action::distribution::RELEASE_TARGETS;
use halyard_action::signing::{
    COSIGN_IDENTITY_REGEXP, COSIGN_OIDC_ISSUER, CYCLONEDX_INSTALL, SBOM_FILE, bundle_name,
    sbom_command, sign_command, signed_blobs, verify_command,
};

fn main() {
    // (1) The producer's sign half: every release blob — the five
    // pinned target assets in the manifest's canonical order, then the
    // SBOM — rides the same keyless sign line, the bundle written
    // beside the blob.
    println!(
        "producer: the release pipeline signs {} blobs:",
        signed_blobs().len()
    );
    for blob in signed_blobs() {
        println!("  $ {}", sign_command(&blob));
        println!("      -> bundle {}", bundle_name(&blob));
    }

    // (2) The SBOM half: the CycloneDX SBOM of the shipped binary's
    // crate, generated with the pinned argv and uploaded as the
    // release artifact under the pinned name — signed like every
    // other blob.
    println!("\nsbom: the shipped binary's dependency graph, CycloneDX JSON");
    println!("  $ {CYCLONEDX_INSTALL}");
    println!("  $ {}", sbom_command());
    println!("      -> release artifact {SBOM_FILE} (signed above)");

    // (3) The consumer's verify half: the composite action verifies the
    // downloaded asset against its bundle, pinning this repository's
    // own workflow identity under the GitHub Actions OIDC issuer — a
    // tampered or unsigned asset is refused, never tolerated.
    println!("\nconsumer: the composite action verifies each download");
    let first = RELEASE_TARGETS
        .iter()
        .find(|target| target.triple == "x86_64-unknown-linux-musl")
        .unwrap_or_else(|| unreachable!("the pinned set carries the musl x64 target"));
    let asset = first.asset_name();
    println!("  $ {}", verify_command(&asset));
    println!("      identity pinned to {COSIGN_IDENTITY_REGEXP} under issuer {COSIGN_OIDC_ISSUER}");
    println!("\nthe templates pin this grammar: cargo run -p xtask -- check-release");
}
