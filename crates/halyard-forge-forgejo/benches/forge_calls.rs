//! The REST adapter's in-memory path baseline (M7 step 2): request
//! building + transport dispatch (scripted) + response parsing for the
//! shapes the §11.1 machine drives. The network itself is never a
//! benchmark — the §11.5 budget is a *call-count* budget (2 on the pull
//! update path), which the contract tests assert; this bench documents
//! the adapter's own CPU overhead around each call.
//!
//! Run: `cargo bench -p halyard-forge-forgejo`

use std::fmt::Write as _;

use criterion::Criterion;
use halyard_forge::transport::{HttpResponse, MemoryTransport};
use halyard_forge::{BranchName, Forge, ForgeError, ReleaseSpec};
use halyard_forge_forgejo::{ForgejoEndpoint, ForgejoRestForge};

/// House-style fixture: the endpoint the bench probes.
fn endpoint() -> ForgejoEndpoint {
    ForgejoEndpoint::with_base("octo/halyard-demo", "https://codeberg.org/api/v1")
        .unwrap_or_else(|e| unreachable!("valid endpoint: {e}"))
}

fn branch() -> BranchName {
    BranchName::new("halyard/release/main").unwrap_or_else(|e| unreachable!("valid branch: {e}"))
}

/// A pull-list payload carrying `count` pull bodies (the find probe's
/// parse cost at realistic list sizes — the `head` filter usually
/// returns few, but the wire body is the list the repo's open pulls).
fn pull_list(count: usize) -> String {
    let mut out = String::from("[");
    for i in 0..count {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            r#"{{"number":{i},"title":"chore: something {i}","body":"a plausible release body","state":"open","html_url":"https://codeberg.org/octo/halyard-demo/pulls/{i}","head":{{"ref":"feature/{i}"}}}}"#
        );
    }
    out.push(']');
    out
}

/// Response fixture: status 200 + body.
fn ok(body: &str) -> HttpResponse {
    HttpResponse {
        status: 200,
        headers: Vec::new(),
        body: body.to_owned(),
    }
}

/// `find_release_pr` end-to-end over the scripted transport: request
/// build (headers, URL, query) + dispatch + list parse + head-ref
/// match — the update path's first call.
fn bench_find(c: &mut Criterion) {
    c.bench_function("rest/find_release_pr/64-open-pulls", |b| {
        b.iter_batched(
            || {
                let transport = MemoryTransport::new();
                transport.enqueue(ok(&pull_list(64)));
                let forge = ForgejoRestForge::with_transport(endpoint(), "t", transport);
                (forge, branch())
            },
            |(forge, branch)| {
                let found = forge.find_release_pr(&branch);
                debug_assert!(found.is_ok(), "the scripted find cannot fail");
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

/// The pull-update path: find + PATCH — the §11.4 two-call shape the
/// §11.5 budget pins.
fn bench_update_pull(c: &mut Criterion) {
    c.bench_function("rest/upsert_pr/find-then-patch", |b| {
        b.iter_batched(
            || {
                let transport = MemoryTransport::new();
                transport.enqueue(ok(&pull_list(1)));
                transport.enqueue(ok(&pull_json(41)));
                let forge = ForgejoRestForge::with_transport(endpoint(), "t", transport);
                let spec = halyard_forge::PrSpec {
                    branch: branch(),
                    title: "chore: release v0.1.0".to_owned(),
                    body: "## 🚀 Release plan".to_owned(),
                };
                (forge, spec)
            },
            |(forge, spec)| {
                let pr = forge.upsert_pr(&spec);
                debug_assert!(pr.is_ok(), "the scripted upsert cannot fail");
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

/// Release creation: GET-by-tag (404) + POST — the tag phase's per-pkg
/// exchange.
fn bench_create_release(c: &mut Criterion) {
    c.bench_function("rest/create_release/get-then-post", |b| {
        b.iter_batched(
            || {
                let transport = MemoryTransport::new();
                transport.enqueue(HttpResponse {
                    status: 404,
                    headers: Vec::new(),
                    body: r#"{"message":"Not Found"}"#.to_owned(),
                });
                transport.enqueue(ok(
                    r#"{"tag_name":"core-v0.5.0","html_url":"https://codeberg.org/octo/halyard-demo/releases/tag/core-v0.5.0","id":3}"#,
                ));
                let forge = ForgejoRestForge::with_transport(endpoint(), "t", transport);
                let spec = ReleaseSpec {
                    tag: "core-v0.5.0".to_owned(),
                    notes: "## Features\n\n- the painter walk".to_owned(),
                };
                (forge, spec)
            },
            |(forge, spec)| {
                let release = forge.create_release(&spec);
                debug_assert!(release.is_ok(), "the scripted creation cannot fail");
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

/// The `ForgeError` construction cost is on the failure path of every
/// call — pinned so error reasons stay cheap to build (they surface in
/// reports verbatim).
fn bench_error_construction(c: &mut Criterion) {
    c.bench_function("rest/error/transport-build", |b| {
        b.iter(|| {
            let error = ForgeError::transport("connection reset by peer during read");
            debug_assert_eq!(error.code(), "E-FORGE-TRANSPORT");
            drop(error);
        });
    });
}

/// One pull body (the update path's scripted response).
fn pull_json(number: u64) -> String {
    format!(
        r###"{{"number":{number},"title":"chore: release v0.1.0","body":"## 🚀 Release plan","state":"open","html_url":"https://codeberg.org/octo/halyard-demo/pulls/{number}","head":{{"ref":"halyard/release/main"}}}}"###
    )
}

fn main() {
    let mut criterion = Criterion::default();
    bench_find(&mut criterion);
    bench_update_pull(&mut criterion);
    bench_create_release(&mut criterion);
    bench_error_construction(&mut criterion);
    criterion.final_summary();
}
