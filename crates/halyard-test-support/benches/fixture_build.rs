//! Fixture build throughput.
//!
//! Fixture construction speed gates the scale of the differential tests that
//! arrive with the git engine (M2.2 requires 10 000 painter-walk samples);
//! this bench keeps the DSL honest as scenarios grow.

use criterion::Criterion;
use halyard_test_support::scenarios;

/// Build the two structural scenarios repeatedly.
fn bench_scenario_builds(c: &mut Criterion) {
    let mut group = c.benchmark_group("git_fixture");
    group.sample_size(20);
    group.bench_function("linear_cargo_workspace", |b| {
        b.iter(scenarios::linear_cargo_workspace);
    });
    group.bench_function("merge_two_branches", |b| {
        b.iter(scenarios::merge_two_branches);
    });
    group.finish();
}

/// Criterion entry point (hand-written instead of `criterion_group!` so the
/// generated function keeps its doc comment under `missing_docs`).
fn main() {
    let mut c = Criterion::default().configure_from_args();
    bench_scenario_builds(&mut c);
}
