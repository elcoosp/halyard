//! Bench: Cargo manifest parse + span extraction (§6.2 hot path).
//!
//! Measures `toml_edit` parse + dependency/inheritance extraction over a
//! representative workspace root and a member manifest with the full
//! §6.3 surface. Budget row: §14.1 "Plan with 200 new commits, 40 crates"
//! (discovery is its front half); formal budgets arm at M2.6.

use criterion::Criterion;

const ROOT_MANIFEST: &str = r#"
[workspace]
members = ["crates/*", "tools/*"]
exclude = ["crates/legacy"]

[workspace.package]
version = "1.4.2"
edition = "2021"

[workspace.dependencies]
serde = { version = "1.0", features = ["derive"] }
halyard-core = { path = "crates/core", version = "1.4.2" }
anyhow = "1.0"
"#;

const MEMBER_MANIFEST: &str = r#"
[package]
name = "halyard-engine"
version.workspace = true
edition.workspace = true
publish = false

[package.metadata.halyard]
tag_style = "{name}-v{version}"

[lib]
crate-type = ["rlib"]

[dependencies]
serde = { workspace = true }
halyard-core = { workspace = true }
anyhow = { version = "1.0", optional = true }
[target.'cfg(unix)'.dependencies]
libc = "0.2"
[dev-dependencies]
halyard-core = { workspace = true }
tempfile = "3"
[build-dependencies]
cc = "1.0"
"#;

fn parse_pair() {
    // Bench fixture constants must parse; a failure aborts the bench
    // loudly (bench fns are not `#[test]`, so the expect/unwrap lint
    // allowance does not reach them). `Document` (the span-carrying
    // parse) replaced the deprecated `ImDocument` alias (toml_edit 0.25).
    let root = match toml_edit::Document::parse(std::hint::black_box(ROOT_MANIFEST)) {
        Ok(doc) => doc,
        Err(e) => unreachable!("root manifest parses: {e}"),
    };
    let member = match toml_edit::Document::parse(std::hint::black_box(MEMBER_MANIFEST)) {
        Ok(doc) => doc,
        Err(e) => unreachable!("member manifest parses: {e}"),
    };
    // Touch the spans so the bench covers extraction, not just parsing.
    let _ = root
        .get("workspace")
        .and_then(|item| item.as_table())
        .and_then(|t| t.get("members"))
        .and_then(|item| item.as_value())
        .and_then(toml_edit::Value::span);
    let _ = member
        .get("package")
        .and_then(|item| item.as_table())
        .and_then(|t| t.get("version"))
        .and_then(|item| item.as_value())
        .and_then(toml_edit::Value::span);
}

fn bench_manifest_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("eco-cargo");
    group.throughput(criterion::Throughput::Bytes(
        u64::try_from(ROOT_MANIFEST.len() + MEMBER_MANIFEST.len()).unwrap_or(u64::MAX),
    ));
    group.bench_function("parse_pair_with_spans", |b| {
        b.iter(parse_pair);
    });
    group.finish();
}

/// Criterion harness entry (the `criterion_group!` expansion, spelled out
/// so the generated `pub fn` does not trip `missing_docs`).
fn benches() {
    let mut criterion: Criterion = Criterion::default().configure_from_args();
    bench_manifest_parse(&mut criterion);
}

fn main() {
    benches();
    Criterion::default().configure_from_args().final_summary();
}
