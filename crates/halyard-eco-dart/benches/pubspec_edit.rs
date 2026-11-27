//! Bench: the Dart/pub pubspec edit (the release half's hot path, M8
//! `Dart/pub` cell, ADR-0060). Measures the semantic patch — parse, slot
//! match, in-place byte splice, read-back re-parse — over a
//! representative pubspec.

use criterion::Criterion;
use halyard_core::EcosystemAdapter;
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;
use halyard_eco_dart::DartAdapter;

const APP: &str = "# the app member\nname: app\nversion: 1.4.0\nresolution: workspace\ndependencies:\n  shared: ^2.3.0\n  http: ^1.2.0\n";

fn edit() {
    // The fixture is a constant that must edit; a failure exits loudly
    // (bench fns are not `#[test]`, so the test-lint allowance does not
    // reach them — the pom_edit bench posture).
    let fail = |what: &str| -> ! {
        eprintln!("the bench fixture must {what}");
        std::process::exit(101);
    };
    let Some(file) = RelPath::new("packages/app/pubspec.yaml") else {
        fail("carry a normalized fixture path");
    };
    let start = APP
        .find("1.4.0")
        .and_then(|idx| u32::try_from(idx).ok())
        .unwrap_or_else(|| fail("carry the version row"));
    let patches = [PlanPatch {
        start,
        end: start + 5,
        text: "1.5.0".to_owned(),
    }];
    match DartAdapter.edit_versions(&file, APP.as_bytes(), &patches) {
        Ok(out) if out.applied == 1 => {}
        _ => {
            fail("edit cleanly");
        }
    }
}

fn bench(c: &mut Criterion) {
    c.bench_function("dart_pubspec_edit", |b| b.iter(edit));
}

/// The bench entry point (the criterion macros generate undocumented
/// items — the workspace's missing-docs gate forbids them; the sibling
/// benches call the group directly).
fn main() {
    bench(&mut Criterion::default().configure_from_args());
}
