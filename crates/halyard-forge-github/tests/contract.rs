//! The `GitHubRestForge` contract (M4.1b, ADR-0013): every `Forge`
//! method driven over a scripted `MemoryTransport` with GitHub-shaped
//! payloads (shapes verified in `docs/research/github-rest-forge.md`).
//! The assertions pin the wire contract: request shapes, the §11.4
//! idempotence values, the `ETag` conditionals and the §11.5 call log.

use halyard_core::reason::Oid;
use halyard_forge::transport::{HttpRequest, HttpResponse, MemoryTransport};
use halyard_forge::{
    ApiCommit, ApiFile, BranchName, CallRecord, Forge, ForgeError, MachineInput, PrSpec, RateInfo,
    ReleaseSpec, run_release_machine,
};
use halyard_forge_github::{ForgeEndpoint, GitHubRestForge};

type BoxError = Box<dyn std::error::Error>;

/// House-style render helper: `?`-propagating (no `unwrap` in tests;
/// impossible paths are `unreachable!`).
const BRANCH: &str = "halyard/release/main";
const OWNER: &str = "octo";
const REPO: &str = "halyard-demo";

fn forge() -> (GitHubRestForge<MemoryTransport>, MemoryTransport) {
    let transport = MemoryTransport::new();
    let forge = GitHubRestForge::with_transport(
        ForgeEndpoint::with_base(OWNER, REPO, "https://api.github.com")
            .unwrap_or_else(|error| unreachable!("static endpoint is valid: {error}")),
        "smoke-token",
        transport.clone(),
    );
    (forge, transport)
}

fn forge_anonymous() -> (GitHubRestForge<MemoryTransport>, MemoryTransport) {
    let transport = MemoryTransport::new();
    let forge = GitHubRestForge::with_transport(
        ForgeEndpoint::with_base(OWNER, REPO, "https://api.github.com")
            .unwrap_or_else(|error| unreachable!("static endpoint is valid: {error}")),
        "",
        transport.clone(),
    );
    (forge, transport)
}

/// A list-pulls body carrying one PR for the release branch (the shape
/// the docs pin: `number`, `title`, `body`, `html_url`, `head.ref`).
fn pr_list(number: u64, title: &str, body: &str) -> String {
    format!(
        r#"[{{"number":{number},"title":"{title}","body":{body_json},"html_url":"https://github.com/{OWNER}/{REPO}/pull/{number}","head":{{"ref":"{BRANCH}"}}}}]"#,
        body_json = serde_json::to_string(body)
            .unwrap_or_else(|error| unreachable!("plain string serializes: {error}")),
    )
}

fn pr(number: u64, title: &str, body: &str) -> String {
    format!(
        r#"{{"number":{number},"title":"{title}","body":{body_json},"html_url":"https://github.com/{OWNER}/{REPO}/pull/{number}","head":{{"ref":"{BRANCH}"}}}}"#,
        body_json = serde_json::to_string(body)
            .unwrap_or_else(|error| unreachable!("plain string serializes: {error}")),
    )
}

fn release_body(tag: &str) -> String {
    format!(
        r#"{{"id":1,"tag_name":"{tag}","html_url":"https://github.com/{OWNER}/{REPO}/releases/tag/{tag}"}}"#
    )
}

fn ref_body(sha: &str) -> String {
    format!(r#"{{"ref":"refs/heads/{BRANCH}","object":{{"sha":"{sha}","type":"commit"}}}}"#)
}

fn commit_body(sha: &str, tree: &str) -> String {
    format!(r#"{{"sha":"{sha}","tree":{{"sha":"{tree}"}}}}"#)
}

fn sha_response(sha: &str) -> String {
    format!(r#"{{"sha":"{sha}"}}"#)
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

fn branch() -> BranchName {
    BranchName::new(BRANCH).unwrap_or_else(|error| unreachable!("static branch is valid: {error}"))
}

/// The Nth scripted item (the `indexing_slicing`-safe accessor the
/// restriction lint leaves in place; tests allow `unreachable!`).
fn at<T>(items: &[T], index: usize) -> &T {
    items
        .get(index)
        .unwrap_or_else(|| unreachable!("item {index} was recorded; {} were", items.len()))
}

/// A JSON object field that must be present (the same discipline).
fn field<'a>(value: &'a serde_json::Value, key: &str) -> &'a serde_json::Value {
    value
        .get(key)
        .unwrap_or_else(|| unreachable!("field {key} is present"))
}

/// A JSON string field that must be present and textual.
fn str_field<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    field(value, key)
        .as_str()
        .unwrap_or_else(|| unreachable!("field {key} is a string"))
}

/// The Nth element of a JSON array field that must be present.
fn array_item<'a>(value: &'a serde_json::Value, key: &str, index: usize) -> &'a serde_json::Value {
    field(value, key)
        .as_array()
        .unwrap_or_else(|| unreachable!("field {key} is an array"))
        .get(index)
        .unwrap_or_else(|| unreachable!("item {index} of {key} is present"))
}

