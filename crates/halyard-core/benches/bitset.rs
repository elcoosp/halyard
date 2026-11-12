//! `SmallBitset` throughput.
//!
//! Bitset paint/union/get are the per-commit hot-loop primitives of the
//! git engine (§8.3); the bench pins both the inline (< 128 packages) and
//! spill (> 128 packages) regions so the M2.2 walk inherits honest numbers.

use std::hint::black_box;

use criterion::Criterion;
use halyard_core::{PkgId, SmallBitset};

/// Builds a bitset with every `stride`-th package id set up to `packages`.
fn build(packages: u32, stride: u32) -> SmallBitset {
    let mut set = SmallBitset::new();
    let mut idx = 0;
    while idx < packages {
        set.set(PkgId::new(idx));
        idx += stride.max(1);
    }
    set
}

fn bench_bitset(c: &mut Criterion) {
    let mut group = c.benchmark_group("small_bitset");
    for packages in [64u32, 128, 512, 2048] {
        let a = build(packages, 2);
        let b = build(packages, 3);
        group.bench_function(format!("union_{packages}"), |biter| {
            biter.iter(|| {
                let mut out = a.clone();
                out.union_with(black_box(&b));
                out
            });
        });
        group.bench_function(format!("count_iter_{packages}"), |biter| {
            biter.iter(|| black_box(&a).iter().count());
        });
    }
    group.bench_function("get_mixed", |biter| {
        biter.iter(|| {
            (0..256u32)
                .map(|idx| black_box(&build(512, 7)).get(PkgId::new(idx)))
                .fold(0u32, |acc, hit| acc + u32::from(hit))
        });
    });
    group.finish();
}

/// Criterion entry point (hand-written instead of `criterion_group!` so the
/// generated function keeps its doc comment under `missing_docs`).
fn main() {
    let mut c = Criterion::default().configure_from_args();
    bench_bitset(&mut c);
}
