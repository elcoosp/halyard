//! Bench: per-target registry routing (M5.2 continued, ADR-0021).
//!
//! Measures [`TargetRouter::registry_for`] — the override-first,
//! by-name-fallback resolution the engine runs once per target before
//! any call (validate-then-act) — against routers with zero and 64
//! npm-side overrides, over the two-target a→b publication the engine
//! suite shares. Microsecond scale by design: routing must never be
//! the reason a publish run waits.

use std::sync::Arc;

use criterion::Criterion;
use halyard_publish::{MemoryRegistry, PublishTarget, RegistryRouter, TargetRouter};

/// A publish target shaped like the engine suite's `a` (the routing
/// reads only `registry` and `package`).
fn target(registry: &str, package: &str) -> PublishTarget {
    PublishTarget {
        package: package.to_owned(),
        version: "0.2.0".to_owned(),
        ecosystem: "cargo".to_owned(),
        registry: registry.to_owned(),
        manifest_dir: String::new(),
        manifest: "Cargo.toml".to_owned(),
        access: None,
        provenance: None,
    }
}

fn bench_target_router(c: &mut Criterion) {
    let by_name_client = Arc::new(MemoryRegistry::new()) as Arc<dyn halyard_registry::Registry>;
    let mut group = c.benchmark_group("target_router");
    group.throughput(criterion::Throughput::Elements(1));

    group.bench_function("by_name_route", |b| {
        let router = TargetRouter::new().with(
            "crates.io",
            Arc::clone(&by_name_client) as Arc<dyn halyard_registry::Registry>,
        );
        let target = target("crates.io", "a");
        b.iter(|| {
            assert!(router.registry_for(&target).is_ok());
        });
    });

    group.bench_function("override_hit", |b| {
        let router = TargetRouter::new()
            .with(
                "crates.io",
                Arc::clone(&by_name_client) as Arc<dyn halyard_registry::Registry>,
            )
            .with_override(
                "crates.io",
                "a",
                Arc::new(MemoryRegistry::new()) as Arc<dyn halyard_registry::Registry>,
            );
        let target = target("crates.io", "a");
        b.iter(|| {
            assert!(router.registry_for(&target).is_ok());
        });
    });

    group.bench_function("override_miss_among_64", |b| {
        let mut router = TargetRouter::new().with(
            "crates.io",
            Arc::clone(&by_name_client) as Arc<dyn halyard_registry::Registry>,
        );
        for i in 0..64 {
            router = router.with_override(
                "npm",
                &format!("pkg-{i:03}"),
                Arc::new(MemoryRegistry::new()) as Arc<dyn halyard_registry::Registry>,
            );
        }
        // A miss walks the override map (the `(registry, package)` key
        // is absent) and falls back to the by-name route.
        let target = target("crates.io", "a");
        b.iter(|| {
            assert!(router.registry_for(&target).is_ok());
        });
    });

    group.finish();
}

fn main() {
    let mut criterion = criterion::Criterion::default();
    bench_target_router(&mut criterion);
}
