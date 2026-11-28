//! Bench: the maven adapter's settings-file governance slice (M1 of the
//! Gradle half's multi-project aggregator shape, ADR-0058 continued).
//! Measures the settings-file grammar scan (the `rootProject.name` /
//! `include` captures over the comment/string-skipping lexer) plus the
//! adapter's governed discovery over the settings tree — the
//! discovery-path cost one settings file adds to its subtree.

use criterion::Criterion;
use halyard_core::EcosystemAdapter;
use halyard_core::discover::DiscoverCtx;
use halyard_eco_maven::{MavenAdapter, settings};

/// The settings file fixture: the name row, a parenthesized and an
/// unparenthesized include call, a comment and a sibling string the
/// scanner must skip in place — the realistic multi-project shape.
const SETTINGS_GRADLE: &str = concat!(
    "// the root project's name (the descriptor's setter)\n",
    "rootProject.name = 'polyglot-parent'\n",
    "\n",
    "include 'library'\n",
    "include(\":services:api\", \":tools\")\n",
    "\n",
    "description = \"include ':ghost'\"\n",
);

/// One settings scan plus one governed discovery over the subtree (the
/// discovery path's own unit). The bench fixture failure exits loudly
/// instead of panicking (the sibling bench posture).
fn fail(reason: &str) -> ! {
    eprintln!("the fixture must {reason}");
    std::process::exit(101);
}

fn bench(c: &mut Criterion) {
    // The grammar scan alone: one settings file, the lexer's own cost.
    c.bench_function("maven_settings_scan", |b| {
        b.iter(|| {
            let parsed = settings::parse_settings(SETTINGS_GRADLE)
                .unwrap_or_else(|_| fail("parse the fixture"));
            assert_eq!(parsed.root_names.len(), 1);
            assert_eq!(parsed.includes.len(), 2);
        });
    });

    // The governed discovery: the settings file plus three build files
    // (the root and two members) — the subtree the settings file names.
    let mut ctx = DiscoverCtx::new();
    if !ctx.insert_file("settings.gradle", SETTINGS_GRADLE.as_bytes().to_vec()) {
        fail("carry the settings fixture");
    }
    if !ctx.insert_file("build.gradle", "version = '1.0.0'\n".as_bytes().to_vec()) {
        fail("carry the root fixture");
    }
    if !ctx.insert_file(
        "library/build.gradle",
        "version = '2.0.0'\n".as_bytes().to_vec(),
    ) {
        fail("carry the library fixture");
    }
    if !ctx.insert_file(
        "services/api/build.gradle.kts",
        "version = \"0.3.0\"\n".as_bytes().to_vec(),
    ) {
        fail("carry the api fixture");
    }
    c.bench_function("maven_settings_governed_discover", |b| {
        b.iter(|| {
            let rows = MavenAdapter::new()
                .discover(&ctx)
                .unwrap_or_else(|_| fail("discover the fixture"));
            assert_eq!(rows.len(), 3);
        });
    });
}

/// The bench entry point (the criterion macros generate undocumented
/// items — the workspace's missing-docs gate forbids them; the sibling
/// benches call the group directly).
fn main() {
    bench(&mut Criterion::default().configure_from_args());
}