/// A string element of a JSON array field (the `parents: [sha]` shape).
fn array_str<'a>(value: &'a serde_json::Value, key: &str, index: usize) -> &'a str {
    array_item(value, key, index)
        .as_str()
        .unwrap_or_else(|| unreachable!("item {index} of {key} is a string"))
}

/// `find_release_pr` on an empty forge: `None` (a value, §11.4), one
/// GET with the §11.4/§11.5 mandatory headers and the `head` filter.
#[test]
fn find_release_pr_empty_is_none_with_one_conditional_capable_get() -> Result<(), BoxError> {
    let (forge, transport) = forge();
    transport.enqueue(response(200, "[]"));
    let found = forge.find_release_pr(&branch())?;
    assert!(found.is_none());
    let sent = transport.sent();
    assert_eq!(sent.len(), 1);
    let expected = format!(
        "https://api.github.com/repos/{OWNER}/{REPO}/pulls?state=open&head={OWNER}:{BRANCH}"
    );
    assert_eq!(at(&sent, 0).url, expected, "the list probe filters by head");
    assert_eq!(
        at(&sent, 0).header("Accept"),
        Some("application/vnd.github+json")
    );
    assert_eq!(
        at(&sent, 0).header("X-GitHub-Api-Version"),
        Some("2022-11-28")
    );
    assert!(
        at(&sent, 0)
            .header("User-Agent")
            .is_some_and(|ua| ua.starts_with("halyard/")),
        "a valid User-Agent is mandatory (research §1)"
    );
    assert_eq!(
        at(&sent, 0).header("Authorization"),
        Some("Bearer smoke-token")
    );
    Ok(())
}

/// An open PR maps onto the seam's `Pr` value.
#[test]
fn find_release_pr_maps_the_open_pr() -> Result<(), BoxError> {
    let (forge, transport) = forge();
    transport.enqueue(response_with_header(
        200,
        ("etag", "\"abc123\""),
        &pr_list(7, "chore: release v0.5.0", "## 🚀 Release plan"),
    ));
    let found = forge.find_release_pr(&branch())?;
    let pr = found.unwrap_or_else(|| unreachable!("the scripted body carries the PR"));
    assert_eq!(pr.number, 7);
    assert_eq!(pr.branch, BRANCH);
    assert_eq!(pr.url, format!("https://github.com/{OWNER}/{REPO}/pull/7"));
    Ok(())
}

/// The re-find rides the cached `ETag` (`If-None-Match`); a `304`
/// returns the cached PR without re-parsing — the conditional the
/// research pins (304s do not count against the primary limit).
#[test]
fn re_find_is_conditional_and_304_returns_the_cached_pr() -> Result<(), BoxError> {
    let (forge, transport) = forge();
    transport.enqueue(response_with_header(
        200,
        ("etag", "\"abc123\""),
        &pr_list(7, "chore: release v0.5.0", "## 🚀 Release plan"),
    ));
    let first = forge.find_release_pr(&branch())?;
    assert!(first.is_some());
    transport.enqueue(response(304, ""));
    let second = forge.find_release_pr(&branch())?;
    let pr = second.unwrap_or_else(|| unreachable!("304 returns the cached PR"));
    assert_eq!(pr.number, 7);
    let sent = transport.sent();
    assert_eq!(sent.len(), 2);
    assert_eq!(
        at(&sent, 1).header("If-None-Match"),
        Some("\"abc123\""),
        "the second probe is conditional"
    );
    Ok(())
}

