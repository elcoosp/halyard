//! The `BitbucketRestForge` contract (M7 step 3, ADR-0051): every
//! `Forge` method driven over a scripted `MemoryTransport` with
//! Bitbucket-shaped payloads (shapes verified in
//! `docs/research/bitbucket-rest-forge.md` against Atlassian's
//! documented REST 2.0 reference). The assertions pin the wire
//! contract: request shapes, the §11.4 idempotence values, the
//! `Authorization: Bearer` auth and the §11.5 call log.

use std::fmt::Write as _;

use halyard_forge::transport::{HttpResponse, MemoryTransport};
use halyard_forge::{
    ApiCommit, BranchName, CallRecord, Forge, ForgeError, MachineInput, PrSpec, RateInfo,
    ReleaseSpec, run_release_machine,
};
use halyard_forge_bitbucket::{BitbucketEndpoint, BitbucketRestForge};

const REPO: &str = "octo/halyard-demo";
const BRANCH: &str = "halyard/release/main";

fn forge() -> (BitbucketRestForge<MemoryTransport>, MemoryTransport) {
    let transport = MemoryTransport::new();
    let forge = BitbucketRestForge::with_transport(endpoint(), "smoke-token", transport.clone());
    (forge, transport)
}

fn endpoint() -> BitbucketEndpoint {
    BitbucketEndpoint::new(REPO)
        .unwrap_or_else(|error| unreachable!("static endpoint is valid: {error}"))
}

fn branch() -> BranchName {
    BranchName::new(BRANCH).unwrap_or_else(|error| unreachable!("static branch is valid: {error}"))
}

fn spec(title: &str, body: &str) -> PrSpec {
    PrSpec {
        branch: branch(),
        title: title.to_owned(),
        body: body.to_owned(),
    }
}

/// A Bitbucket-shaped pull body (research §2: `id`, `title`,
/// `description`, `links.html.href`, `source.branch.name`).
fn pull(id: u64, source_branch: &str, title: &str, description: &str) -> String {
    serde_json::json!({
        "id": id,
        "title": title,
        "description": description,
        "state": "open",
        "links": { "html": { "href": format!("https://bitbucket.org/{REPO}/pullrequests/{id}") } },
        "source": { "branch": { "name": source_branch } },
    })
    .to_string()
}

fn response(status: u16, body: &str) -> HttpResponse {
    HttpResponse {
        status,
        headers: Vec::new(),
        body: body.to_owned(),
    }
}

fn response_with_header(status: u16, header: (&str, &str), body: &str) -> HttpResponse {
    HttpResponse {
        status,
        headers: vec![(header.0.to_owned(), header.1.to_owned())],
        body: body.to_owned(),
    }
}

fn empty_list() -> String {
    "[]".to_owned()
}

/// A tag body (research §3: `name` + `links.html.href`; the release
/// marker on this surface is the tag).
fn tag(name: &str) -> String {
    serde_json::json!({
        "type": "tag",
        "name": name,
        "message": "Added tag for changeset f85de28",
        "links": { "html": { "href": "https://bitbucket.org/octo/halyard-demo/commits/tag/3.8" } },
        "target": { "hash": "f85de28eae32e7d3064b1a1321309071bbaaa069" },
    })
    .to_string()
}

#[test]
fn find_on_empty_forge_is_none_with_one_get() {
    let (forge, transport) = forge();
    transport.enqueue(response(200, &empty_list()));
    let found = forge
        .find_release_pr(&branch())
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert!(found.is_none(), "not-found is a value (§11.4)");
    let sent = transport.sent();
    assert_eq!(sent.len(), 1, "one GET, nothing else");
    let request = sent.first().unwrap_or_else(|| unreachable!("one request"));
    assert_eq!(request.method, "GET");
    assert!(
        request.url.starts_with(
            "https://api.bitbucket.org/2.0/repositories/octo/halyard-demo/pullrequests"
        ),
        "the documented pull-list path (research §1): {}",
        request.url
    );
    assert!(
        request.url.contains("state%20%3D%20%22OPEN%22"),
        "the q filter pins the OPEN state (research §2): {}",
        request.url
    );
    assert!(
        request
            .url
            .contains("source.branch.name%20%3D%20%22halyard%2Frelease%2Fmain%22"),
        "the q filter rides the documented dotted-path grammar (research §2): {}",
        request.url
    );
    assert_eq!(
        request.header("Authorization"),
        Some("Bearer smoke-token"),
        "the documented OAuth 2.0 Bearer scheme (research §1)"
    );
    assert!(
        request
            .header("User-Agent")
            .is_some_and(|ua| ua.starts_with("halyard/")),
        "the adapter's UA convention"
    );
}

