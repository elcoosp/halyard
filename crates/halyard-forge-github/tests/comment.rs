//! The `upsert_pr_comment` wire contract (M4.5 step 1, §11.6): the
//! adapter's marker-based comment upsert over the scripted
//! `MemoryTransport` — the create path (scan → `POST`), the rewrite
//! path (scan → `PATCH`), the deterministic pagination bound, the
//! fail-closed spec validation and the §11.5 call log.

use halyard_forge::transport::{HttpRequest, HttpResponse, MemoryTransport};
use halyard_forge::{CommentSpec, Forge, ForgeError};
use halyard_forge_github::{ForgeEndpoint, GitHubRestForge};

const OWNER: &str = "octo";
const REPO: &str = "halyard-demo";
const MARKER: &str = "<!-- halyard:check v1 -->";

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

fn response(status: u16, body: &str) -> HttpResponse {
    HttpResponse {
        status,
        headers: Vec::new(),
        body: body.to_owned(),
    }
}

/// The Nth scripted item (the `indexing_slicing`-safe accessor the
/// contract tests share).
fn scripted(sent: &[HttpRequest], index: usize) -> &HttpRequest {
    sent.get(index)
        .unwrap_or_else(|| unreachable!("the script logged {index} requests"))
}

fn spec(pr_number: u64, body: &str) -> CommentSpec {
    CommentSpec {
        pr_number,
        marker: MARKER.to_owned(),
        body: format!("{MARKER}\n\n{body}"),
    }
}

#[test]
fn comment_create_scans_then_posts() -> Result<(), Box<dyn std::error::Error>> {
    let (forge, transport) = forge();
    transport.enqueue(response(200, "[]"));
    transport.enqueue(response(
        201,
        r#"{"id":9,"html_url":"https://github.com/octo/halyard-demo/pull/1#issuecomment-9"}"#,
    ));
    let comment = forge.upsert_pr_comment(&spec(1, "if merged, this will release"))?;
    assert!(comment.created, "an empty scan means the fresh path");
    assert!(
        comment.url.contains("issuecomment-9"),
        "the forge's comment URL rides verbatim"
    );
    let sent = transport.sent();
    let get = scripted(&sent, 0);
    assert_eq!(get.method, "GET");
    assert_eq!(
        get.url,
        "https://api.github.com/repos/octo/halyard-demo/issues/1/comments?per_page=100&page=1"
    );
    let post = scripted(&sent, 1);
    assert_eq!(post.method, "POST");
    assert_eq!(
        post.url,
        "https://api.github.com/repos/octo/halyard-demo/issues/1/comments"
    );
    let body = post.body.as_deref().unwrap_or_default();
    assert!(body.contains(MARKER), "the posted body carries the marker");
    assert!(body.contains("if merged, this will release"));
    Ok(())
}

#[test]
fn comment_rewrite_scans_then_patches_in_place() -> Result<(), Box<dyn std::error::Error>> {
    let (forge, transport) = forge();
    transport.enqueue(response(
        200,
        r#"[
            {"id":4,"html_url":"https://github.com/octo/halyard-demo/pull/1#issuecomment-4","body":"a human's note"},
            {"id":9,"html_url":"https://github.com/octo/halyard-demo/pull/1#issuecomment-9","body":"<!-- halyard:check v1 -->\n\nstale verdict"}
        ]"#,
    ));
    transport.enqueue(response(
        200,
        r#"{"id":9,"html_url":"https://github.com/octo/halyard-demo/pull/1#issuecomment-9"}"#,
    ));
    let comment = forge.upsert_pr_comment(&spec(1, "fresh verdict"))?;
    assert!(!comment.created, "the marker hit rewrites in place (I1)");
    assert_eq!(
        comment.url, "https://github.com/octo/halyard-demo/pull/1#issuecomment-9",
        "the rewritten comment keeps its URL"
    );
    let sent = transport.sent();
    let patch = scripted(&sent, 1);
    assert_eq!(patch.method, "PATCH");
    assert_eq!(
        patch.url, "https://api.github.com/repos/octo/halyard-demo/issues/comments/9",
        "the patch addresses the found comment by id"
    );
    let body = patch.body.as_deref().unwrap_or_default();
    assert!(
        body.contains("fresh verdict"),
        "the rewrite carries the new body"
    );
    Ok(())
}

