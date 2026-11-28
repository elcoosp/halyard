//! Bench: the maven adapter's Gradle discovery slice (M1 of the
//! Maven/Gradle cell's Gradle half, ADR-0058 continued). Measures the
//! hand-rolled Gradle lexer's comment/string skip + the
//! `version = <literal>` assignment scan over a realistic build script
//! — the discovery-path cost per build file (the planner re-runs
//! discovery on the same bytes at apply time, so the scan stays pure
//! and cheap).

use criterion::Criterion;
use halyard_core::EcosystemAdapter;
use halyard_core::discover::DiscoverCtx;
use halyard_eco_maven::MavenAdapter;

/// The Groovy build-file fixture: the literal `version = '1.4.2'`
/// assignment inside the realistic instruction mix (a `plugins` block,
/// a `repositories` block, a line comment, a block comment, and a
/// sibling string literal that the scanner must skip in place).
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

/// One scan of one build file (the discovery path's own unit). The
/// bench fixture failure exits loudly instead of panicking (the sibling
/// bench posture).
fn fail(reason: &str) -> ! {
    eprintln!("the fixture must {reason}");
    std::process::exit(101);
}

fn bench(c: &mut Criterion) {
    let mut ctx = DiscoverCtx::new();
    if !ctx.insert_file("build.gradle", BUILD_GRADLE.as_bytes().to_vec()) {
        fail("carry the fixture file");
    }
    c.bench_function("maven_gradle_scan_build", |b| {
        b.iter(|| {
            let rows = MavenAdapter::new()
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
