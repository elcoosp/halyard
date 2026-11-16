//! Publish-auth benchmark (M5.3 step 1, ADR-0019): the per-run cost the
//! auth plumbing adds — one environment-table capture, the per-target
//! resolution the spawn effect performs before every tool call, and the
//! scrub pass over a realistic tool-error tail. All three must be
//! trivially cheap against a publish's network round trip: the capture
//! happens once, the resolution once per target, the scrub once per
//! surfaced failure.
//!
//! The OIDC group (M5.3 step 3, ADR-0031) measures the trusted-
//! publishing resolution over a scripted transport: the full exchange
//! path (id-token request + registry exchange + memo insert) and the
//! warm path (the per-target resolution the spawn effect performs once
//! the exchange is memoized — pure memory work, no network).

use std::collections::BTreeMap;

use std::sync::Arc;

use criterion::{BatchSize, Criterion};
use halyard_forge::{HttpResponse, MemoryTransport};
use halyard_publish::{
    EnvPublishAuth, OidcContext, OidcPublishAuth, PublishAuth, PublishTarget, scrub_secrets,
    variables_for,
};

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

fn bench_oidc(c: &mut Criterion) {
    let mut group = c.benchmark_group("publish_oidc");
    let targets = targets();

    // One scripted happy path per fresh provider: the first crates.io
    // target performs the documented two-call exchange, the rest ride
    // the memo (and the npm targets the honest absence). This measures
    // the full cold resolution.
    group.bench_function("exchange_and_resolve_14", |b| {
        b.iter_batched(
            || {
                let transport = MemoryTransport::new();
                transport.enqueue(HttpResponse {
                    status: 200,
                    headers: Vec::new(),
                    body: r#"{"value":"gha-jwt-for-the-bench"}"#.to_owned(),
                });
                transport.enqueue(HttpResponse {
                    status: 200,
                    headers: Vec::new(),
                    body: r#"{"token":"exchanged-token-for-the-bench"}"#.to_owned(),
                });
                OidcPublishAuth::new(
                    EnvPublishAuth::default(),
                    Some(OidcContext {
                        request_url: "https://actions.example.test/oidc?audience=".to_owned(),
                        request_token: "request-token-for-the-bench".to_owned(),
                    }),
                    Arc::new(transport),
                    "https://crates.io".to_owned(),
                    true,
                )
            },
            |auth| {
                for target in &targets {
                    let _resolved = PublishAuth::credentials(&auth, target);
                }
            },
            BatchSize::SmallInput,
        );
    });

    // The warm path: the exchange memoized once, then the per-target
    // resolution the spawn effect performs before every tool call —
    // memo hits and the honest absent rows, zero network.
    group.bench_function("memoized_resolve_14", |b| {
        let transport = MemoryTransport::new();
        transport.enqueue(HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: r#"{"value":"gha-jwt-for-the-bench"}"#.to_owned(),
        });
        transport.enqueue(HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: r#"{"token":"exchanged-token-for-the-bench"}"#.to_owned(),
        });
        let auth = OidcPublishAuth::new(
            EnvPublishAuth::default(),
            Some(OidcContext {
                request_url: "https://actions.example.test/oidc?audience=".to_owned(),
                request_token: "request-token-for-the-bench".to_owned(),
            }),
            Arc::new(transport),
            "https://crates.io".to_owned(),
            true,
        );
        if let Some(first) = targets.first() {
            let _warm = PublishAuth::credentials(&auth, first);
        }
        b.iter(|| {
            for target in &targets {
                let _resolved = PublishAuth::credentials(&auth, target);
            }
        });
    });

    group.finish();
}

fn main() {
    let mut criterion = Criterion::default();
    bench_auth(&mut criterion);
    bench_oidc(&mut criterion);
}