#[test]
fn empty_token_fails_closed_before_any_call() {
    let transport = MemoryTransport::new();
    let forge = BitbucketRestForge::with_transport(endpoint(), "", transport.clone());
    let error = forge
        .find_release_pr(&branch())
        .expect_err("an empty token never reaches the wire");
    assert_eq!(error.code(), "E-FORGE-AUTH");
    assert!(
        error.to_string().contains("BITBUCKET_TOKEN"),
        "the decline names the lane's token grammar: {error}"
    );
    assert!(
        transport.sent().is_empty(),
        "the refusal happens before any call"
    );
}

#[test]
fn upsert_creates_then_rewrites_in_place() {
    let (forge, transport) = forge();
    // Pass 1: no open pull — the create (POST with the explicit
    // destination, research §2).
    transport.enqueue(response(200, &empty_list()));
    transport.enqueue(response(
        201,
        &pull(7, BRANCH, "chore: release v0.1.0", "the body"),
    ));
    let created = forge
        .upsert_pr(&spec("chore: release v0.1.0", "the body"))
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert_eq!(created.number, 7);
    assert_eq!(
        created.url, "https://bitbucket.org/octo/halyard-demo/pullrequests/7",
        "the pull's own web URL rides (research §2)"
    );
    // Pass 2: the pull is found — the rewrite (PUT, title/description
    // only — never a branch or state change, research §2).
    transport.enqueue(response(
        200,
        &format!("[{}]", pull(7, BRANCH, "old title", "old body")),
    ));
    transport.enqueue(response(
        200,
        &pull(7, BRANCH, "chore: release v0.1.1", "the new body"),
    ));
    let updated = forge
        .upsert_pr(&spec("chore: release v0.1.1", "the new body"))
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert_eq!(updated.number, 7);
    assert_eq!(updated.body, "the new body");
    let sent = transport.sent();
    let create = sent
        .get(1)
        .unwrap_or_else(|| unreachable!("the create POST"));
    assert_eq!(create.method, "POST");
    assert!(
        create
            .url
            .ends_with("/repositories/octo/halyard-demo/pullrequests"),
        "the create posts to the repository's pullrequests: {}",
        create.url
    );
    let create_body: serde_json::Value = serde_json::from_str(create.body.as_deref().unwrap_or(""))
        .unwrap_or_else(|error| unreachable!("the scripted body parses: {error}"));
    assert_eq!(
        create_body.get("title"),
        Some(&serde_json::json!("chore: release v0.1.0"))
    );
    assert_eq!(
        create_body
            .get("source")
            .and_then(|s| s.get("branch"))
            .and_then(|b| b.get("name")),
        Some(&serde_json::json!(BRANCH)),
        "the source rides the release branch (research §2)"
    );
    assert_eq!(
        create_body
            .get("destination")
            .and_then(|d| d.get("branch"))
            .and_then(|b| b.get("name")),
        Some(&serde_json::json!("main")),
        "the destination is the §11.2 inverse mapping, sent explicitly (research §2)"
    );
    let rewrite = sent
        .get(3)
        .unwrap_or_else(|| unreachable!("the rewrite PUT"));
    assert_eq!(rewrite.method, "PUT");
    assert!(
        rewrite.url.ends_with("/pullrequests/7"),
        "the rewrite addresses the found id: {}",
        rewrite.url
    );
    let rewrite_body: serde_json::Value =
        serde_json::from_str(rewrite.body.as_deref().unwrap_or(""))
            .unwrap_or_else(|error| unreachable!("the scripted body parses: {error}"));
    assert!(
        rewrite_body.get("source").is_none(),
        "a rewrite never touches branches"
    );
    assert_eq!(
        rewrite_body.get("description"),
        Some(&serde_json::json!("the new body")),
        "the notes ride description (research §2)"
    );
}

#[test]
fn non_release_branch_fails_closed() {
    let (forge, transport) = forge();
    let foreign = PrSpec {
        branch: BranchName::new("feature/topic").unwrap_or_else(|e| unreachable!("valid: {e}")),
        title: "t".to_owned(),
        body: "b".to_owned(),
    };
    let error = forge
        .upsert_pr(&foreign)
        .expect_err("a foreign branch shape is a caller bug, never a guess");
    assert_eq!(error.code(), "E-FORGE-REQUEST");
    assert!(
        transport.sent().is_empty(),
        "the refusal happens before any call"
    );
}

