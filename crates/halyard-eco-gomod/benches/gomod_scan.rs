//! Bench: Go-module scan (§6.4's hot path at the go.mod/go.work grammar,
//! M8). Measures the hand-rolled lexer + row assembly over a
//! representative multi-module workspace (go.work + three member go.mods
//! with require blocks, replaces, and a /v2 suffix).

use criterion::Criterion;
use halyard_core::EcosystemAdapter;
use halyard_core::discover::DiscoverCtx;
use halyard_eco_gomod::GoModAdapter;

const GO_WORK: &str = "go 1.23.0\n\nuse (\n\t.\n\t./api\n\t./libs/metrics\n)\n\nreplace example.com/external/sdk => ./vendor/sdk\n";

const ROOT_GOMOD: &str = r"module example.com/halyard

go 1.23.0

require (
	example.com/halyard/api v0.1.2
	example.com/external/sdk v1.4.0
	example.com/other/dep v0.2.0 // indirect
)

replace example.com/other/dep => example.com/fork/dep v0.2.1
";

const API_GOMOD: &str = r"module example.com/halyard/api

go 1.23.0

require (
	example.com/halyard/libs/metrics/v2 v2.1.0
	example.com/external/sdk v1.4.0
)

exclude example.com/external/sdk v1.3.0
";

const METRICS_GOMOD: &str = r"module example.com/halyard/libs/metrics/v2

go 1.23.0

require example.com/external/sdk v1.4.0
";

fn ctx() -> DiscoverCtx {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("go.work", GO_WORK.as_bytes().to_vec());
    ctx.insert_file("go.mod", ROOT_GOMOD.as_bytes().to_vec());
    ctx.insert_file("api/go.mod", API_GOMOD.as_bytes().to_vec());
    ctx.insert_file("libs/metrics/go.mod", METRICS_GOMOD.as_bytes().to_vec());
    ctx
}

fn scan() {
    let ctx = ctx();
    // The fixture is a constant that must discover; a failure exits
    // loudly (bench fns are not `#[test]`, so the test-lint allowance
    // does not reach them — the pom_edit bench posture).
    let rows = match GoModAdapter::new().discover(&ctx) {
        Ok(rows) => rows,
        Err(error) => {
            eprintln!("the bench fixture must discover: {error}");
            std::process::exit(101);
        }
    };
    assert_eq!(rows.len(), 3);
}

fn bench(c: &mut Criterion) {
    c.bench_function("gomod_scan_3_modules", |b| b.iter(scan));
}

/// The bench entry point (the criterion macros generate undocumented
/// items — the workspace's missing-docs gate forbids them; the sibling
/// benches call the group directly).
fn main() {
    bench(&mut Criterion::default().configure_from_args());
}
