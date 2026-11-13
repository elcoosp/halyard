//! Bench: npm `package.json` scan (§6.4 hot path).
//!
//! Measures the hand-written span scanner over a representative manifest
//! (deps in all four fields + facets + publishConfig). Budget row: §14.1
//! "Plan with 200 new commits, 40 crates"; formal budgets arm at M2.6.

use criterion::Criterion;

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
    // The fixture is a constant that must parse; a parse failure aborts
    // the bench loudly (bench fns are not `#[test]`, so the test-lint
    // allowance does not reach them).
    let doc = match halyard_eco_npm::json_span::JsonDoc::parse(std::hint::black_box(
        PACKAGE_JSON.as_bytes(),
    )) {
        Ok(doc) => doc,
        Err(e) => unreachable!("bench fixture must parse: {e}"),
    };
    // Touch spans + a value so extraction is covered.
    let _ = doc
        .get_path(&["dependencies", "@halyard/core"])
        .map(|v| v.span);
    let _ = doc.get("napi").is_some();
}

fn bench_json_scan(c: &mut Criterion) {
    let mut group = c.benchmark_group("eco-npm");
    group.throughput(criterion::Throughput::Bytes(
        u64::try_from(PACKAGE_JSON.len()).unwrap_or(u64::MAX),
    ));
    group.bench_function("scan_package_json", |b| {
        b.iter(scan);
    });
    group.finish();
}

/// Criterion harness entry (the `criterion_group!` expansion, spelled out
/// so the generated `pub fn` does not trip `missing_docs`).
fn benches() {
    let mut criterion: Criterion = Criterion::default().configure_from_args();
    bench_json_scan(&mut criterion);
}

fn main() {
    benches();
    Criterion::default().configure_from_args().final_summary();
}