#[test]
fn close_reports_the_not_found_value_then_uses_the_verified_decline_shape() {
    let (forge, transport) = forge();
    // Not-found is a value: no decline POST for an absent pull (§11.4,
    // I1).
    transport.enqueue(response(200, &empty_list()));
    let closed = forge
        .close_pr(&branch())
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert!(!closed, "the absent pull is the not-found value");
    // The verified close verb: the decline POST (research §8), bodyless.
    transport.enqueue(response(200, &format!("[{}]", pull(7, BRANCH, "t", "b"))));
    transport.enqueue(response(200, &pull(7, BRANCH, "t", "b")));
    let closed = forge
        .close_pr(&branch())
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert!(closed, "the open pull declines (§11.1 step 3)");
    let sent = transport.sent();
    let decline = sent
        .last()
        .unwrap_or_else(|| unreachable!("the decline POST"));
    assert_eq!(decline.method, "POST");
    assert!(
        decline.url.ends_with("/pullrequests/7/decline"),
        "the documented decline endpoint (research §8): {}",
        decline.url
    );
    assert!(
        decline.body.is_none(),
        "the decline POST carries no body (research §8)"
    );
}

#[test]
fn release_marker_probes_the_tag_and_reports_the_existed_shape() {
    let (forge, transport) = forge();
    // release_exists: the tag probe's 404 is the documented not-found
    // value (research §3).
    transport.enqueue(response(
        404,
        r#"{"type":"error","error":{"message":"Resource not found"}}"#,
    ));
    let exists = forge
        .release_exists("v0.1.0")
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert!(!exists, "an absent tag is the not-found value");
    // create_release over a present tag: the existed shape with the
    // tag's web URL (research §3 — the marker is verifiably in place).
    transport.enqueue(response(200, &tag("v0.1.0")));
    let release = forge
        .create_release(&ReleaseSpec {
            tag: "v0.1.0".to_owned(),
            notes: "the notes".to_owned(),
        })
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert!(!release.created, "a GET never creates (I1)");
    assert_eq!(release.tag, "v0.1.0");
    assert_eq!(
        release.url, "https://bitbucket.org/octo/halyard-demo/commits/tag/3.8",
        "the tag's own web URL rides (research §3)"
    );
    let sent = transport.sent();
    let probe = sent.last().unwrap_or_else(|| unreachable!("the tag probe"));
    assert!(
        probe.url.ends_with("/refs/tags/v0.1.0"),
        "the documented single-tag path (research §3): {}",
        probe.url
    );
}

#[test]
fn release_creation_declines_when_the_marker_is_absent() {
    let (forge, transport) = forge();
    transport.enqueue(response(
        404,
        r#"{"type":"error","error":{"message":"Resource not found"}}"#,
    ));
    let error = forge
        .create_release(&ReleaseSpec {
            tag: "v0.9.9".to_owned(),
            notes: String::new(),
        })
        .expect_err("no release object exists and the marker is absent (research §3)");
    assert_eq!(error.code(), "E-FORGE-UNSUPPORTED");
    match error {
        ForgeError::Unsupported { adapter } => {
            assert_eq!(adapter, "BitbucketRestForge");
        }
        other => unreachable!("the decline shape: {other:?}"),
    }
}

#[test]
fn rate_limit_snapshot_and_retry_after() {
    let (forge, transport) = forge();
    // The throttle status maps onto the rate-limit taxonomy, honoring
    // the deployment's retry-after (research §6).
    transport.enqueue(response_with_header(
        429,
        ("retry-after", "17"),
        r#"{"type":"error","error":{"message":"Rate limit exceeded"}}"#,
    ));
    let error = forge
        .find_release_pr(&branch())
        .expect_err("the scripted throttle surfaces");
    assert_eq!(error.code(), "E-FORGE-RATE-LIMIT");
    assert!(
        error.to_string().contains("17"),
        "the documented retry-after is honored: {error}"
    );
    // The quota snapshot records the documented limit header only — the
    // remaining count has no header on this surface and stays the
    // honest zero (research §6).
    transport.enqueue(response_with_header(
        200,
        ("x-ratelimit-limit", "1000"),
        &empty_list(),
    ));
    let _ = forge
        .find_release_pr(&branch())
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert_eq!(
        forge.rate_limit(),
        RateInfo {
            limit: 1000,
            remaining: 0
        },
        "the honest snapshot: the documented limit rides, the undocumented remaining stays zero"
    );
}

