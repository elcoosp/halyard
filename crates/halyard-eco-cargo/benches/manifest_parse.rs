//! Bench: Cargo manifest parse + span extraction (§6.2 hot path).
//!
//! Measures `toml_edit` parse + dependency/inheritance extraction over a
//! representative workspace root and a member manifest with the full
//! §6.3 surface. Budget row: §14.1 "Plan with 200 new commits, 40 crates"
//! (discovery is its front half); formal budgets arm at M2.6.

use criterion::{black_box, criterion_group, criterion_main, Criterion};

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
    let root =
        toml_edit::ImDocument::parse(black_box(ROOT_MANIFEST)).expect("root manifest parses");
    let member =
        toml_edit::ImDocument::parse(black_box(MEMBER_MANIFEST)).expect("member manifest parses");
    // Touch the spans so the bench covers extraction, not just parsing.
    let _ = root
        .get("workspace")
        .and_then(|item| item.as_table())
        .and_then(|t| t.get("members"))
        .and_then(|item| item.as_value())
        .and_then(|v| v.span());
    let _ = member
        .get("package")
        .and_then(|item| item.as_table())
        .and_then(|t| t.get("version"))
        .and_then(|item| item.as_value())
        .and_then(|v| v.span());
}

fn bench_manifest_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("eco-cargo");
    group.throughput(criterion::Throughput::Bytes(
        (ROOT_MANIFEST.len() + MEMBER_MANIFEST.len()) as u64,
    ));
    group.bench_function("parse_pair_with_spans", |b| {
        b.iter(parse_pair);
    });
    group.finish();
}

criterion_group!(benches, bench_manifest_parse);
criterion_main!(benches);
