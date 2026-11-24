//! The §12 composite distribution's grammar, demonstrated end-to-end
//! offline (ADR-0052): the runner-platform resolution, the asset names
//! the release pipeline packages and the composite action downloads,
//! the SHA256SUMS manifest the pipeline assembles, and the verify the
//! composite action's checksum step performs — including the tamper
//! refusal. The templates (`action/action.yml`,
//! `.github/workflows/release.yml`) are pinned to this grammar by
//! `cargo run -p xtask -- check-release`; this example prints the
//! contract a release actually rides.

use halyard_action::distribution::{
    CHECKSUMS_FILE, ChecksumsManifest, RELEASE_REPO, RELEASE_TARGETS, resolve_target,
};

fn main() {
    // (1) The platform resolution the composite action performs from
    // the runner's own labels.
    for runner in [("Linux", "X64"), ("macOS", "ARM64"), ("Windows", "X64")] {
        match resolve_target(runner.0, runner.1) {
            Ok(target) => {
                println!(
                    "runner {}/{} -> {} (asset {})",
                    runner.0,
                    runner.1,
                    target.triple,
                    target.asset_name()
                );
            }
            Err(error) => println!("runner {}/{} -> refused: {error}", runner.0, runner.1),
        }
    }

    // (2) The release: the pinned assets, packaged, digested, assembled
    // into the checksum manifest in the grammar's canonical order.
    let assets: Vec<(String, Vec<u8>)> = RELEASE_TARGETS
        .iter()
        .map(|target| {
            let name = target.asset_name();
            let bytes = Vec::from(target.triple.as_bytes());
            (name, bytes)
        })
        .collect();
    let borrowed: Vec<(&str, &[u8])> = assets
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    let manifest = match ChecksumsManifest::from_files(&borrowed) {
        Ok(manifest) => manifest,
        Err(error) => {
            println!("the release assembly refused: {error}");
            return;
        }
    };
    println!("\n{RELEASE_REPO} release manifest ({CHECKSUMS_FILE}):");
    print!("{}", manifest.render());

    // (3) The composite action's checksum step: the downloaded bytes
    // must digest to the recorded line. A tampered download refuses.
    let Some(first) = assets.first() else {
        println!("no pinned release targets — the grammar is empty");
        return;
    };
    println!("\nverify ok: {}", {
        match manifest.verify(&first.0, &first.1) {
            Ok(()) => format!("{} matches its manifest line", first.0),
            Err(error) => format!("refused: {error}"),
        }
    });
    let tampered = Vec::from(&b"tampered"[..]);
    println!("verify tampered: {}", {
        match manifest.verify(&first.0, &tampered) {
            Ok(()) => String::from("accepted (this would be a bug)"),
            Err(error) => format!("refused: {error}"),
        }
    });

    // (4) The coverage gate: a manifest that skips a pinned target is
    // not a release.
    let all: Vec<&halyard_action::distribution::ReleaseTarget> = RELEASE_TARGETS.iter().collect();
    match manifest.require_coverage(&all) {
        Ok(()) => println!(
            "coverage: all {} pinned targets covered — the release is whole",
            all.len()
        ),
        Err(error) => println!("coverage refused: {error}"),
    }
}
