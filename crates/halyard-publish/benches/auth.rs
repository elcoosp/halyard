//! Publish-auth benchmark (M5.3 step 1, ADR-0019): the per-run cost the
//! auth plumbing adds — one environment-table capture, the per-target
//! resolution the spawn effect performs before every tool call, and the
//! scrub pass over a realistic tool-error tail. All three must be
//! trivially cheap against a publish's network round trip: the capture
//! happens once, the resolution once per target, the scrub once per
//! surfaced failure.

use std::collections::BTreeMap;

use criterion::{BatchSize, Criterion};
use halyard_publish::{EnvPublishAuth, PublishAuth, PublishTarget, scrub_secrets, variables_for};

/// One target per documented grammar shape.
fn targets() -> Vec<PublishTarget> {
    let mut targets = Vec::new();
    for index in 0..14 {
        targets.push(PublishTarget {
            package: format!("p{index}"),
            version: "0.2.0".to_owned(),
            ecosystem: if index % 2 == 0 { "cargo" } else { "npm" }.to_owned(),
            registry: if index % 2 == 0 { "crates.io" } else { "npm" }.to_owned(),
            manifest_dir: format!("crates/p{index}"),
            manifest: format!("crates/p{index}/Cargo.toml"),
        });
    }
    targets
}

fn table() -> BTreeMap<String, String> {
    [
        ("CARGO_REGISTRY_TOKEN", "cargo-token-value-for-the-bench"),
        ("NPM_TOKEN", "npm-token-value-for-the-bench"),
        ("NODE_AUTH_TOKEN", "node-token-value-for-the-bench"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect()
}

/// A realistic tool failure tail (the stderr the scrub pass runs over:
/// ~1 KiB of diagnosis with the token echoed twice).
fn failure_tail(token: &str) -> String {
    let mut lines = Vec::new();
    for index in 0..40 {
        lines.push(format!("info[{index}]: packaging {index} files"));
    }
    lines.push(format!(
        "warning: verifying upload against {token}: retrying"
    ));
    lines.push(format!(
        "error: upload rejected — token {token} is not authorized"
    ));
    lines.join("\n")
}

fn bench_auth(c: &mut Criterion) {
    let mut group = c.benchmark_group("publish_auth");

    group.bench_function("table_capture_14", |b| {
        b.iter_batched(table, EnvPublishAuth::from_table, BatchSize::SmallInput);
    });

    let auth = EnvPublishAuth::from_table(table());
    let targets = targets();
    group.bench_function("credentials_14_targets", |b| {
        b.iter(|| {
            for target in &targets {
                let _resolved = PublishAuth::credentials(&auth, target);
                let _grammar = variables_for(target);
            }
        });
    });

    let token = "cargo-token-value-for-the-bench";
    let tail = failure_tail(token);
    let secrets = PublishAuth::secrets(&auth);
    let refs: Vec<&str> = secrets.iter().map(String::as_str).collect();
    group.bench_function("scrub_1k_tail", |b| {
        b.iter(|| {
            let _scrubbed = scrub_secrets(&tail, &refs);
        });
    });

    group.finish();
}

fn main() {
    let mut criterion = Criterion::default();
    bench_auth(&mut criterion);
}
