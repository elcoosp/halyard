//! Bench: the Dart/pub pubspec scan (§6.4's hot path at the
//! `pubspec.yaml` grammar, M8 `Dart/pub` cell, ADR-0060). Measures the
//! hand-rolled scanner + row assembly over a representative pub
//! workspace (the root with `workspace` entries, two members with
//! hosted/name-matched deps, one path-source dep, comments, and one
//! canonical `version` row).

use criterion::Criterion;
use halyard_core::EcosystemAdapter;
use halyard_core::discover::DiscoverCtx;
use halyard_eco_dart::DartAdapter;

const ROOT_PUBSPEC: &str = r"# the pub workspace root (the documented shape)
name: _
publish_to: none
environment:
  sdk: ^3.6.0
workspace:
  - packages/shared
  - packages/app
";

const SHARED_PUBSPEC: &str = r"# the shared library every member names
name: shared
version: 2.3.0
resolution: workspace
environment:
  sdk: ^3.6.0
dependencies:
  http: ^1.2.0
  meta: ^1.15.0
";

const APP_PUBSPEC: &str = r"# the application member
name: app
version: 1.4.0 # the release row
resolution: workspace
environment:
  sdk: ^3.6.0
dependencies:
  shared: ^2.3.0
  shared_ui:
    path: ../shared
  flutter_driver:
    sdk: flutter
dev_dependencies:
  test: ^1.25.0
dependency_overrides:
  meta: ^1.15.0
";

fn ctx() -> DiscoverCtx {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("pubspec.yaml", ROOT_PUBSPEC.as_bytes().to_vec());
    ctx.insert_file(
        "packages/shared/pubspec.yaml",
        SHARED_PUBSPEC.as_bytes().to_vec(),
    );
    ctx.insert_file("packages/app/pubspec.yaml", APP_PUBSPEC.as_bytes().to_vec());
    ctx
}

fn scan() {
    let ctx = ctx();
    // The fixture is a constant that must parse; a parse failure exits
    // the bench loudly (bench fns are not `#[test]`, so the test-lint
    // allowance does not reach them — the sibling nuget bench posture).
    let rows = match DartAdapter::new().discover(&ctx) {
        Ok(rows) => rows,
        Err(error) => {
            eprintln!("the bench fixture must parse: {error}");
            std::process::exit(101);
        }
    };
    assert_eq!(rows.len(), 3);
}

fn bench(c: &mut Criterion) {
    c.bench_function("dart_pubspec_scan_3_packages", |b| b.iter(scan));
}

/// The bench entry point (the criterion macros generate undocumented
/// items — the workspace's missing-docs gate forbids them; the sibling
/// benches call the group directly).
fn main() {
    bench(&mut Criterion::default().configure_from_args());
}
