//! Bench: pypi `pyproject.toml` scan (§6.4's hot path at the PEP 621
//! grammar, M8 step 1).
//!
//! Measures the `toml_edit` span parse + PEP 440/508 classification over
//! a representative uv-workspace member manifest (deps in all three
//! surfaces + sources + workspace table). Budget row: §14.1 "Plan with
//! 200 new commits, 40 crates"; formal budgets arm at M2.6.

use criterion::Criterion;
use halyard_core::EcosystemAdapter;

const PYPROJECT: &str = r#"[project]
name = "albatross"
version = "1.4.2"
requires-python = ">=3.12"
dependencies = [
  "halyard-core>=0.1",
  "tqdm>=4,<5",
  "requests[security]>=2.31",
  "numpy>=1.26 ; sys_platform == 'win32'",
]

[project.optional-dependencies]
cli = ["typer>=0.12"]
test = ["pytest>=8", "pytest-cov>=5"]

[dependency-groups]
dev = ["ruff>=0.6", "mypy>=1.11"]

[tool.uv.sources]
halyard-core = { workspace = true }

[tool.uv.workspace]
members = ["packages/*"]
exclude = ["packages/seeds"]
"#;

fn scan() {
    // The fixture is a constant that must parse; a parse failure aborts
    // the bench loudly (bench fns are not `#[test]`, so the test-lint
    // allowance does not reach them).
    let mut ctx = halyard_core::discover::DiscoverCtx::new();
    assert!(
        ctx.insert_file("pyproject.toml", PYPROJECT.as_bytes().to_vec()),
        "fixture path"
    );
    let rows = match halyard_eco_pypi::PyPiAdapter.discover(&ctx) {
        Ok(rows) => rows,
        Err(error) => {
            eprintln!("the bench fixture must parse: {error}");
            std::process::exit(101);
        }
    };
    let Some(row) = rows.first() else {
        std::process::exit(101);
    };
    let _ = row.version.as_ref().map(std::string::ToString::to_string);
    let _ = row.deps.len();
}

fn bench_pyproject_scan(c: &mut Criterion) {
    let mut group = c.benchmark_group("eco-pypi");
    group.throughput(criterion::Throughput::Bytes(
        u64::try_from(PYPROJECT.len()).unwrap_or(u64::MAX),
    ));
    group.bench_function("scan_pyproject", |b| {
        b.iter(scan);
    });
    group.finish();
}

/// Criterion harness entry (the `criterion_group!` expansion, spelled out
/// so the generated `pub fn` does not trip `missing_docs`).
fn benches() {
    let mut criterion: Criterion = Criterion::default().configure_from_args();
    bench_pyproject_scan(&mut criterion);
}

fn main() {
    benches();
    Criterion::default().configure_from_args().final_summary();
}
