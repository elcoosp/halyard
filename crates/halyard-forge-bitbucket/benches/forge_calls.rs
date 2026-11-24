//! The REST adapter's in-memory path baseline (M7 step 3): request
//! building + transport dispatch (scripted) + response parsing for the
//! shapes the §11.1 machine drives. The network itself is never a
//! benchmark — the §11.5 budget is a *call-count* budget (2 on the pull
//! update path), which the contract tests assert; this bench documents
//! the adapter's own CPU overhead around each call.
//!
//! Run: `cargo bench -p halyard-forge-bitbucket`

use std::fmt::Write as _;

use criterion::Criterion;
use halyard_forge::transport::{HttpResponse, MemoryTransport};
use halyard_forge::{BranchName, Forge, ForgeError, ReleaseSpec};
use halyard_forge_bitbucket::{BitbucketEndpoint, BitbucketRestForge};

/// House-style fixture: the endpoint the bench probes.
fn endpoint() -> BitbucketEndpoint {
    BitbucketEndpoint::with_base("octo/halyard-demo", "https://api.bitbucket.org/2.0")
        .unwrap_or_else(|e| unreachable!("valid endpoint: {e}"))
}

fn branch() -> BranchName {
    BranchName::new("halyard/release/main").unwrap_or_else(|e| unreachable!("valid branch: {e}"))
}

/// A pull-list payload carrying `count` pull bodies (the find probe's
/// parse cost at realistic list sizes — the `q` filter usually returns
/// few, but the wire body is the list the repo's open pulls).
fn pull_list(count: usize) -> String {
    let mut out = String::from("[");
    for i in 0..count {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            "{}",
            serde_json::json!({
                "id": i,
                "title": format!("chore: something {i}"),
                "description": "a plausible release body",
                "state": "open",
                "links": { "html": { "href": format!("https://bitbucket.org/octo/halyard-demo/pullrequests/{i}") } },
                "source": { "branch": { "name": format!("feature/{i}") } },
            })
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
/// build (headers, URL, `q` encoding) + dispatch + list parse +
/// source-branch match — the update path's first call.
fn bench_find(c: &mut Criterion) {
    c.bench_function("rest/find_release_pr/64-open-pulls", |b| {
        b.iter_batched(
            || {
                let transport = MemoryTransport::new();
                transport.enqueue(ok(&pull_list(64)));
                let forge = BitbucketRestForge::with_transport(endpoint(), "t", transport);
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

/// The pull-update path: find + PUT — the §11.4 two-call shape the
/// §11.5 budget pins.
fn bench_update_pull(c: &mut Criterion) {
    c.bench_function("rest/upsert_pr/find-then-put", |b| {
        b.iter_batched(
            || {
                let transport = MemoryTransport::new();
                transport.enqueue(ok(&pull_list(1)));
                transport.enqueue(ok(&pull_json(41)));
                let forge = BitbucketRestForge::with_transport(endpoint(), "t", transport);
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

/// The release act on this surface: the tag probe (the marker in
/// place, research §3) — the tag phase's per-pkg exchange, no POST.
fn bench_release_probe(c: &mut Criterion) {
    c.bench_function("rest/create_release/tag-probe", |b| {
        b.iter_batched(
            || {
                let transport = MemoryTransport::new();
                transport.enqueue(ok(
                    r#"{"type":"tag","name":"core-v0.5.0","links":{"html":{"href":"https://bitbucket.org/octo/halyard-demo/commits/tag/core-v0.5.0"}}}"#,
                ));
                let forge = BitbucketRestForge::with_transport(endpoint(), "t", transport);
                let spec = ReleaseSpec {
                    tag: "core-v0.5.0".to_owned(),
                    notes: "## Features\n\n- the painter walk".to_owned(),
                };
                (forge, spec)
            },
            |(forge, spec)| {
                let release = forge.create_release(&spec);
                debug_assert!(release.is_ok(), "the scripted probe cannot fail");
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
fn pull_json(id: u64) -> String {
    serde_json::json!({
        "id": id,
        "title": "chore: release v0.1.0",
        "description": "## 🚀 Release plan",
        "state": "open",
        "links": { "html": { "href": format!("https://bitbucket.org/octo/halyard-demo/pullrequests/{id}") } },
        "source": { "branch": { "name": "halyard/release/main" } },
    })
    .to_string()
}

fn main() {
    let mut criterion = Criterion::default();
    bench_find(&mut criterion);
    bench_update_pull(&mut criterion);
    bench_release_probe(&mut criterion);
    bench_error_construction(&mut criterion);
    criterion.final_summary();
}
