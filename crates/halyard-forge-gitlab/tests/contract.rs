//! The `GitLabRestForge` contract (M7 step 1, ADR-0049): every `Forge`
//! method driven over a scripted `MemoryTransport` with GitLab-shaped
//! payloads (shapes verified in `docs/research/gitlab-rest-forge.md`).
//! The assertions pin the wire contract: request shapes, the §11.4
//! idempotence values, the `PRIVATE-TOKEN` auth and the §11.5 call log.

use base64::Engine as _;
use halyard_forge::transport::{HttpResponse, MemoryTransport};
use halyard_forge::{
    ApiCommit, ApiFile, BranchName, CallRecord, Forge, MachineInput, PrSpec, RateInfo, ReleaseSpec,
    run_release_machine,
};
use halyard_forge_gitlab::{GitLabEndpoint, GitLabRestForge};

const PROJECT: &str = "octo/halyard-demo";
const BRANCH: &str = "halyard/release/main";

fn forge() -> (GitLabRestForge<MemoryTransport>, MemoryTransport) {
    let transport = MemoryTransport::new();
    let forge = GitLabRestForge::with_transport(endpoint(), "smoke-token", transport.clone());
    (forge, transport)
}

fn endpoint() -> GitLabEndpoint {
    GitLabEndpoint::new(PROJECT)
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

fn mr(iid: u64, source: &str, title: &str, body: &str) -> String {
    format!(
        r#"{{"iid":{iid},"title":{title},"description":{body},"state":"opened","source_branch":"{source}","target_branch":"main","web_url":"https://gitlab.com/octo/halyard-demo/-/merge_requests/{iid}"}}"#,
        title = json_str(title),
        body = json_str(body),
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

fn sha_response(sha: &str) -> String {
    format!(r#"{{"id":"{sha}","short_id":"abc1234","title":"x"}}"#)
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
            .starts_with("https://gitlab.com/api/v4/projects/octo%2Fhalyard-demo/merge_requests"),
        "the project path is URL-encoded: {}",
        request.url
    );
    assert!(request.url.contains("state=opened"), "{}", request.url);
    assert!(request.url.contains("source_branch="), "{}", request.url);
    assert_eq!(
        request.header("PRIVATE-TOKEN"),
        Some("smoke-token"),
        "the documented auth header (research §1)"
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
    let forge = GitLabRestForge::with_transport(endpoint(), "", transport.clone());
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
        &mr(7, BRANCH, "chore: release v0.1.0", "body-one"),
    ));
    let opened = forge
        .upsert_pr(&spec("chore: release v0.1.0", "body-one"))
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert_eq!(opened.number, 7);
    transport.enqueue(response(200, &format!("[{}]", mr(7, BRANCH, "old", "old"))));
    transport.enqueue(response(
        200,
        &mr(7, BRANCH, "chore: release v0.1.0", "body-two"),
    ));
    let updated = forge
        .upsert_pr(&spec("chore: release v0.1.0", "body-two"))
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert_eq!(updated.number, 7, "same branch ⇒ same MR (I1)");
    assert_eq!(updated.body, "body-two");
    let sent = transport.sent();
    let create = sent.get(1).unwrap_or_else(|| unreachable!("recorded"));
    assert_eq!(create.method, "POST");
    let create_body: serde_json::Value =
        serde_json::from_str(create.body.as_deref().unwrap_or_default())
            .unwrap_or_else(|error| unreachable!("the adapter sends valid JSON: {error}"));
    assert_eq!(
        create_body.get("source_branch").and_then(|v| v.as_str()),
        Some(BRANCH)
    );
    assert_eq!(
        create_body.get("target_branch").and_then(|v| v.as_str()),
        Some("main")
    );
    assert_eq!(
        create_body.get("title").and_then(|v| v.as_str()),
        Some("chore: release v0.1.0")
    );
    let rewrite = sent.get(3).unwrap_or_else(|| unreachable!("recorded"));
    assert_eq!(rewrite.method, "PUT");
    assert!(
        rewrite.url.ends_with("/merge_requests/7"),
        "the iid addresses the MR: {}",
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
        rewrite_body.get("description").and_then(|v| v.as_str()),
        Some("body-two")
    );
    assert!(
        rewrite_body.get("state_event").is_none(),
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
fn close_reports_the_not_found_value_and_never_double_closes() {
    let (forge, transport) = forge();
    transport.enqueue(response(200, &empty_list()));
    let closed = forge
        .close_pr(&branch())
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert!(!closed, "nothing to close is Ok(false), not an error");
    transport.enqueue(response(200, &format!("[{}]", mr(3, BRANCH, "t", "b"))));
    transport.enqueue(response(200, &mr(3, BRANCH, "t", "b")));
    let closed = forge
        .close_pr(&branch())
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert!(closed);
    let sent = transport.sent();
    let close = sent.get(2).unwrap_or_else(|| unreachable!("recorded"));
    let close_body: serde_json::Value =
        serde_json::from_str(close.body.as_deref().unwrap_or_default())
            .unwrap_or_else(|error| unreachable!("the adapter sends valid JSON: {error}"));
    assert_eq!(
        close_body.get("state_event").and_then(|v| v.as_str()),
        Some("close"),
        "the documented close (research §3)"
    );
}

#[test]
fn release_is_get_by_tag_first_and_idempotent() {
    let (forge, transport) = forge();
    transport.enqueue(response(404, r#"{"message":"404 Not Found"}"#));
    transport.enqueue(response(
        201,
        r#"{"tag_name":"pkg-v0.1.0","_links":{"self":"https://gitlab.com/octo/halyard-demo/-/releases/pkg-v0.1.0"}}"#,
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
        created.url, "https://gitlab.com/octo/halyard-demo/-/releases/pkg-v0.1.0",
        "`_links.self` is the release URL (research §5)"
    );
    transport.enqueue(response(
        200,
        r#"{"tag_name":"pkg-v0.1.0","_links":{"self":"https://gitlab.com/octo/halyard-demo/-/releases/pkg-v0.1.0"}}"#,
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
}

#[test]
fn rate_limit_snapshot_and_retry_after() {
    let (forge, transport) = forge();
    transport.enqueue(response_with_header(
        429,
        ("retry-after", "17"),
        r#"{"message":"429 Too Many Requests"}"#,
    ));
    let error = forge
        .find_release_pr(&branch())
        .expect_err("a throttled response is a rate-limit error");
    assert_eq!(error.code(), "E-FORGE-RATE-LIMIT");
    assert_eq!(
        forge.rate_limit(),
        RateInfo {
            limit: 0,
            remaining: 0,
        },
        "GitLab does not universally send quota headers — zeros are the honest \
         snapshot (research §7)"
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
fn commit_via_api_probes_each_file_then_posts_base64_actions() {
    let (forge, transport) = forge();
    // The file does not exist on the branch ⇒ the create action kind.
    transport.enqueue(response(404, r#"{"message":"404 Not Found"}"#));
    transport.enqueue(response(
        201,
        &sha_response("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
    ));
    let oid = forge
        .commit_via_api(&ApiCommit {
            branch: branch(),
            message: "chore: release v0.1.0".to_owned(),
            files: vec![ApiFile {
                path: "crates/core/Cargo.toml".to_owned(),
                content: b"[package]\nversion = \"0.1.0\"".to_vec(),
            }],
        })
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert_eq!(
        oid.as_bytes().len(),
        40,
        "the commits API's `id` is the created sha"
    );
    let sent = transport.sent();
    let probe = sent.first().unwrap_or_else(|| unreachable!("recorded"));
    assert!(
        probe
            .url
            .contains("/repository/files/crates%2Fcore%2FCargo.toml"),
        "the file path is URL-encoded: {}",
        probe.url
    );
    assert!(
        probe.url.contains("ref=halyard/release/main"),
        "the ref rides the query verbatim (research §6): {}",
        probe.url
    );
    let create = sent.get(1).unwrap_or_else(|| unreachable!("recorded"));
    assert_eq!(create.method, "POST");
    assert!(create.url.contains("/repository/commits"));
    let body: serde_json::Value = serde_json::from_str(create.body.as_deref().unwrap_or_default())
        .unwrap_or_else(|error| unreachable!("the adapter sends valid JSON: {error}"));
    assert_eq!(body.get("branch").and_then(|v| v.as_str()), Some(BRANCH));
    let actions = body
        .get("actions")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| unreachable!("one action per file"));
    assert_eq!(actions.len(), 1);
    let action = actions
        .first()
        .unwrap_or_else(|| unreachable!("one action"));
    assert_eq!(
        action.get("action").and_then(|v| v.as_str()),
        Some("create")
    );
    assert_eq!(
        action.get("encoding").and_then(|v| v.as_str()),
        Some("base64")
    );
    let expected =
        base64::engine::general_purpose::STANDARD.encode(b"[package]\nversion = \"0.1.0\"");
    assert_eq!(
        action.get("content").and_then(|v| v.as_str()),
        Some(expected.as_str())
    );
}

#[test]
fn comment_scan_walks_ascending_notes_then_rewrites() {
    let (forge, transport) = forge();
    transport.enqueue(response(
        200,
        r#"[{"id":11,"body":"<!-- halyard:check v1 -->\n\nstale","system":false}]"#,
    ));
    transport.enqueue(response(
        200,
        r#"{"id":11,"body":"<!-- halyard:check v1 -->\n\nfresh","system":false}"#,
    ));
    let comment = forge
        .upsert_pr_comment(&halyard_forge::CommentSpec {
            pr_number: 7,
            marker: "<!-- halyard:check v1 -->".to_owned(),
            body: "<!-- halyard:check v1 -->\n\nfresh".to_owned(),
        })
        .unwrap_or_else(|error| unreachable!("the scripted exchange succeeded: {error}"));
    assert!(!comment.created, "the marker hit rewrites in place (I1)");
    let sent = transport.sent();
    let scan = sent.first().unwrap_or_else(|| unreachable!("recorded"));
    assert!(
        scan.url.contains("sort=asc"),
        "GitLab's note list defaults to desc — the ascending walk must ask (research §4)"
    );
    assert!(scan.url.contains("order_by=created_at"), "{}", scan.url);
    assert!(scan.url.contains("per_page=100"), "{}", scan.url);
    let rewrite = sent.get(1).unwrap_or_else(|| unreachable!("recorded"));
    assert_eq!(rewrite.method, "PUT");
    assert!(rewrite.url.ends_with("/notes/11"), "{}", rewrite.url);
}

#[test]
fn machine_propagate_and_prepare_passes_ride_the_adapter() {
    let (forge, transport) = forge();
    transport.enqueue(response(404, r#"{"message":"404 Not Found"}"#));
    transport.enqueue(response(
        201,
        r#"{"tag_name":"pkg-v0.1.0","_links":{"self":"https://gitlab.com/octo/halyard-demo/-/releases/pkg-v0.1.0"}}"#,
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
    transport.enqueue(response(401, r#"{"message":"401 Unauthorized"}"#));
    let error = forge
        .find_release_pr(&branch())
        .expect_err("an invalid token is an auth failure");
    assert_eq!(error.code(), "E-FORGE-AUTH");
    transport.enqueue(response(500, r#"{"message":"500 Internal Server Error"}"#));
    let error = forge
        .find_release_pr(&branch())
        .expect_err("a server failure is a transport failure");
    assert_eq!(error.code(), "E-FORGE-TRANSPORT");
    transport.enqueue(response(
        400,
        r#"{"message":{"target_branch":["is missing"]}}"#,
    ));
    let error = forge
        .upsert_pr(&spec("t", "b"))
        .expect_err("a rejected request is a request failure");
    assert_eq!(error.code(), "E-FORGE-REQUEST");
}