/// `upsert_pr` on an empty forge creates: `POST /pulls` with the §11.2
/// base derived from the release-branch prefix (zero extra calls).
#[test]
fn upsert_creates_with_derived_base() -> Result<(), BoxError> {
    let (forge, transport) = forge();
    transport.enqueue(response(200, "[]"));
    transport.enqueue(response_with_header(
        201,
        ("etag", "\"e2\""),
        &pr(9, "t", "b"),
    ));
    let spec = PrSpec {
        branch: branch(),
        title: "chore: release v0.5.0".to_owned(),
        body: "## 🚀 Release plan".to_owned(),
    };
    let pr = forge.upsert_pr(&spec)?;
    assert_eq!(pr.number, 9);
    let sent = transport.sent();
    assert_eq!(sent.len(), 2, "find + create only");
    assert_eq!(at(&sent, 1).method, "POST");
    let payload: serde_json::Value = serde_json::from_str(
        at(&sent, 1)
            .body
            .as_deref()
            .unwrap_or_else(|| unreachable!("the create carries a JSON body")),
    )?;
    assert_eq!(str_field(&payload, "head"), BRANCH);
    assert_eq!(
        str_field(&payload, "base"),
        "main",
        "the §11.2 prefix inverse"
    );
    assert_eq!(str_field(&payload, "title"), "chore: release v0.5.0");
    assert_eq!(str_field(&payload, "body"), "## 🚀 Release plan");
    Ok(())
}

/// `upsert_pr` rewrites the existing PR in place — same number, no
/// duplicate (I1); the ETag-cached find avoids a re-parse.
#[test]
fn upsert_updates_in_place() -> Result<(), BoxError> {
    let (forge, transport) = forge();
    transport.enqueue(response_with_header(
        200,
        ("etag", "\"abc\""),
        &pr_list(7, "old title", "old body"),
    ));
    transport.enqueue(response(200, &pr(7, "new title", "new body")));
    let spec = PrSpec {
        branch: branch(),
        title: "new title".to_owned(),
        body: "new body".to_owned(),
    };
    let pr = forge.upsert_pr(&spec)?;
    assert_eq!(pr.number, 7, "same number — rewritten, not duplicated");
    assert_eq!(pr.title, "new title");
    let sent = transport.sent();
    assert_eq!(at(&sent, 1).method, "PATCH");
    assert!(
        at(&sent, 1)
            .url
            .ends_with(&format!("/repos/{OWNER}/{REPO}/pulls/7")),
        "the rewrite targets the found PR"
    );
    let payload: serde_json::Value = serde_json::from_str(
        at(&sent, 1)
            .body
            .as_deref()
            .unwrap_or_else(|| unreachable!("PATCH carries a body")),
    )?;
    assert_eq!(str_field(&payload, "title"), "new title");
    assert_eq!(str_field(&payload, "body"), "new body");
    Ok(())
}

/// `release_exists` treats the documented 404 as the not-found *value*.
#[test]
fn release_exists_maps_404_to_false() -> Result<(), BoxError> {
    let (forge, transport) = forge();
    transport.enqueue(response(404, "{\"message\":\"Not Found\"}"));
    assert!(!forge.release_exists("core-v0.5.0")?);
    transport.enqueue(response(200, &release_body("core-v0.5.0")));
    assert!(forge.release_exists("core-v0.5.0")?);
    Ok(())
}

/// `create_release` is GET-by-tag-first (§11.1 step 2, I1): the fresh
/// path creates (201), the re-run reports the existing release.
#[test]
fn create_release_is_get_by_tag_first_idempotent() -> Result<(), BoxError> {
    let (forge, transport) = forge();
    let spec = ReleaseSpec {
        tag: "core-v0.5.0".to_owned(),
        notes: "## Features\n\n- the painter walk".to_owned(),
    };
    transport.enqueue(response(404, "{\"message\":\"Not Found\"}"));
    transport.enqueue(response(201, &release_body("core-v0.5.0")));
    let release = forge.create_release(&spec)?;
    assert!(release.created);
    assert_eq!(release.tag, "core-v0.5.0");
    // The I1 re-run: the GET finds the release; no second POST.
    transport.enqueue(response(200, &release_body("core-v0.5.0")));
    let rerun = forge.create_release(&spec)?;
    assert!(!rerun.created, "the re-run reports the existing release");
    let posts = transport
        .sent()
        .iter()
        .filter(|req| req.method == "POST")
        .count();
    assert_eq!(posts, 1, "exactly one create across both passes");
    let payload: serde_json::Value = serde_json::from_str(
        at(&transport.sent(), 1)
            .body
            .as_deref()
            .unwrap_or_else(|| unreachable!("the create carries a JSON body")),
    )?;
    assert_eq!(str_field(&payload, "tag_name"), "core-v0.5.0");
    assert_eq!(
        str_field(&payload, "body"),
        "## Features\n\n- the painter walk"
    );
    Ok(())
}