#[test]
fn commit_via_api_declines_naming_the_adapter() {
    let (forge, transport) = forge();
    let error = forge
        .commit_via_api(&ApiCommit {
            branch: branch(),
            message: "chore: release v0.1.0".to_owned(),
            files: Vec::new(),
        })
        .expect_err("the documented decline (research §5)");
    assert_eq!(error.code(), "E-FORGE-UNSUPPORTED");
    match error {
        ForgeError::Unsupported { adapter } => {
            assert_eq!(adapter, "BitbucketRestForge");
        }
        other => unreachable!("the decline shape: {other:?}"),
    }
    assert!(transport.sent().is_empty(), "the decline spends no call");
}

#[test]
fn comment_scan_walks_the_paginated_envelope_then_rewrites() {
    let (scan_forge, scan_transport) = forge();
    // A single short page carrying the marker: the scan rewrites in
    // place (PUT …/comments/{id}, research §4).
    scan_transport.enqueue(response(
        200,
        r#"{"size":1,"page":1,"pagelen":50,"values":[{"id":11,"content":{"raw":"before <!-- halyard:check v1 --> after"},"links":{"html":{"href":"https://bitbucket.org/octo/halyard-demo/pullrequests/7/comments/11"}}}]}"#,
    ));
    scan_transport.enqueue(response(
        200,
        r#"{"id":11,"content":{"raw":"rewritten <!-- halyard:check v1 -->"},"links":{"html":{"href":"https://bitbucket.org/octo/halyard-demo/pullrequests/7/comments/11"}}}"#,
    ));
    let comment = scan_forge
        .upsert_pr_comment(&halyard_forge::CommentSpec {
            pr_number: 7,
            marker: "<!-- halyard:check v1 -->".to_owned(),
            body: "rewritten <!-- halyard:check v1 -->".to_owned(),
        })
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert!(!comment.created, "the marker hit rewrites in place (I1)");
    assert_eq!(
        comment.url,
        "https://bitbucket.org/octo/halyard-demo/pullrequests/7/comments/11"
    );
    let sent = scan_transport.sent();
    let scan = sent.first().unwrap_or_else(|| unreachable!("the scan"));
    assert!(
        scan.url.contains("pullrequests/7/comments"),
        "the documented comment list path (research §4): {}",
        scan.url
    );
    assert!(
        scan.url.contains("pagelen=50"),
        "the documented pagination parameter rides (research §4): {}",
        scan.url
    );
    let rewrite = sent
        .get(1)
        .unwrap_or_else(|| unreachable!("the rewrite PUT"));
    assert_eq!(rewrite.method, "PUT");
    assert!(
        rewrite.url.ends_with("/pullrequests/7/comments/11"),
        "the rewrite addresses the found comment id: {}",
        rewrite.url
    );
    let rewrite_body: serde_json::Value =
        serde_json::from_str(rewrite.body.as_deref().unwrap_or(""))
            .unwrap_or_else(|error| unreachable!("the scripted body parses: {error}"));
    assert_eq!(
        rewrite_body
            .get("content")
            .and_then(|content| content.get("raw")),
        Some(&serde_json::json!("rewritten <!-- halyard:check v1 -->")),
        "the body rides content.raw (research §4)"
    );

    // The empty scan creates (POST, same content.raw shape).
    let (create_forge, create_transport) = forge();
    create_transport.enqueue(response(
        200,
        r#"{"size":0,"page":1,"pagelen":50,"values":[]}"#,
    ));
    create_transport.enqueue(response(
        201,
        r#"{"id":12,"content":{"raw":"<!-- halyard:check v1 -->"},"links":{"html":{"href":"https://bitbucket.org/octo/halyard-demo/pullrequests/7/comments/12"}}}"#,
    ));
    let comment = create_forge
        .upsert_pr_comment(&halyard_forge::CommentSpec {
            pr_number: 7,
            marker: "<!-- halyard:check v1 -->".to_owned(),
            body: "<!-- halyard:check v1 -->".to_owned(),
        })
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert!(comment.created, "the empty scan creates");
    let sent = create_transport.sent();
    let create = sent
        .last()
        .unwrap_or_else(|| unreachable!("the create POST"));
    assert_eq!(create.method, "POST");
    assert!(
        create.url.ends_with("/pullrequests/7/comments"),
        "the create posts to the pull's comments: {}",
        create.url
    );
}

