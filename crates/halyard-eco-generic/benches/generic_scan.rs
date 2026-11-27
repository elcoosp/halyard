//! Bench: the generic adapter's discovery slice (M1 of the §17 `generic`
//! cell, ADR-0062). Measures the pattern compile + UTF-8 match + capture
//! over the declared version file — the discovery-path cost per declared
//! package (the planner re-runs discovery on the same bytes at apply
//! time, so the scan stays pure and cheap).

use criterion::Criterion;
use halyard_core::EcosystemAdapter;
use halyard_core::discover::{DiscoverCtx, GenericFileSpec};
use halyard_eco_generic::GenericAdapter;

/// The declared-file fixture: a version row inside surrounding markup
/// (the realistic shape — a `const` row, not a bare `VERSION` file).
const MARKED_UP: &str = "pub const VERSION: &str = \"1.4.2\"; // the release row\n";

/// One scan of one declared file (spec construction + compile + match +
/// row build) — the discovery path's own unit.
/// The bench fixture failure exits loudly instead of panicking (the
/// sibling bench posture).
fn fail(reason: &str) -> ! {
    eprintln!("the fixture must {reason}");
    std::process::exit(101);
}

fn bench(c: &mut Criterion) {
    let mut ctx = DiscoverCtx::new();
    if !ctx.insert_file("src/version.rs", MARKED_UP.as_bytes().to_vec()) {
        fail("carry the fixture file");
    }
    let specs = [GenericFileSpec {
        name: "app".to_owned(),
        file: "src/version.rs".to_owned(),
        pattern: "VERSION: &str = \"(?<version>\\d+\\.\\d+\\.\\d+)\";".to_owned(),
    }];
    c.bench_function("generic_scan_declared_file", |b| {
        b.iter(|| {
            let rows = GenericAdapter::new(&specs)
                .discover(&ctx)
                .unwrap_or_else(|_| fail("scan the fixture"));
            assert_eq!(rows.len(), 1);
        });
    });
}

/// The bench entry point (the criterion macros generate undocumented
/// items — the workspace's missing-docs gate forbids them; the sibling
/// benches call the group directly).
fn main() {
    bench(&mut Criterion::default().configure_from_args());
}