/// `close_pr`: the found PR is `PATCHed` closed (`true`); the re-run
/// finds nothing and reports `false` — a value, never an error (I1).
#[test]
fn close_pr_closes_once_then_reports_false() -> Result<(), BoxError> {
    let (forge, transport) = forge();
    transport.enqueue(response(200, &pr_list(7, "t", "b")));
    transport.enqueue(response(200, &pr(7, "t", "b")));
    assert!(forge.close_pr(&branch())?);
    transport.enqueue(response(200, "[]"));
    assert!(!forge.close_pr(&branch())?);
    let patches = transport
        .sent()
        .iter()
        .filter(|req| req.method == "PATCH")
        .count();
    assert_eq!(patches, 1, "the re-close is a no-op");
    Ok(())
}

/// `commit_via_api` (UTF-8 path): ref → commit → tree (content entries —
/// no blob round trips) → commit → ref update; `force: false` is
/// explicit (§11.2: never forced).
#[test]
fn commit_via_api_writes_the_git_data_chain() -> Result<(), BoxError> {
    const BASE: &str = "1111111111111111111111111111111111111111";
    const BASE_TREE: &str = "2222222222222222222222222222222222222222";
    const NEW_TREE: &str = "3333333333333333333333333333333333333333";
    const NEW_COMMIT: &str = "4444444444444444444444444444444444444444";
    let (forge, transport) = forge();
    transport.enqueue(response(200, &ref_body(BASE)));
    transport.enqueue(response(200, &commit_body(BASE, BASE_TREE)));
    transport.enqueue(response(201, &sha_response(NEW_TREE)));
    transport.enqueue(response(201, &sha_response(NEW_COMMIT)));
    transport.enqueue(response(200, &ref_body(NEW_COMMIT)));
    let spec = ApiCommit {
        branch: branch(),
        message: "chore: release v0.5.0".to_owned(),
        files: vec![ApiFile {
            path: "crates/core/Cargo.toml".to_owned(),
            content: b"[package]\nversion = \"0.5.0\"\n".to_vec(),
        }],
    };
    let oid = forge.commit_via_api(&spec)?;
    assert_eq!(
        oid,
        Oid::from_hex(NEW_COMMIT.as_bytes()).unwrap_or_else(|| unreachable!("fixture is 40 hex"))
    );
    let sent = transport.sent();
    assert_eq!(sent.len(), 5, "ref + commit + tree + commit + ref update");
    let tree_payload: serde_json::Value = serde_json::from_str(
        at(&sent, 2)
            .body
            .as_deref()
            .unwrap_or_else(|| unreachable!("the tree create carries a body")),
    )?;
    assert_eq!(str_field(&tree_payload, "base_tree"), BASE_TREE);
    assert_eq!(
        str_field(array_item(&tree_payload, "tree", 0), "path"),
        "crates/core/Cargo.toml"
    );
    assert_eq!(
        str_field(array_item(&tree_payload, "tree", 0), "mode"),
        "100644"
    );
    assert_eq!(
        str_field(array_item(&tree_payload, "tree", 0), "type"),
        "blob"
    );
    assert_eq!(
        str_field(array_item(&tree_payload, "tree", 0), "content"),
        "[package]\nversion = \"0.5.0\"\n",
        "UTF-8 content rides the tree entry (no blob round trip)"
    );
    let commit_payload: serde_json::Value = serde_json::from_str(
        at(&sent, 3)
            .body
            .as_deref()
            .unwrap_or_else(|| unreachable!("the commit create carries a body")),
    )?;
    assert_eq!(str_field(&commit_payload, "tree"), NEW_TREE);
    assert_eq!(array_str(&commit_payload, "parents", 0), BASE);
    let ref_payload: serde_json::Value = serde_json::from_str(
        at(&sent, 4)
            .body
            .as_deref()
            .unwrap_or_else(|| unreachable!("the ref update carries a body")),
    )?;
    assert_eq!(str_field(&ref_payload, "sha"), NEW_COMMIT);
    assert_eq!(
        field(&ref_payload, "force"),
        &serde_json::json!(false),
        "never forced (§11.2)"
    );
    Ok(())
}

