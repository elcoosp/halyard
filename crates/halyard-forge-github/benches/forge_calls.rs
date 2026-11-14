//! The REST adapter's in-memory path baseline (M4.1b): request
//! building + transport dispatch (scripted) + response parsing for the
//! shapes the §11.1 machine drives. The network itself is never a
//! benchmark — the §11.5 budget is a *call-count* budget (≤ 2 on the
//! update path), which the contract tests assert; this bench documents
//! the adapter's own CPU overhead around each call.
//!
//! Run: `cargo bench -p halyard-forge-github`

use std::fmt::Write as _;

use criterion::Criterion;
use halyard_forge::transport::{HttpResponse, MemoryTransport};
use halyard_forge::{BranchName, Forge, ForgeError, ReleaseSpec};
use halyard_forge_github::{ForgeEndpoint, GitHubRestForge};

/// House-style fixture: the branch the bench probes.
fn endpoint() -> ForgeEndpoint {
    ForgeEndpoint::with_base("octo", "halyard-demo", "https://api.github.com")
        .unwrap_or_else(|e| unreachable!("valid endpoint: {e}"))
}

fn branch() -> BranchName {
    BranchName::new("halyard/release/main").unwrap_or_else(|e| unreachable!("valid branch: {e}"))
}

/// A PR-list payload carrying `count` PR bodies (the find probe's parse
/// cost at realistic list sizes — the head filter usually returns few,
/// but the wire body is the list the repo's open PRs).
fn pr_list(count: usize) -> String {
    let mut out = String::from("[");
    for i in 0..count {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            r#"{{"number":{i},"title":"chore: something {i}","body":"a plausible release body","html_url":"https://github.com/octo/halyard-demo/pull/{i}","head":{{"ref":"feature/{i}"}}}}"#
        );
    }
    out.push(']');
    out
}

/// Response fixture: status 200 + body.
fn ok(body: &str) -> HttpResponse {
    HttpResponse {
        status: 200,
        headers: vec![("etag".to_owned(), "\"v1\"".to_owned())],
        body: body.to_owned(),
    }
}

/// `find_release_pr` end-to-end over the scripted transport: request
/// build (headers, URL, query) + dispatch + list parse + head match —
/// the update path's first call.
fn bench_find(c: &mut Criterion) {
    c.bench_function("rest/find_release_pr/64-open-prs", |b| {
        b.iter_batched(
            || {
                let transport = MemoryTransport::new();
                transport.enqueue(ok(&pr_list(64)));
                let forge = GitHubRestForge::with_transport(endpoint(), "t", transport);
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

/// The conditional re-find: the `ETag` cache answers 304 — the shape the
/// §11.1 no-op path rides (research §3: a 304 does not count against
/// the primary limit).
fn bench_conditional_refind(c: &mut Criterion) {
    c.bench_function("rest/find_release_pr/conditional-304", |b| {
        b.iter_batched(
            || {
                let transport = MemoryTransport::new();
                transport.enqueue(ok(&pr_list(1)));
                transport.enqueue(HttpResponse {
                    status: 304,
                    headers: Vec::new(),
                    body: String::new(),
                });
                let forge = GitHubRestForge::with_transport(endpoint(), "t", transport);
                let branch = branch();
                let _ = forge.find_release_pr(&branch);
                (forge, branch)
            },
            |(forge, branch)| {
                let found = forge.find_release_pr(&branch);
                debug_assert!(found.is_ok(), "the scripted re-find cannot fail");
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
                    body: "{\"message\":\"Not Found\"}".to_owned(),
                });
                transport.enqueue(ok(
                    "{\"html_url\":\"https://github.com/octo/halyard-demo/releases/tag/core-v0.5.0\"}",
                ));
                let forge = GitHubRestForge::with_transport(endpoint(), "t", transport);
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

fn main() {
    let mut criterion = Criterion::default();
    bench_find(&mut criterion);
    bench_conditional_refind(&mut criterion);
    bench_create_release(&mut criterion);
    bench_error_construction(&mut criterion);
    criterion.final_summary();
}
