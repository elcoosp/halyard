//! Cache record roundtrip (§8.7): the insert/flush path and the
//! reopen/get path — the two operations every planning run spends on the
//! persistent cache. Pins the M2.5 seam's per-operation cost so the
//! §14.1 no-op budgets stay honest.

use std::hint::black_box;

use criterion::Criterion;
use halyard_cache::{Cache, CacheKey, CacheRecord};
use halyard_core::{PkgId, SmallBitset};

/// Builds one cache record touching every `stride`-th of `packages`.
fn record(packages: u32) -> CacheRecord {
    let mut touched = SmallBitset::new();
    let mut idx = 0;
    while idx < packages {
        touched.set(PkgId::new(idx));
        idx += 8;
    }
    CacheRecord {
        touched,
        subject_end: 32,
        body_start: 34,
    }
}

/// One commit oid as bytes (any distinct 20 bytes work — the key hashes
/// them with the layout/config components).
fn oid(index: u32) -> [u8; 20] {
    let mut bytes = [0u8; 20];
    bytes[..4].copy_from_slice(&index.to_be_bytes());
    bytes
}

fn bench_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_roundtrip");
    for records in [200u32, 2000] {
        let rec = record(64);
        group.bench_function(format!("insert_flush_{records}"), |biter| {
            biter.iter(|| {
                let dir = tempfile::tempdir().unwrap_or_else(|e| unreachable!("tempdir: {e}"));
                let mut cache =
                    Cache::open(dir.path()).unwrap_or_else(|e| unreachable!("open: {e}"));
                let mut idx = 0;
                while idx < records {
                    cache
                        .insert(CacheKey::compute(&oid(idx), "layout", "config"), record(64))
                        .unwrap_or_else(|e| unreachable!("insert: {e}"));
                    idx += 1;
                }
                cache.flush().unwrap_or_else(|e| unreachable!("flush: {e}"));
                black_box(&rec);
            });
        });
        group.bench_function(format!("reopen_get_{records}"), |biter| {
            // Build the persisted file once; the benchmark reopens it and
            // reads every record back (the warm-run path, §8.7).
            let dir = tempfile::tempdir().unwrap_or_else(|e| unreachable!("tempdir: {e}"));
            {
                let mut cache =
                    Cache::open(dir.path()).unwrap_or_else(|e| unreachable!("open: {e}"));
                let mut idx = 0;
                while idx < records {
                    cache
                        .insert(CacheKey::compute(&oid(idx), "layout", "config"), record(64))
                        .unwrap_or_else(|e| unreachable!("insert: {e}"));
                    idx += 1;
                }
                cache.flush().unwrap_or_else(|e| unreachable!("flush: {e}"));
            }
            biter.iter(|| {
                let cache = Cache::open(dir.path()).unwrap_or_else(|e| unreachable!("reopen: {e}"));
                let mut hits = 0u32;
                let mut idx = 0;
                while idx < records {
                    if cache
                        .get(&CacheKey::compute(&oid(idx), "layout", "config"))
                        .is_some()
                    {
                        hits += 1;
                    }
                    idx += 1;
                }
                black_box(hits);
            });
        });
    }
    group.finish();
}

/// Criterion entry point (hand-written instead of `criterion_group!` so the
/// generated function keeps its doc comment under `missing_docs`).
fn main() {
    let mut c = Criterion::default().configure_from_args();
    bench_roundtrip(&mut c);
}