/// `commit_via_api` (binary path): non-UTF-8 payloads upload through
/// the base64 blob endpoint and reference the returned sha.
#[test]
fn commit_via_api_uploads_binary_files_as_base64_blobs() -> Result<(), BoxError> {
    use base64::Engine as _;
    const BASE: &str = "1111111111111111111111111111111111111111";
    const BASE_TREE: &str = "2222222222222222222222222222222222222222";
    const BLOB: &str = "5555555555555555555555555555555555555555";
    const NEW_TREE: &str = "3333333333333333333333333333333333333333";
    const NEW_COMMIT: &str = "4444444444444444444444444444444444444444";
    let (forge, transport) = forge();
    transport.enqueue(response(200, &ref_body(BASE)));
    transport.enqueue(response(200, &commit_body(BASE, BASE_TREE)));
    transport.enqueue(response(201, &sha_response(BLOB)));
    transport.enqueue(response(201, &sha_response(NEW_TREE)));
    transport.enqueue(response(201, &sha_response(NEW_COMMIT)));
    transport.enqueue(response(200, &ref_body(NEW_COMMIT)));
    let spec = ApiCommit {
        branch: branch(),
        message: "chore: release assets".to_owned(),
        files: vec![ApiFile {
            path: "assets/icon.bin".to_owned(),
            content: vec![0x89, 0x50, 0x4e, 0x47],
        }],
    };
    forge.commit_via_api(&spec)?;
    let sent = transport.sent();
    assert_eq!(sent.len(), 6, "ref + commit + blob + tree + commit + ref");
    let blob_payload: serde_json::Value = serde_json::from_str(
        at(&sent, 2)
            .body
            .as_deref()
            .unwrap_or_else(|| unreachable!("the blob create carries a body")),
    )?;
    assert_eq!(
        str_field(&blob_payload, "encoding"),
        "base64",
        "binary payloads ride the documented encoding"
    );
    let expected = base64::engine::general_purpose::STANDARD.encode([0x89, 0x50, 0x4e, 0x47]);
    assert_eq!(str_field(&blob_payload, "content"), expected);
    let tree_payload: serde_json::Value = serde_json::from_str(
        at(&sent, 3)
            .body
            .as_deref()
            .unwrap_or_else(|| unreachable!("the tree create carries a body")),
    )?;
    assert_eq!(str_field(array_item(&tree_payload, "tree", 0), "sha"), BLOB);
    assert!(
        array_item(&tree_payload, "tree", 0)
            .get("content")
            .is_none(),
        "a sha entry never carries content (research §4)"
    );
    Ok(())
}

/// `rate_limit` is the last-observed header snapshot (research §2:
/// headers are authoritative); before any response it reports zeros —
/// nothing observed yet.
#[test]
fn rate_limit_snapshots_response_headers() -> Result<(), BoxError> {
    let (forge, transport) = forge();
    assert_eq!(
        forge.rate_limit(),
        RateInfo {
            limit: 0,
            remaining: 0
        },
        "no observation yet"
    );
    transport.enqueue(response_with_header(
        200,
        ("x-ratelimit-limit", "5000"),
        "[]",
    ));
    // A second header the snapshot reads:
    transport.enqueue(response_with_header(
        200,
        ("x-ratelimit-remaining", "4999"),
        "[]",
    ));
    let _ = forge.find_release_pr(&branch())?;
    let _ = forge.find_release_pr(&branch())?;
    assert_eq!(
        forge.rate_limit(),
        RateInfo {
            limit: 5000,
            remaining: 4999
        }
    );
    Ok(())
}

