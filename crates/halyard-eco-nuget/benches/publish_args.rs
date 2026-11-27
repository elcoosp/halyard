//! Bench: the `dotnet` publish argv assembly (the §17 M8 `NuGet` cell's
//! argv arm, ADR-0059 surface).
//!
//! Measures the pack/push assembly per shape — the assembly the spawn
//! effect runs per nuget target. The assembly must stay free (argv
//! atoms, I7); the shapes pin the bare pack and the documented
//! official-server push (I3).

use criterion::Criterion;

use halyard_eco_nuget::{dotnet_pack_args, dotnet_push_args};

fn bench_publish_args(c: &mut Criterion) {
    let mut group = c.benchmark_group("eco-nuget");
    group.bench_function("publish_args_pack", |b| {
        b.iter(|| dotnet_pack_args());
    });
    group.bench_function("publish_args_push", |b| {
        b.iter(|| dotnet_push_args("nuget.org", "Contoso.App", "1.2.4"));
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
}
