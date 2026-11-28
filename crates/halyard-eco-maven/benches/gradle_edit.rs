//! Bench: the maven adapter's Gradle edit half — the semantic in-place
//! literal-assignment version patch (M2 of the Maven/Gradle cell's
//! Gradle half, ADR-0058 continued). Measures the re-scan + span-match +
//! splice + read-back verify over a canonical Groovy build script.

use criterion::Criterion;
use halyard_core::EcosystemAdapter;
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;
use halyard_eco_maven::MavenAdapter;

/// The Groovy build-file fixture: the literal `version = '1.4.2'`
/// assignment inside the realistic instruction mix (the discovery
/// bench's shape — a `plugins` block, a line comment, a block comment,
/// a `repositories` block, and a sibling string literal the swap must
/// preserve).
const BUILD_GRADLE: &str = concat!(
    "plugins {\n",
    "    id 'java'\n",
    "}\n",
    "\n",
    "// the project version (a literal assignment — the documented form)\n",
    "/* the override rule is ordinary property overwrite, no surprise */\n",
    "version = '1.4.2'\n",
    "\n",
    "repositories {\n",
    "    mavenCentral()\n",
    "}\n",
    "\n",
    "description = \"the version = '9.9.9' string is skipped in place\"\n",
);

/// The version literal's value text in the fixture (the discovery scan
/// pins its span; the edit aims the patch there).
const VALUE_TEXT: &str = "1.4.2";

/// The bench fixture failure exits loudly instead of panicking (the
/// sibling bench posture).
fn fail(reason: &str) -> ! {
    eprintln!("the fixture must {reason}");
    std::process::exit(101);
}

fn bench(c: &mut Criterion) {
    // The patch aims at the last literal's value span (the span
    // discovery emits — located by scanning, pinned by the edit contract
    // tests).
    let start = BUILD_GRADLE
        .find(VALUE_TEXT)
        .unwrap_or_else(|| fail("carry the version value"));
    let end = start + VALUE_TEXT.len();
    let file = RelPath::new("build.gradle").unwrap_or_else(|| fail("carry the fixture path"));
    let patches = [PlanPatch {
        start: u32::try_from(start).unwrap_or_else(|_| fail("carry the span")),
        end: u32::try_from(end).unwrap_or_else(|_| fail("carry the span")),
        text: "1.5.0".to_owned(),
    }];
    let adapter = MavenAdapter::new();
    c.bench_function("maven_gradle_edit_version", |b| {
        b.iter(|| {
            let outcome = adapter
                .edit_versions(&file, BUILD_GRADLE.as_bytes(), &patches)
                .unwrap_or_else(|_| fail("apply the patch"));
            assert_eq!(outcome.applied, 1);
        });
    });
}

/// The bench entry point (the criterion macros generate undocumented
/// items — the workspace's missing-docs gate forbids them; the sibling
/// benches call the group directly).
fn main() {
    bench(&mut Criterion::default().configure_from_args());
}
