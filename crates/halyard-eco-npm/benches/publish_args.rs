//! Bench: the pnpm publish argv assembly (ADR-0033 surface).
//!
//! Measures [`NpmPublishOptions`] → argv per shape (unset, access-only,
//! provenance pair, the combined form) — the assembly the spawn effect
//! runs per npm target. The assembly must stay free (argv atoms, I7);
//! the shapes pin the deterministic access-before-provenance order (I3).

use criterion::Criterion;

use halyard_eco_npm::{NpmPublishOptions, pnpm_publish_args};

/// The unset shape: the bare `publish` argv (trusted publishing
/// decides).
fn argv_unset(dry: bool) -> Vec<String> {
    pnpm_publish_args(dry, &NpmPublishOptions::default())
}

/// The combined shape: access + explicit provenance request.
fn argv_combined(dry: bool) -> Vec<String> {
    let options = NpmPublishOptions {
        access: Some("public".to_owned()),
        provenance: Some(true),
    };
    pnpm_publish_args(dry, &options)
}

/// The documented opt-out shape: `--no-provenance` (the self-hosted
/// escape hatch).
fn argv_opt_out(dry: bool) -> Vec<String> {
    let options = NpmPublishOptions {
        access: Some("restricted".to_owned()),
        provenance: Some(false),
    };
    pnpm_publish_args(dry, &options)
}

fn bench_publish_args(c: &mut Criterion) {
    let mut group = c.benchmark_group("eco-npm");
    group.bench_function("publish_args_unset_live", |b| {
        b.iter(|| argv_unset(false));
    });
    group.bench_function("publish_args_combined_dry", |b| {
        b.iter(|| argv_combined(true));
    });
    group.bench_function("publish_args_opt_out_live", |b| {
        b.iter(|| argv_opt_out(false));
    });
    group.finish();
}

/// Criterion harness entry (the `criterion_group!` expansion, spelled out
/// so the generated `pub fn` does not trip `missing_docs`).
fn benches() {
    let mut criterion: Criterion = Criterion::default().configure_from_args();
    bench_publish_args(&mut criterion);
}

fn main() {
    benches();
    Criterion::default().configure_from_args().final_summary();
}
