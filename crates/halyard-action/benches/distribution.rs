//! The §12 distribution grammar's cost (ADR-0052): the checksum
//! manifest's build/parse/verify cycle over a full 5-target release
//! must stay free — the composite action's checksum step is the
//! download's epilogue, never its cost driver. The signing + SBOM
//! grammar's sign/verify line rendering (ADR-0054) rides the same
//! cycle: pure string building, rendered by the gate on every
//! `check-release` run.

use std::hint::black_box;

use criterion::Criterion;
use halyard_action::distribution::{ChecksumsManifest, RELEASE_TARGETS};
use halyard_action::signing::{sign_command, signed_blobs, verify_command};

/// A deterministic pseudo-asset: one megabyte of seeded bytes per
/// target (the payload shape the pipeline packages).
fn asset_bytes(seed: u8) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(1024 * 1024);
    let mut state = seed;
    for _ in 0..1024 * 1024 {
        state = state.wrapping_mul(31).wrapping_add(17);
        bytes.push(state);
    }
    bytes
}

fn full_release_assets() -> Vec<(String, Vec<u8>)> {
    RELEASE_TARGETS
        .iter()
        .enumerate()
        .map(|(idx, target)| {
            let name = target.asset_name();
            let bytes = asset_bytes(u8::try_from(idx).unwrap_or(u8::MAX));
            (name, bytes)
        })
        .collect()
}

fn main() {
    let mut criterion: Criterion = Criterion::default().configure_from_args();
    bench(&mut criterion);
    criterion.final_summary();
}

/// The grammar's cycle over a full 5-target release: the pipeline's
/// manifest build, the parse the gates ride, the verify the composite
/// action's checksum step performs on the downloaded asset. A fixture
/// failure (impossible for the pinned set) ends the bench, it never
/// panics mid-run.
fn bench(group: &mut Criterion) {
    let assets = full_release_assets();
    let borrowed: Vec<(&str, &[u8])> = assets
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();

    let Ok(manifest) = ChecksumsManifest::from_files(&borrowed) else {
        return;
    };
    let rendered = manifest.render();
    let Some(first) = assets.first() else {
        return;
    };
    let (first_name, first_bytes) = (first.0.as_str(), first.1.as_slice());

    group.bench_function("manifest_from_files_5_targets_mib", |b| {
        b.iter(|| {
            let _ = black_box(ChecksumsManifest::from_files(black_box(&borrowed)));
        });
    });
    group.bench_function("manifest_parse_render", |b| {
        b.iter(|| {
            let _ = black_box(ChecksumsManifest::parse(black_box(&rendered)));
        });
    });
    group.bench_function("manifest_verify_one_asset_mib", |b| {
        b.iter(|| {
            let _ = black_box(manifest.verify(black_box(first_name), black_box(first_bytes)));
        });
    });

    // The M5.4 signing + SBOM grammar's render cycle (ADR-0054): one
    // sign + one verify line per release blob — the five pinned target
    // assets plus the SBOM — the lines the producer/consumer templates
    // pin and the gate re-renders on every run.
    group.bench_function("signing_lines_5_targets_plus_sbom", |b| {
        b.iter(|| {
            let mut lines = Vec::with_capacity(signed_blobs().len() * 2);
            for blob in signed_blobs() {
                lines.push(black_box(sign_command(&blob)));
                lines.push(black_box(verify_command(&blob)));
            }
            black_box(lines)
        });
    });
}
