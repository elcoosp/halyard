//! The §16.2 `bench` command's aggregation cost (ADR-0042). The
//! reduction is a pure function of the recorded samples, so the bench
//! feeds synthetic distributions — the per-stage grouping plus the
//! nearest-rank percentiles over the sample counts a real pass loop
//! produces (8 passes, the default; 200, a long-running gate) — so a
//! percentile-math regression that turns the report linear never
//! reaches the command.

use std::hint::black_box;

use criterion::Criterion;
use halyard_cli::bench::{StageSample, aggregate};

/// The four pipeline stages the `bench` command records (the
/// `--profile` trace's span vocabulary).
const STAGES: [&str; 4] = ["config", "discover", "history", "plan"];

/// `stages × passes` samples with deterministic, magnitudinally
/// plausible durations (each stage sits in its real band, varied by the
/// sample index so the percentiles have spread to rank).
fn samples(passes: usize) -> Vec<StageSample> {
    let bands: [(u128, u128); 4] = [(70, 30), (9_900, 300), (165_000, 5_000), (13_000, 400)];
    let mut out = Vec::with_capacity(STAGES.len() * passes);
    for pass in 0..passes {
        for (stage, (base, spread)) in STAGES.iter().zip(bands) {
            out.push(StageSample {
                stage,
                duration_us: base + u128::try_from(pass % 17).unwrap_or(0) * spread / 17,
            });
        }
    }
    out
}

fn bench_bench_report(c: &mut Criterion) {
    let mut group = c.benchmark_group("bench_report");
    group.bench_function("aggregate_4stages_8passes", |biter| {
        let samples = samples(8);
        biter.iter(|| aggregate(black_box(&samples)));
    });
    group.bench_function("aggregate_4stages_200passes", |biter| {
        let samples = samples(200);
        biter.iter(|| aggregate(black_box(&samples)));
    });
    group.finish();
}

fn main() {
    bench_bench_report(&mut Criterion::default().configure_from_args());
    Criterion::default().configure_from_args().final_summary();
}