/// The call log records one entry per HTTP call (method + path), oldest
/// first — the §11.5 accounting surface the machine reads.
#[test]
fn call_log_records_every_http_call() -> Result<(), BoxError> {
    let (forge, transport) = forge();
    transport.enqueue(response(200, "[]"));
    transport.enqueue(response(201, &pr(9, "t", "b")));
    let spec = PrSpec {
        branch: branch(),
        title: "t".to_owned(),
        body: "b".to_owned(),
    };
    let _ = forge.upsert_pr(&spec)?;
    let calls: Vec<CallRecord> = forge.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(at(&calls, 0).method, "GET");
    assert!(
        at(&calls, 0)
            .target
            .starts_with(&format!("/repos/{OWNER}/{REPO}/pulls")),
        "the target is the request path: {}",
        at(&calls, 0).target
    );
    assert_eq!(at(&calls, 1).method, "POST");
    assert_eq!(forge.call_count("GET"), 1);
    Ok(())
}

/// The machine over the REST adapter (§11.4 "≤ 2 REST calls on the
/// update path"): the machine's find probe + the upsert's own
/// branch-lookup + the rewrite = 3 HTTP calls, and the second lookup
/// rides the cached `ETag` (a 304 under the primary limit). The budget
/// assertion pins the *shape* honestly instead of hiding the third call.
#[test]
fn machine_update_path_call_budget() -> Result<(), BoxError> {
    let (forge, transport) = forge();
    transport.enqueue(response(200, &pr_list(7, "old", "old")));
    transport.enqueue(response(200, &pr_list(7, "old", "old")));
    transport.enqueue(response(200, &pr(7, "new", "new")));
    let report = run_release_machine(
        &forge,
        &MachineInput {
            branch: branch(),
            pending_releases: Vec::new(),
            release_pr: Some(PrSpec {
                branch: branch(),
                title: "new".to_owned(),
                body: "new".to_owned(),
            }),
            content_commit: None,
        },
    )?;
    assert_eq!(
        report.calls, 3,
        "machine find + upsert find + PATCH, all observable"
    );
    let effects: Vec<String> = report
        .effects
        .iter()
        .map(|effect| format!("{effect:?}"))
        .collect();
    assert_eq!(effects.len(), 1);
    assert!(at(&effects, 0).contains("PrUpdated"), "{effects:?}");
    Ok(())
}

/// Without credentials every method fails closed with `E-FORGE-AUTH`
/// *before* any HTTP call — the shell's missing-token state is an auth
/// failure, never a network attempt.
#[test]
fn anonymous_forge_fails_closed_before_any_call() {
    let (forge, transport) = forge_anonymous();
    let err = forge
        .release_exists("core-v0.5.0")
        .expect_err("no token configured");
    assert_eq!(err.code(), ForgeError::AUTH_CODE, "{err}");
    assert!(
        err.to_string().contains("token"),
        "the reason names the missing credentials: {err}"
    );
    assert!(transport.sent().is_empty(), "nothing reached the transport");
}

/// A release branch outside the §11.2 shape is a caller bug — fail
/// closed with `E-FORGE-REQUEST` before any call (ADR-0013 decision 6).
#[test]
fn non_release_branch_is_a_caller_bug() {
    let (forge, transport) = forge();
    let err = forge
        .upsert_pr(&PrSpec {
            branch: BranchName::new("feature/odd")
                .unwrap_or_else(|error| unreachable!("valid branch name: {error}")),
            title: "t".to_owned(),
            body: "b".to_owned(),
        })
        .expect_err("no §11.2 prefix to strip the base from");
    assert_eq!(err.code(), ForgeError::REQUEST_CODE, "{err}");
    assert!(
        transport.sent().is_empty(),
        "the malformed spec never reaches the wire"
    );
}

/// The scripted request type is inspectable — the tests above ride
/// this; a compile-time pin that the vocabulary stays plain data.
#[test]
fn request_vocabulary_is_plain_data() {
    let request = HttpRequest {
        method: "GET",
        url: "https://api.github.com/x".to_owned(),
        headers: vec![("accept".to_owned(), "json".to_owned())],
        body: None,
    };
    assert_eq!(request.header("ACCEPT"), Some("json"));
    assert!(request.body.is_none());
}
