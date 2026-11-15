//! Bench: cargo-registry config resolution (M5.2 continued, ADR-0021).
//!
//! Measures the pure resolution core (`resolve_registry_index`: TOML
//! parse + `[registries.<name>]` lookup + URL classification) over a
//! realistic multi-registry configuration, and the sparse-URL grammar
//! (`sparse_index_base`). The resolution runs once per non-default
//! registry name per publish command — microsecond scale, but it sits
//! on the validate-then-act path the §15.3 gate runs before every
//! call, so a regression here is a CLI-latency regression.

use criterion::Criterion;
use halyard_eco_cargo::registry_config::{resolve_registry_index, sparse_index_base};

/// A realistic project-level config: one sparse custom registry (with
/// its token, the full entry shape), one git-index registry (the
/// honest-decline case), and an unrelated entry the lookup must skip.
fn realistic_config() -> String {
    String::from(
        "[registries.corp]\nindex = \"sparse+https://corp.example/index/\"\n\
         token = \"redacted\"\n\n\
         [registries.corp-legacy]\nindex = \"https://corp.example/git/index\"\n\n\
         [registries.vendor]\nindex = \"sparse+https://vendor.example.cn/index\"\n",
    )
}

fn bench_registry_resolve(c: &mut Criterion) {
    let config = realistic_config();
    let mut group = c.benchmark_group("registry_config");
    group.throughput(criterion::Throughput::Elements(1));
    group.bench_function("resolve_sparse_among_4", |b| {
        b.iter(|| {
            let lookup = resolve_registry_index("corp", &[("config.toml", &config)], None);
            assert!(
                matches!(
                    &lookup,
                    halyard_eco_cargo::registry_config::RegistryIndexLookup::Sparse(base)
                        if base == "https://corp.example/index"
                ),
                "the sparse entry resolves: {lookup}"
            );
        });
    });
    group.bench_function("resolve_git_index_decline", |b| {
        b.iter(|| {
            let lookup = resolve_registry_index("corp-legacy", &[("config.toml", &config)], None);
            assert!(
                matches!(
                    &lookup,
                    halyard_eco_cargo::registry_config::RegistryIndexLookup::Unsupported(_)
                ),
                "the git index declines: {lookup}"
            );
        });
    });
    group.bench_function("unknown_name_miss", |b| {
        b.iter(|| {
            assert_eq!(
                resolve_registry_index("absent", &[("config.toml", &config)], None),
                halyard_eco_cargo::registry_config::RegistryIndexLookup::Unknown
            );
        });
    });
    group.bench_function("sparse_url_classify", |b| {
        b.iter(|| {
            assert_eq!(
                sparse_index_base("sparse+https://corp.example/index/").as_deref(),
                Some("https://corp.example/index")
            );
        });
    });
    group.finish();
}

fn main() {
    let mut criterion = criterion::Criterion::default();
    bench_registry_resolve(&mut criterion);
}
