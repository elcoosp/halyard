//! The `ForgejoRestForge` contract (M7 step 2, ADR-0050): every `Forge`
//! method driven over a scripted `MemoryTransport` with Forgejo-shaped
//! payloads (shapes verified in `docs/research/forgejo-rest-forge.md`
//! against the live `/api/v1` specification). The assertions pin the
//! wire contract: request shapes, the §11.4 idempotence values, the
//! `Authorization: token` auth and the §11.5 call log.

use halyard_forge::transport::{HttpResponse, MemoryTransport};
use halyard_forge::{
    ApiCommit, BranchName, CallRecord, Forge, ForgeError, MachineInput, PrSpec, RateInfo,
    ReleaseSpec, run_release_machine,
};
use halyard_forge_forgejo::{ForgejoEndpoint, ForgejoRestForge};

const REPO: &str = "octo/halyard-demo";
const BRANCH: &str = "halyard/release/main";

fn forge() -> (ForgejoRestForge<MemoryTransport>, MemoryTransport) {
    let transport = MemoryTransport::new();
    let forge = ForgejoRestForge::with_transport(endpoint(), "smoke-token", transport.clone());
    (forge, transport)
}

fn endpoint() -> ForgejoEndpoint {
    ForgejoEndpoint::new(REPO)
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

fn pull(number: u64, head_ref: &str, title: &str, body: &str) -> String {
    let head = json_str(head_ref);
    let title = json_str(title);
    let body = json_str(body);
    format!(
        r#"{{"number":{number},"title":{title},"body":{body},"state":"open","html_url":"https://codeberg.org/{REPO}/pulls/{number}","head":{{"ref":{head}}}}}"#
    )
}

fn json_str(text: &str) -> String {
    serde_json::to_string(text)
        .unwrap_or_else(|error| unreachable!("plain string serializes: {error}"))
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
        request
            .url
            .starts_with("https://codeberg.org/api/v1/repos/octo/halyard-demo/pulls"),
        "the documented pull-list path: {}",
        request.url
    );
    assert!(request.url.contains("state=open"), "{}", request.url);
    assert!(
        request.url.contains(&format!("head={BRANCH}")),
        "the head filter rides the query verbatim (research §2): {}",
        request.url
    );
    assert_eq!(
        request.header("Authorization"),
        Some("token smoke-token"),
        "the documented auth header — `token` prefixed (research §1)"
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
    let forge = ForgejoRestForge::with_transport(endpoint(), "", transport.clone());
    let error = forge
        .find_release_pr(&branch())
        .expect_err("an empty token never reaches the wire");
    assert_eq!(error.code(), "E-FORGE-AUTH");
    assert!(transport.sent().is_empty(), "no call was made");
}

#[test]
fn upsert_creates_then_rewrites_in_place() {
    let (forge, transport) = forge();
    transport.enqueue(response(200, &empty_list()));
    transport.enqueue(response(
        201,
        &pull(7, BRANCH, "chore: release v0.1.0", "body-one"),
    ));
    let opened = forge
        .upsert_pr(&spec("chore: release v0.1.0", "body-one"))
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert_eq!(opened.number, 7);
    assert_eq!(
        opened.url, "https://codeberg.org/octo/halyard-demo/pulls/7",
        "the PullRequest's html_url is the PR URL (research §2)"
    );
    transport.enqueue(response(
        200,
        &format!("[{}]", pull(7, BRANCH, "old", "old")),
    ));
    transport.enqueue(response(
        200,
        &pull(7, BRANCH, "chore: release v0.1.0", "body-two"),
    ));
    let updated = forge
        .upsert_pr(&spec("chore: release v0.1.0", "body-two"))
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert_eq!(updated.number, 7, "same branch ⇒ same PR (I1)");
    assert_eq!(updated.body, "body-two");
    let sent = transport.sent();
    let create = sent.get(1).unwrap_or_else(|| unreachable!("recorded"));
    assert_eq!(create.method, "POST");
    let create_body: serde_json::Value =
        serde_json::from_str(create.body.as_deref().unwrap_or_default())
            .unwrap_or_else(|error| unreachable!("the adapter sends valid JSON: {error}"));
    assert_eq!(
        create_body.get("head").and_then(|v| v.as_str()),
        Some(BRANCH)
    );
    assert_eq!(
        create_body.get("base").and_then(|v| v.as_str()),
        Some("main"),
        "the §11.2 inverse mapping: halyard/release/main targets main"
    );
    assert_eq!(
        create_body.get("title").and_then(|v| v.as_str()),
        Some("chore: release v0.1.0")
    );
    let rewrite = sent.get(3).unwrap_or_else(|| unreachable!("recorded"));
    assert_eq!(rewrite.method, "PATCH");
    assert!(
        rewrite.url.ends_with("/pulls/7"),
        "the index addresses the pull: {}",
        rewrite.url
    );
    let rewrite_body: serde_json::Value =
        serde_json::from_str(rewrite.body.as_deref().unwrap_or_default())
            .unwrap_or_else(|error| unreachable!("the adapter sends valid JSON: {error}"));
    assert_eq!(
        rewrite_body.get("title").and_then(|v| v.as_str()),
        Some("chore: release v0.1.0")
    );
    assert_eq!(
        rewrite_body.get("body").and_then(|v| v.as_str()),
        Some("body-two")
    );
    assert!(
        rewrite_body.get("state").is_none(),
        "a rewrite never flips state"
    );
}

#[test]
fn non_release_branch_fails_closed() {
    let (forge, transport) = forge();
    let foreign = PrSpec {
        branch: BranchName::new("feature-x")
            .unwrap_or_else(|error| unreachable!("static branch is valid: {error}")),
        title: "x".to_owned(),
        body: "y".to_owned(),
    };
    let error = forge
        .upsert_pr(&foreign)
        .expect_err("a foreign branch shape is a caller bug");
    assert_eq!(error.code(), "E-FORGE-REQUEST");
    assert!(
        transport.sent().is_empty(),
        "the decline happens before any call"
    );
}

#[test]
fn close_reports_the_not_found_value_then_uses_the_verified_state_shape() {
    let (forge, transport) = forge();
    transport.enqueue(response(200, &empty_list()));
    let closed = forge
        .close_pr(&branch())
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert!(!closed, "nothing to close is Ok(false), not an error");
    transport.enqueue(response(200, &format!("[{}]", pull(3, BRANCH, "t", "b"))));
    transport.enqueue(response(200, &pull(3, BRANCH, "t", "b")));
    let closed = forge
        .close_pr(&branch())
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert!(closed);
    let sent = transport.sent();
    let close = sent.get(2).unwrap_or_else(|| unreachable!("recorded"));
    assert_eq!(close.method, "PATCH");
    assert!(close.url.ends_with("/pulls/3"), "{}", close.url);
    let close_body: serde_json::Value =
        serde_json::from_str(close.body.as_deref().unwrap_or_default())
            .unwrap_or_else(|error| unreachable!("the adapter sends valid JSON: {error}"));
    assert_eq!(
        close_body.get("state").and_then(|v| v.as_str()),
        Some("closed"),
        "the verified close — EditPullRequestOption.state (research §8)"
    );
}

#[test]
fn close_survives_the_vanished_pull_race() {
    let (forge, transport) = forge();
    transport.enqueue(response(200, &format!("[{}]", pull(3, BRANCH, "t", "b"))));
    transport.enqueue(response(404, r#"{"message":"Not Found"}"#));
    let closed = forge
        .close_pr(&branch())
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert!(
        closed,
        "the pull vanished between the find and the close — the end state \
             is already the intended one (I1)"
    );
}

#[test]
fn release_is_get_by_tag_first_and_idempotent() {
    let (forge, transport) = forge();
    transport.enqueue(response(404, r#"{"message":"Not Found"}"#));
    transport.enqueue(response(
        201,
        r#"{"tag_name":"pkg-v0.1.0","html_url":"https://codeberg.org/octo/halyard-demo/releases/tag/pkg-v0.1.0","id":3}"#,
    ));
    let created = forge
        .create_release(&ReleaseSpec {
            tag: "pkg-v0.1.0".to_owned(),
            notes: "## Features".to_owned(),
        })
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert!(created.created, "the fresh path reports created");
    assert_eq!(created.tag, "pkg-v0.1.0");
    assert_eq!(
        created.url, "https://codeberg.org/octo/halyard-demo/releases/tag/pkg-v0.1.0",
        "`html_url` is the release URL (research §3)"
    );
    transport.enqueue(response(
        200,
        r#"{"tag_name":"pkg-v0.1.0","html_url":"https://codeberg.org/octo/halyard-demo/releases/tag/pkg-v0.1.0","id":3}"#,
    ));
    let existed = forge
        .create_release(&ReleaseSpec {
            tag: "pkg-v0.1.0".to_owned(),
            notes: "## Features".to_owned(),
        })
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert!(
        !existed.created,
        "the GET-by-tag-first re-run creates nothing (I1)"
    );
    let sent = transport.sent();
    let probe = sent.first().unwrap_or_else(|| unreachable!("recorded"));
    assert!(
        probe.url.ends_with("/releases/tags/pkg-v0.1.0"),
        "the GET-by-tag-first probe (research §3): {}",
        probe.url
    );
    let create = sent.get(1).unwrap_or_else(|| unreachable!("recorded"));
    let create_body: serde_json::Value =
        serde_json::from_str(create.body.as_deref().unwrap_or_default())
            .unwrap_or_else(|error| unreachable!("the adapter sends valid JSON: {error}"));
    assert_eq!(
        create_body.get("tag_name").and_then(|v| v.as_str()),
        Some("pkg-v0.1.0"),
        "`tag_name` is the create body's required field (research §3)"
    );
    assert_eq!(
        create_body.get("body").and_then(|v| v.as_str()),
        Some("## Features"),
        "the notes ride `body` (research §3)"
    );
}

#[test]
fn rate_limit_snapshot_and_retry_after() {
    let (forge, transport) = forge();
    transport.enqueue(response_with_header(
        429,
        ("retry-after", "17"),
        r#"{"message":"Too Many Requests"}"#,
    ));
    let error = forge
        .find_release_pr(&branch())
        .expect_err("a throttled response is a rate-limit error");
    assert_eq!(error.code(), "E-FORGE-RATE-LIMIT");
    assert!(
        error.to_string().contains("retry after 17s"),
        "the server's own retry-after wins: {error}"
    );
    assert_eq!(
        forge.rate_limit(),
        RateInfo {
            limit: 0,
            remaining: 0,
        },
        "no quota headers are defined on this surface — zeros are the honest \
             snapshot (research §6)"
    );
    transport.enqueue(response_with_header(
        200,
        ("x-ratelimit-limit", "2000"),
        &empty_list(),
    ));
    let _ = forge
        .find_release_pr(&branch())
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert_eq!(
        forge.rate_limit().limit,
        2000,
        "quota headers are snapshotted when a deployment sends them"
    );
}

#[test]
fn commit_via_api_declines_naming_the_adapter() {
    let (forge, transport) = forge();
    let error = forge
        .commit_via_api(&ApiCommit {
            branch: branch(),
            message: "chore: release v0.1.0".to_owned(),
            files: vec![],
        })
        .expect_err("the documented honest decline (research §5)");
    assert_eq!(error.code(), "E-FORGE-UNSUPPORTED");
    let ForgeError::Unsupported { adapter } = error else {
        unreachable!("the decline is the Unsupported variant")
    };
    assert_eq!(
        adapter, "ForgejoRestForge",
        "the decline names the adapter (§7.3's naming convention)"
    );
    assert!(
        transport.sent().is_empty(),
        "a decline spends no calls — the contents API has no atomic \
             multi-file commit to fake"
    );
}

#[test]
fn comment_scan_walks_the_issue_comments_then_rewrites() {
    let (forge1, transport1) = forge();
    transport1.enqueue(response(
        200,
        r#"[{"id":11,"body":"<!-- halyard:check v1 -->\n\nstale","html_url":"https://codeberg.org/octo/halyard-demo/issues/comments/11"}]"#,
    ));
    transport1.enqueue(response(
        200,
        r#"{"id":11,"body":"<!-- halyard:check v1 -->\n\nfresh","html_url":"https://codeberg.org/octo/halyard-demo/issues/comments/11"}"#,
    ));
    let comment = forge1
        .upsert_pr_comment(&halyard_forge::CommentSpec {
            pr_number: 7,
            marker: "<!-- halyard:check v1 -->".to_owned(),
            body: "<!-- halyard:check v1 -->\n\nfresh".to_owned(),
        })
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert!(!comment.created, "the marker hit rewrites in place (I1)");
    let sent = transport1.sent();
    let scan = sent.first().unwrap_or_else(|| unreachable!("recorded"));
    assert!(
        scan.url.ends_with("/issues/7/comments"),
        "the scan addresses the PR's issue index (research §4): {}",
        scan.url
    );
    assert!(
        !scan.url.contains("page=") && !scan.url.contains("limit="),
        "the endpoint documents no pagination parameters — the scan walks \
             the array once (research §4): {}",
        scan.url
    );
    let rewrite = sent.get(1).unwrap_or_else(|| unreachable!("recorded"));
    assert_eq!(rewrite.method, "PATCH");
    assert!(
        rewrite.url.ends_with("/issues/comments/11"),
        "the Comment.id is the rewrite address (research §4): {}",
        rewrite.url
    );

    // The create path: an empty scan opens a fresh comment.
    let (forge2, transport2) = forge();
    transport2.enqueue(response(200, &empty_list()));
    transport2.enqueue(response(
        201,
        r#"{"id":12,"body":"<!-- halyard:check v1 -->\n\nfresh","html_url":"https://codeberg.org/octo/halyard-demo/issues/comments/12"}"#,
    ));
    let comment = forge2
        .upsert_pr_comment(&halyard_forge::CommentSpec {
            pr_number: 7,
            marker: "<!-- halyard:check v1 -->".to_owned(),
            body: "<!-- halyard:check v1 -->\n\nfresh".to_owned(),
        })
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert!(comment.created, "an empty scan creates (I1's GET-first)");
    let sent = transport2.sent();
    let create = sent.get(1).unwrap_or_else(|| unreachable!("recorded"));
    assert_eq!(create.method, "POST");
    assert!(create.url.ends_with("/issues/7/comments"), "{}", create.url);
}

#[test]
fn machine_propagate_and_prepare_passes_ride_the_adapter() {
    let (forge, transport) = forge();
    transport.enqueue(response(404, r#"{"message":"Not Found"}"#));
    transport.enqueue(response(
        201,
        r#"{"tag_name":"pkg-v0.1.0","html_url":"https://codeberg.org/octo/halyard-demo/releases/tag/pkg-v0.1.0","id":3}"#,
    ));
    let report = run_release_machine(
        &forge,
        &MachineInput {
            branch: branch(),
            pending_releases: vec![ReleaseSpec {
                tag: "pkg-v0.1.0".to_owned(),
                notes: "## Features".to_owned(),
            }],
            release_pr: Some(spec("chore: release v0.1.0", "## 🚀 Release plan")),
            content_commit: None,
        },
    )
    .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert!(report.propagated, "pending releases gate the prepare phase");
    assert_eq!(report.effects.len(), 1, "one release effect, no PR effects");
    let calls = forge.calls();
    assert_eq!(calls.len(), 2, "GET-by-tag + create (§11.5)");
    let record: &CallRecord = calls.first().unwrap_or_else(|| unreachable!("recorded"));
    assert_eq!(record.method, "GET");
}

#[test]
fn server_side_failures_map_onto_the_taxonomy() {
    let (forge, transport) = forge();
    transport.enqueue(response(401, r#"{"message":"user does not exist"}"#));
    let error = forge
        .find_release_pr(&branch())
        .expect_err("an invalid token is an auth failure");
    assert_eq!(error.code(), "E-FORGE-AUTH");
    transport.enqueue(response(500, r#"{"message":"500 Internal Server Error"}"#));
    let error = forge
        .find_release_pr(&branch())
        .expect_err("a server failure is a transport failure");
    assert_eq!(error.code(), "E-FORGE-TRANSPORT");
    transport.enqueue(response(409, r#"{"message":"The head branch is closed"}"#));
    let error = forge
        .upsert_pr(&spec("t", "b"))
        .expect_err("a conflict (the documented 409 on the pull PATCH) is a conflict failure");
    assert_eq!(error.code(), "E-FORGE-CONFLICT");
    transport.enqueue(response(422, r#"{"message":"validation error"}"#));
    let error = forge
        .upsert_pr(&spec("t", "b"))
        .expect_err("a rejected request is a request failure");
    assert_eq!(error.code(), "E-FORGE-REQUEST");
}
