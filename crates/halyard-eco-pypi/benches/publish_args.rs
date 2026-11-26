//! Bench: the `uv` publish argv assembly (M8 step 2, ADR-0056 surface).
//!
//! Measures the live/dry assembly per shape — the assembly the spawn
//! effect runs per pypi target. The assembly must stay free (argv
//! atoms, I7); the shapes pin the bare live argv and the `--dry-run`
//! forecast (I3).

use criterion::Criterion;

use halyard_eco_pypi::uv_publish_args;

fn bench_publish_args(c: &mut Criterion) {
    let mut group = c.benchmark_group("eco-pypi");
    group.bench_function("publish_args_live", |b| {
        b.iter(|| uv_publish_args(false));
    });
    group.bench_function("publish_args_dry", |b| {
        b.iter(|| uv_publish_args(true));
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
