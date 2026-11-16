//! Bench: the §10.3 branch-bound channel resolution (ADR-0025 step 2).
//!
//! `ResolvedConfig::channel_for_branch` runs once per planning command
//! in the CLI's `context()` — a map lookup over the branch bindings
//! falling back to the unbound default. The bench pins that the
//! resolution stays a trivial scan (the resolution must never cost more
//! than the plan it feeds) and that a binding-heavy config does not
//! change the shape.

use std::hint::black_box;

use criterion::Criterion;
use halyard_config::{Layer, ResolvedConfig, resolve};

/// A config with `bindings` branch bindings and an unbound default.
fn resolved(bindings: usize) -> ResolvedConfig {
    let bindings: std::collections::BTreeMap<String, String> = (0..bindings)
        .map(|idx| (format!("train-{idx:03}"), "rc".to_owned()))
        .collect();
    let versioning = halyard_config::model::VersioningCfg {
        channel: Some("beta".to_owned()),
        channel_branches: Some(bindings),
        ..halyard_config::model::VersioningCfg::default()
    };
    let cfg = halyard_config::HalyardConfig {
        versioning: Some(versioning),
        ..halyard_config::HalyardConfig::default()
    };
    resolve(&[Layer::Defaults, Layer::File(cfg)])
        .unwrap_or_else(|_| unreachable!("fixture config is valid"))
}

fn bench_branch_channels(c: &mut Criterion) {
    let mut group = c.benchmark_group("channel_branch");

    // The bound hit: the branch's binding wins over the default.
    let config = resolved(16);
    group.bench_function("bound_hit_16", |b| {
        b.iter(|| {
            let _ = black_box(config.channel_for_branch(black_box(Some("train-008"))));
        });
    });

    // The fallback: no binding for the branch, the default applies.
    group.bench_function("default_fallback_16", |b| {
        b.iter(|| {
            let _ = black_box(config.channel_for_branch(black_box(Some("main"))));
        });
    });

    // A detached/unborn HEAD: `None` plans with the default.
    group.bench_function("detached_head_16", |b| {
        b.iter(|| {
            let _ = black_box(config.channel_for_branch(black_box(None)));
        });
    });

    group.finish();
}

/// Criterion entry point (hand-written instead of `criterion_group!` so the
/// generated function keeps its doc comment under `missing_docs`).
fn main() {
    let mut c = Criterion::default().configure_from_args();
    bench_branch_channels(&mut c);
}