#[test]
fn comment_scan_walks_pages_until_a_short_page() -> Result<(), Box<dyn std::error::Error>> {
    let (forge, transport) = forge();
    // A full first page (100 comments) with no marker, then a short
    // second page — the scan is a deterministic ascending walk.
    let page: Vec<String> = (0..100)
        .map(|index| format!(r#"{{"id":{index},"body":"filler {index}"}}"#))
        .collect();
    transport.enqueue(response(200, &format!("[{}]", page.join(","))));
    transport.enqueue(response(200, "[]"));
    transport.enqueue(response(
        201,
        r#"{"id":99,"html_url":"https://github.com/octo/halyard-demo/pull/1#issuecomment-99"}"#,
    ));
    let comment = forge.upsert_pr_comment(&spec(1, "verdict"))?;
    assert!(comment.created);
    let sent = transport.sent();
    let first = scripted(&sent, 0);
    assert!(first.url.ends_with("page=1"), "the walk starts at page 1");
    let second = scripted(&sent, 1);
    assert_eq!(second.method, "GET");
    assert!(
        second.url.ends_with("page=2"),
        "a full page continues the walk"
    );
    let post = scripted(&sent, 2);
    assert_eq!(post.method, "POST", "a short page ends the walk → create");
    Ok(())
}

#[test]
fn comment_specs_fail_closed_before_any_exchange() {
    let (forge, transport) = forge();
    let invalid = CommentSpec {
        pr_number: 1,
        marker: String::new(),
        body: "body".to_owned(),
    };
    let error = forge
        .upsert_pr_comment(&invalid)
        .err()
        .unwrap_or_else(|| unreachable!("the spec is invalid by construction"));
    assert!(
        matches!(error, ForgeError::Request { .. }),
        "spec validation is `E-FORGE-REQUEST`, got {error}"
    );
    assert!(
        transport.sent().is_empty(),
        "an invalid spec never reaches the wire"
    );
}

#[test]
fn comment_transport_failures_surface_verbatim() {
    let (forge, transport) = forge();
    transport.enqueue_failure(ForgeError::transport("loopback refused"));
    let error = forge
        .upsert_pr_comment(&spec(1, "verdict"))
        .err()
        .unwrap_or_else(|| unreachable!("the transport is scripted to fail"));
    assert!(
        matches!(error, ForgeError::Transport { .. }),
        "the transport failure is never swallowed, got {error}"
    );
}

#[test]
fn comment_error_statuses_map_onto_the_taxonomy() {
    let (forge, transport) = forge();
    transport.enqueue(response(404, r#"{"message":"Not Found"}"#));
    let error = forge
        .upsert_pr_comment(&spec(404, "verdict"))
        .err()
        .unwrap_or_else(|| unreachable!("404 is scripted"));
    // The adapter's documented 404 semantics (research §5: GitHub
    // reports missing and forbidden alike on repo-scoped probes) — the
    // caller-bug guard for unknown PR numbers lives at the seam level
    // (`CommentSpec::validate` + the reference adapter's store check).
    assert!(
        matches!(error, ForgeError::Auth { .. }),
        "the 404 rides the adapter's mapping, never a fabricated success, got {error}"
    );
}

#[test]
fn comment_calls_are_recorded_for_the_budget_summary() -> Result<(), Box<dyn std::error::Error>> {
    let (forge, transport) = forge();
    transport.enqueue(response(200, "[]"));
    transport.enqueue(response(
        201,
        r#"{"id":9,"html_url":"https://github.com/octo/halyard-demo/pull/1#issuecomment-9"}"#,
    ));
    forge.upsert_pr_comment(&spec(1, "verdict"))?;
    let calls = forge.calls();
    let methods: Vec<&str> = calls.iter().map(|call| call.method).collect();
    assert!(
        methods.contains(&"GET") && methods.contains(&"POST"),
        "both exchanges are observable (§11.5), got {methods:?}"
    );
    Ok(())
}
