//! Bench: the maven adapter's Gradle dependency-edge slice (ADR-0058
//! continued — the recorded-open `project(":sub")` remainder's
//! landing). Measures the build-file grammar scan WITH a top-level
//! `dependencies { … }` block (the brace/identifier state machine plus
//! the `project(':…')` captures) and the governed discovery pass that
//! resolves the proven paths into internal edges.

use criterion::Criterion;
use halyard_core::EcosystemAdapter;
use halyard_core::discover::DiscoverCtx;
use halyard_eco_maven::MavenAdapter;

/// The build file fixture: the version literal, a realistic
/// instruction mix (a plugins block, comments, a sibling string), and
/// a `dependencies` block carrying the documented project rows plus a
/// string-notation row the grammar must skip.
const BUILD_WITH_DEPS: &str = concat!(
    "// the version (the documented literal assignment)\n",
    "version = '1.4.2'\n",
    "\n",
    "plugins {\n",
    "    id 'java'\n",
    "}\n",
    "\n",
    "dependencies {\n",
    "    implementation project(':library')\n",
    "    api project(':core')\n",
    "    testImplementation project(':library')\n",
    "    implementation 'com.example:external:1.0.0'\n",
    "}\n",
);

/// One scan of one build file with its dependency rows, then one
/// governed discovery resolving the edges. The bench fixture failure
/// exits loudly instead of panicking (the sibling bench posture).
fn fail(reason: &str) -> ! {
    eprintln!("the fixture must {reason}");
    std::process::exit(101);
}

fn bench(c: &mut Criterion) {
    let mut ctx = DiscoverCtx::new();
    if !ctx.insert_file("settings.gradle", "include 'library'\n".as_bytes().to_vec()) {
        fail("carry the settings fixture");
    }
    if !ctx.insert_file("build.gradle", BUILD_WITH_DEPS.as_bytes().to_vec()) {
        fail("carry the root fixture");
    }
    if !ctx.insert_file(
        "library/build.gradle",
        "version = '2.0.0'\n".as_bytes().to_vec(),
    ) {
        fail("carry the library fixture");
    }
    c.bench_function("maven_gradle_deps_discover", |b| {
        b.iter(|| {
            let rows = MavenAdapter::new()
                .discover(&ctx)
                .unwrap_or_else(|_| fail("discover the fixture"));
            assert_eq!(rows.len(), 2, "the root and the member");
            let root = rows
                .iter()
                .find(|row| row.manifest == "build.gradle")
                .unwrap_or_else(|| fail("carry the root row"));
            assert_eq!(root.deps.len(), 2, "the two proven edges resolve");
        });
    });
}

/// The bench entry point (the criterion macros generate undocumented
/// items — the workspace's missing-docs gate forbids them; the sibling
/// benches call the group directly).
fn main() {
    bench(&mut Criterion::default().configure_from_args());
}