#[test]
fn comment_scan_walks_a_full_page_before_the_short_one() {
    let (forge, transport) = forge();
    // Page 1 is a FULL page (pagelen comments, none carrying the
    // marker) — the walk advances to page 2, whose short page carries
    // the marker (research §4: the walk terminates on a short page).
    let mut full = String::from(r#"{"size":60,"page":1,"pagelen":50,"values":["#);
    for i in 0..50 {
        if i > 0 {
            full.push(',');
        }
        let _ = write!(
            &mut full,
            r#"{{"id":{},"content":{{"raw":"no marker here"}}}}"#,
            100 + i
        );
    }
    full.push_str("]}");
    transport.enqueue(response(200, &full));
    transport.enqueue(response(
        200,
        r#"{"size":60,"page":2,"pagelen":50,"values":[{"id":11,"content":{"raw":"<!-- halyard:check v1 -->"}}]}"#,
    ));
    transport.enqueue(response(
        200,
        r#"{"id":11,"content":{"raw":"<!-- halyard:check v1 -->"}}"#,
    ));
    let comment = forge
        .upsert_pr_comment(&halyard_forge::CommentSpec {
            pr_number: 9,
            marker: "<!-- halyard:check v1 -->".to_owned(),
            body: "<!-- halyard:check v1 -->".to_owned(),
        })
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert!(!comment.created);
    let sent = transport.sent();
    assert_eq!(sent.len(), 3, "scan page 1 + scan page 2 + the rewrite");
    assert!(
        sent.first()
            .unwrap_or_else(|| unreachable!("page 1"))
            .url
            .contains("page=1"),
        "the walk starts at page 1: {}",
        sent.first().unwrap_or_else(|| unreachable!("page 1")).url
    );
    assert!(
        sent.get(1)
            .unwrap_or_else(|| unreachable!("page 2"))
            .url
            .contains("page=2"),
        "the walk advanced to page 2: {}",
        sent.get(1).unwrap_or_else(|| unreachable!("page 2")).url
    );
}

#[test]
fn machine_propagate_and_prepare_passes_ride_the_adapter() {
    let (forge, transport) = forge();
    // The §11.1 step-2 pass: one pending release whose tag the git-side
    // phase already pushed — the marker probe answers the existed shape
    // (research §3), the machine reports ReleaseExisted and PROPAGATEs.
    transport.enqueue(response(200, &tag("conformance-v0.1.0")));
    let report = run_release_machine(
        &forge,
        &MachineInput {
            branch: branch(),
            pending_releases: vec![ReleaseSpec {
                tag: "conformance-v0.1.0".to_owned(),
                notes: "notes".to_owned(),
            }],
            release_pr: None,
            content_commit: None,
        },
    )
    .unwrap_or_else(|error| unreachable!("the scripted machine pass: {error}"));
    assert!(report.propagated, "pending releases gate the prepare phase");
    assert_eq!(report.effects.len(), 1, "one release row");
    let calls = forge.calls();
    assert_eq!(calls.len(), 1, "one tag probe, no PR half");
    let CallRecord { method, target } = calls
        .first()
        .unwrap_or_else(|| unreachable!("one call"))
        .clone();
    assert_eq!(method, "GET");
    assert!(
        target.contains("/refs/tags/conformance-v0.1.0"),
        "the tag probe is the release act on this surface: {target}"
    );
}

#[test]
fn server_side_failures_map_onto_the_taxonomy() {
    for (status, expected) in [
        (401_u16, "E-FORGE-AUTH"),
        (403, "E-FORGE-AUTH"),
        (400, "E-FORGE-REQUEST"),
        (409, "E-FORGE-CONFLICT"),
        (500, "E-FORGE-TRANSPORT"),
    ] {
        let (forge, transport) = forge();
        transport.enqueue(response(
            status,
            r#"{"type":"error","error":{"message":"the diagnosis"}}"#,
        ));
        let error = forge
            .find_release_pr(&branch())
            .expect_err("the scripted failure surfaces");
        assert_eq!(error.code(), expected, "status {status}");
        assert!(
            error.to_string().contains("the diagnosis"),
            "the server's own error.message rides the reason (research §6 of the error layout): \
             {error}"
        );
    }
}
