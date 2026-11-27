//! Bench: the generic adapter's edit seam (M2 of the §17 `generic` cell,
//! ADR-0062). Measures the semantic re-locate + read-back verify over the
//! declared version file — the apply path's own unit (the pattern
//! re-compiles per patch application because the working-tree bytes may
//! have drifted since the plan; the compile is part of the honest cost).

use criterion::Criterion;
use halyard_core::EcosystemAdapter;
use halyard_core::discover::GenericFileSpec;
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;
use halyard_eco_generic::GenericAdapter;

/// The declared-file fixture: a version row inside surrounding markup.
const MARKED_UP: &str = "pub const VERSION: &str = \"1.4.2\"; // the release row\n";

/// The bench fixture failure exits loudly instead of panicking (the
/// sibling bench posture).
fn fail(reason: &str) -> ! {
    eprintln!("the fixture must {reason}");
    std::process::exit(101);
}

/// One semantic edit application over one declared file.
fn bench(c: &mut Criterion) {
    let file = RelPath::new("src/version.rs").unwrap_or_else(|| fail("carry a normalized path"));
    let specs = [GenericFileSpec {
        name: "app".to_owned(),
        file: "src/version.rs".to_owned(),
        pattern: "VERSION: &str = \"(?<version>\\d+\\.\\d+\\.\\d+)\";".to_owned(),
    }];
    let adapter = GenericAdapter::new(&specs);
    let start = u32::try_from(
        MARKED_UP
            .find("1.4.2")
            .unwrap_or_else(|| fail("fixture span")),
    )
    .unwrap_or_else(|_| fail("fixture span fits u32"));
    let end = start + u32::try_from("1.4.2".len()).unwrap_or_else(|_| fail("fits u32"));
    let patches = [PlanPatch {
        start,
        end,
        text: "1.5.0".to_owned(),
    }];
    c.bench_function("generic_edit_declared_file", |b| {
        b.iter(|| {
            let out = adapter
                .edit_versions(&file, MARKED_UP.as_bytes(), &patches)
                .unwrap_or_else(|_| fail("apply the fixture"));
            assert_eq!(out.applied, 1);
        });
    });
}

/// The bench entry point (the criterion macros generate undocumented
/// items — the workspace's missing-docs gate forbids them; the sibling
/// benches call the group directly).
fn main() {
    bench(&mut Criterion::default().configure_from_args());
}
