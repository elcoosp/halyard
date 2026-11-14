//! The transport seam contract (ADR-0013): plain-data
//! `HttpRequest`/`HttpResponse` vocabulary, the `HttpTransport` effect
//! seam (HTTP statuses are values; only connection-level failures are
//! errors) and the scripted `MemoryTransport` — the transport analogue
//! of `MemoryForge` (recorded script + sent-request log).

use halyard_forge::error::ForgeError;
use halyard_forge::transport::{HttpRequest, HttpResponse, HttpTransport, MemoryTransport};

/// House-style render helper: `?`-propagating (no `unwrap` in tests;
/// impossible paths are `unreachable!`).
fn transport() -> MemoryTransport {
    MemoryTransport::new()
}

fn response(status: u16, body: &str) -> HttpResponse {
    HttpResponse {
        status,
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body: body.to_owned(),
    }
}

fn request() -> HttpRequest {
    HttpRequest {
        method: "GET",
        url: "https://api.github.com/rate_limit".to_owned(),
        headers: vec![(
            "accept".to_owned(),
            "application/vnd.github+json".to_owned(),
        )],
        body: None,
    }
}

/// A scripted response passes through verbatim — statuses are data, the
/// transport never interprets them.
#[test]
fn scripted_response_passes_through() -> Result<(), ForgeError> {
    let t = transport();
    t.enqueue(response(404, "{}"));
    let out = t.execute(request())?;
    assert_eq!(out.status, 404);
    assert_eq!(out.body, "{}");
    assert_eq!(
        out.header("Content-Type"),
        Some("application/json"),
        "header lookup is case-insensitive"
    );
    Ok(())
}

/// The sent-request log records every executed request, oldest first —
/// the adapter's test/observability surface.
#[test]
fn sent_log_records_requests() -> Result<(), ForgeError> {
    let t = transport();
    t.enqueue(response(200, "{}"));
    t.enqueue(response(200, "{}"));
    let second = HttpRequest {
        method: "POST",
        url: "https://api.github.com/repos/o/r/pulls".to_owned(),
        headers: Vec::new(),
        body: Some("{\"head\":\"x\"}".to_owned()),
    };
    t.execute(request())?;
    t.execute(second)?;
    let sent = t.sent();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].method, "GET");
    assert_eq!(sent[1].method, "POST");
    assert_eq!(
        sent[1].body.as_deref(),
        Some("{\"head\":\"x\"}"),
        "the request body rides the log verbatim"
    );
    Ok(())
}

/// An injected failure passes through with its error code — the §15.2
/// failure injector at the transport layer.
#[test]
fn injected_failure_passes_through() {
    let t = transport();
    t.enqueue_failure(ForgeError::transport("connection reset by peer"));
    let err = t
        .execute(request())
        .expect_err("the scripted failure must surface");
    assert_eq!(err.code(), ForgeError::TRANSPORT_CODE, "{err}");
}

/// An exhausted script is a fail-closed transport error, never a panic —
/// a test that under-scripts sees the miss, not a fake success.
#[test]
fn exhausted_script_fails_closed() {
    let t = transport();
    let err = t.execute(request()).expect_err("an empty script must fail");
    assert_eq!(err.code(), ForgeError::TRANSPORT_CODE, "{err}");
    assert!(
        err.to_string().contains("script"),
        "the reason names the exhausted script: {err}"
    );
    assert!(
        t.sent().len() == 1,
        "the attempted request is still observable"
    );
}

/// The scripted transport records the *executed* request even when the
/// response is a failure — symmetric observability with `MemoryForge`
/// (§11.5: a dropped call is an unobservable one).
#[test]
fn failed_exchange_still_records_the_request() {
    let t = transport();
    t.enqueue_failure(ForgeError::rate_limited(30));
    let err = t.execute(request()).expect_err("scripted failure");
    assert_eq!(err.code(), ForgeError::RATE_LIMIT_CODE, "{err}");
    assert_eq!(t.sent().len(), 1);
}
