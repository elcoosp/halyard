//! Bench: npm `package.json` scan (§6.4 hot path).
//!
//! Measures the hand-written span scanner over a representative manifest
//! (deps in all four fields + facets + publishConfig). Budget row: §14.1
//! "Plan with 200 new commits, 40 crates"; formal budgets arm at M2.6.

use criterion::{black_box, criterion_group, criterion_main, Criterion};

const PACKAGE_JSON: &str = r#"{
  "name": "@halyard/engine",
  "version": "1.4.2",
  "private": false,
  "publishConfig": {
    "access": "public",
    "provenance": true
  },
  "dependencies": {
    "@halyard/core": "workspace:^",
    "react": "catalog:",
    "lodash": "catalog:std",
    "picomatch": "^4.0.2"
  },
  "devDependencies": {
    "@napi-rs/cli": "^2.44.0",
    "typescript": "5.6.2"
  },
  "peerDependencies": {
    "react": ">=18 <20"
  },
  "optionalDependencies": {
    "fsevents": "^2.3.3"
  },
  "napi": {
    "binaryName": "engine"
  },
  "workspaces": [
    "packages/*"
  ]
}"#;

fn scan() {
    let doc = halyard_eco_npm::json_span::JsonDoc::parse(black_box(PACKAGE_JSON.as_bytes()))
        .expect("package.json parses");
    // Touch spans + a value so extraction is covered.
    let _ = doc.get_path(&["dependencies", "@halyard/core"]).map(|v| v.span);
    let _ = doc.get("napi").is_some();
}

fn bench_json_scan(c: &mut Criterion) {
    let mut group = c.benchmark_group("eco-npm");
    group.throughput(criterion::Throughput::Bytes(PACKAGE_JSON.len() as u64));
    group.bench_function("scan_package_json", |b| {
        b.iter(scan);
    });
    group.finish();
}

criterion_group!(benches, bench_json_scan);
criterion_main!(benches);
