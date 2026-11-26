//! Bench: the release half's tag-shape cycle (M8, ADR-0057) — the work
//! the release/tag phases do per package: derive the per-package tag
//! template from the module's directory, render the release tag, and
//! parse the base tag back through the template (§8.2 step 3's
//! `extract_version`). Measures the shared appendix-A.3 machinery over
//! the go shapes (root + subdir + a /vN module path).

use criterion::Criterion;
use halyard_core::package::TagTemplate;
use halyard_eco_gomod::tag;

/// One release row's tag cycle: template → render → base parse.
fn tag_cycle(template: &TagTemplate, name: &str, version: &str, tag_text: &str) {
    // The fixture strings are constants that must parse; a parse
    // failure exits the bench loudly (bench fns are not `#[test]`, so
    // the test-lint allowance does not reach them).
    let version = match version.parse() {
        Ok(version) => version,
        Err(error) => {
            eprintln!("the bench fixture version must parse: {error}");
            std::process::exit(101);
        }
    };
    let rendered = template.render(name, &version, "go");
    assert_eq!(rendered, tag_text);
    assert_eq!(
        template.extract_version(tag_text, name, "go"),
        Some(version),
        "the base-tag parse round-trips"
    );
}

fn cycles() {
    let root = tag::template_for("");
    tag_cycle(&root, "example.com/halyard", "1.2.3", "v1.2.3");
    let api = tag::template_for("api");
    tag_cycle(&api, "example.com/halyard/api", "0.2.0", "api/v0.2.0");
    let metrics = tag::template_for("libs/metrics");
    tag_cycle(
        &metrics,
        "example.com/halyard/libs/metrics/v2",
        "2.1.0",
        "libs/metrics/v2.1.0",
    );
}

fn bench(c: &mut Criterion) {
    c.bench_function("tag_shapes_3_modules", |b| b.iter(cycles));
}

/// The bench entry point (the criterion macros generate undocumented
/// items — the workspace's missing-docs gate forbids them; the sibling
/// benches call the group directly).
fn main() {
    bench(&mut Criterion::default().configure_from_args());
}
