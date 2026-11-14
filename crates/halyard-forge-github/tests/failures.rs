//! The REST failure-surface matrix (M4.1b): GitHub statuses map onto
//! the `E-FORGE-*` taxonomy per `docs/research/github-rest-forge.md`
//! §5 — auth, rate limiting (with `retry-after` / reset semantics),
//! conflicts (the I1 race and the non-fast-forward ref update), caller
//! bugs, server-side transport failures and injected connection
//! failures — plus the I2 re-run completing after a mid-sequence
//! failure.

use halyard_forge::transport::{HttpResponse, MemoryTransport};
use halyard_forge::{BranchName, Forge, ForgeError, PrSpec, ReleaseSpec};
use halyard_forge_github::{ForgeEndpoint, GitHubRestForge};

type BoxError = Box<dyn std::error::Error>;

const OWNER: &str = "octo";
const REPO: &str = "halyard-demo";
const BRANCH: &str = "halyard/release/main";

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

/// `401` — bad credentials → `E-FORGE-AUTH`, the server's message in
/// the reason (never the token).
#[test]
fn unauthorized_maps_to_auth() {
    let (forge, transport) = forge();
    transport.enqueue(response(401, "{\"message\":\"Bad credentials\"}"));
    let err = forge
        .find_release_pr(&branch())
        .expect_err("bad credentials");
    assert_eq!(err.code(), ForgeError::AUTH_CODE, "{err}");
    assert!(
        err.to_string().contains("Bad credentials"),
        "the server message is surfaced: {err}"
    );
}

/// `403` with a `retry-after` header — the secondary-limit shape the
/// research pins — maps to `E-FORGE-RATE-LIMIT` with the passed wait.
#[test]
fn secondary_rate_limit_honors_retry_after() {
    let (forge, transport) = forge();
    transport.enqueue(response_with_header(
        403,
        ("retry-after", "42"),
        "{\"message\":\"You have exceeded a secondary rate limit\"}",
    ));
    let err = forge
        .find_release_pr(&branch())
        .expect_err("secondary limit");
    assert_eq!(err.code(), ForgeError::RATE_LIMIT_CODE, "{err}");
    assert!(
        err.to_string().contains("42s"),
        "the suggested wait surfaces: {err}"
    );
}

/// `403` with `x-ratelimit-remaining: 0` and no `retry-after` — the
/// primary-limit shape — maps to `E-FORGE-RATE-LIMIT` with a wait
/// derived from `x-ratelimit-reset`.
#[test]
fn primary_rate_limit_derives_the_wait_from_reset() {
    let (forge, transport) = forge();
    transport.enqueue(response_with_header(
        403,
        ("x-ratelimit-remaining", "0"),
        "{\"message\":\"API rate limit exceeded\"}",
    ));
    let err = forge.find_release_pr(&branch()).expect_err("primary limit");
    assert_eq!(err.code(), ForgeError::RATE_LIMIT_CODE, "{err}");
    let ForgeError::RateLimited { retry_after_secs } = err else {
        unreachable!("asserted above");
    };
    assert!(
        retry_after_secs > 0 && retry_after_secs <= 3600,
        "the reset-derived wait is a sane hour-window value: {retry_after_secs}"
    );
}

/// A plain `403` without rate-limit headers is a scope rejection →
/// `E-FORGE-AUTH` (the credentials lack the required access).
#[test]
fn forbidden_without_rate_headers_is_auth() {
    let (forge, transport) = forge();
    transport.enqueue(response(
        403,
        "{\"message\":\"Resource not accessible by integration\"}",
    ));
    let err = forge
        .find_release_pr(&branch())
        .expect_err("scope rejection");
    assert_eq!(err.code(), ForgeError::AUTH_CODE, "{err}");
}

/// `429` — the other documented rate-limit status — maps to
/// `E-FORGE-RATE-LIMIT` (with the header wait when present).
#[test]
fn too_many_requests_maps_to_rate_limit() {
    let (forge, transport) = forge();
    transport.enqueue(response(429, "{\"message\":\"Too many requests\"}"));
    let err = forge.find_release_pr(&branch()).expect_err("throttled");
    assert_eq!(err.code(), ForgeError::RATE_LIMIT_CODE, "{err}");
}

/// `404` on a repo-scoped *collection* probe is the documented
/// indistinguishable missing-or-forbidden shape → `E-FORGE-AUTH`
/// (research §5), while `404` on the release-by-tag probe stays the
/// not-found *value*.
#[test]
fn not_found_semantics_differ_per_probe() -> Result<(), BoxError> {
    let (forge, transport) = forge();
    transport.enqueue(response(404, "{\"message\":\"Not Found\"}"));
    let err = forge
        .find_release_pr(&branch())
        .expect_err("the repo is invisible to the credentials");
    assert_eq!(err.code(), ForgeError::AUTH_CODE, "{err}");
    transport.enqueue(response(404, "{\"message\":\"Not Found\"}"));
    assert!(
        !forge.release_exists("core-v0.5.0")?,
        "a missing release is a value, not an error (§11.4)"
    );
    Ok(())
}

/// `5xx` is a server-side transport failure (`E-FORGE-TRANSPORT`) —
/// retryable at the run level, never a caller bug.
#[test]
fn server_errors_map_to_transport() {
    let (forge, transport) = forge();
    transport.enqueue(response(502, "{\"message\":\"Bad gateway\"}"));
    let err = forge
        .find_release_pr(&branch())
        .expect_err("server failure");
    assert_eq!(err.code(), ForgeError::TRANSPORT_CODE, "{err}");
}

/// An unexpected `304` (no conditional request was sent) is a protocol
/// violation — fail closed, never fabricate a cached value.
#[test]
fn unrequested_304_fails_closed() {
    let (forge, transport) = forge();
    transport.enqueue(response(304, ""));
    let err = forge
        .find_release_pr(&branch())
        .expect_err("no conditional was sent, so 304 is a violation");
    assert_eq!(err.code(), ForgeError::REQUEST_CODE, "{err}");
}

/// The GET/POST race on release creation (a `422` whose error codes say
/// `already_exists`) is the I1 conflict: re-run, never force.
#[test]
fn create_release_race_maps_to_conflict() {
    let (forge, transport) = forge();
    transport.enqueue(response(404, "{\"message\":\"Not Found\"}"));
    transport.enqueue(response(
        422,
        r#"{"message":"Validation Failed","errors":[{"resource":"Release","code":"already_exists","field":"tag_name"}]}"#,
    ));
    let err = forge
        .create_release(&ReleaseSpec {
            tag: "core-v0.5.0".to_owned(),
            notes: "notes".to_owned(),
        })
        .expect_err("another actor created the release first");
    assert_eq!(err.code(), ForgeError::CONFLICT_CODE, "{err}");
}

/// A non-fast-forward ref update (`409`) is the CAS conflict: the
/// branch moved under us — re-plan, never force (§11.2).
#[test]
fn non_fast_forward_ref_update_maps_to_conflict() {
    let (forge, transport) = forge();
    for status in [409_u16, 422] {
        transport.enqueue(response(
            200,
            r#"{"ref":"refs/heads/x","object":{"sha":"1111111111111111111111111111111111111111","type":"commit"}}"#,
        ));
        transport.enqueue(response(
            200,
            r#"{"sha":"1111111111111111111111111111111111111111","tree":{"sha":"2222222222222222222222222222222222222222"}}"#,
        ));
        transport.enqueue(response(
            201,
            r#"{"sha":"3333333333333333333333333333333333333333"}"#,
        ));
        transport.enqueue(response(
            201,
            r#"{"sha":"4444444444444444444444444444444444444444"}"#,
        ));
        transport.enqueue(response(
            status,
            "{\"message\":\"Update is not a fast forward\"}",
        ));
        let err = forge
            .commit_via_api(&halyard_forge::ApiCommit {
                branch: branch(),
                message: "m".to_owned(),
                files: vec![halyard_forge::ApiFile {
                    path: "Cargo.toml".to_owned(),
                    content: b"[package]\n".to_vec(),
                }],
            })
            .expect_err("the branch moved under us");
        assert_eq!(
            err.code(),
            ForgeError::CONFLICT_CODE,
            "status {status}: {err}"
        );
    }
}

/// A `422` on PR creation without `already_exists` is a caller bug —
/// `E-FORGE-REQUEST` (fail-closed, never retried).
#[test]
fn validation_failure_maps_to_request() {
    let (forge, transport) = forge();
    transport.enqueue(response(200, "[]"));
    transport.enqueue(response(
        422,
        r#"{"message":"Validation Failed","errors":[{"resource":"PullRequest","field":"title","code":"missing_field"}]}"#,
    ));
    let err = forge
        .upsert_pr(&PrSpec {
            branch: branch(),
            title: String::new(),
            body: "b".to_owned(),
        })
        .expect_err("the forge rejects the spec");
    assert_eq!(err.code(), ForgeError::REQUEST_CODE, "{err}");
}

/// A connection-level failure rides the transport seam verbatim
/// (`E-FORGE-TRANSPORT`) — the machine's fail-closed contract treats it
/// like any other code.
#[test]
fn connection_failures_pass_through_as_transport() {
    let (forge, transport) = forge();
    transport.enqueue_failure(ForgeError::transport("dns lookup failed"));
    let err = forge
        .find_release_pr(&branch())
        .expect_err("connection failure");
    assert_eq!(err.code(), ForgeError::TRANSPORT_CODE, "{err}");
}

/// I2 resumability at the adapter level: a mid-sequence server failure
/// aborts the pass; the re-run completes to the same end state with no
/// duplicate release.
#[test]
fn failed_pass_resumes_cleanly() -> Result<(), BoxError> {
    let (forge, transport) = forge();
    // Pass 1: GET says missing, the POST dies on the server.
    transport.enqueue(response(404, "{\"message\":\"Not Found\"}"));
    transport.enqueue(response(500, "{\"message\":\"boom\"}"));
    let spec = ReleaseSpec {
        tag: "core-v0.5.0".to_owned(),
        notes: "notes".to_owned(),
    };
    let err = forge.create_release(&spec).expect_err("server failure");
    assert_eq!(err.code(), ForgeError::TRANSPORT_CODE, "{err}");
    // Pass 2: the re-run — the release did not land, the POST succeeds.
    transport.enqueue(response(404, "{\"message\":\"Not Found\"}"));
    transport.enqueue(response(
        201,
        r#"{"id":1,"tag_name":"core-v0.5.0","html_url":"https://github.com/octo/halyard-demo/releases/tag/core-v0.5.0"}"#,
    ));
    let release = forge.create_release(&spec)?;
    assert!(release.created, "the re-run completed the effect");
    let posts = transport
        .sent()
        .iter()
        .filter(|req| req.method == "POST")
        .count();
    assert_eq!(posts, 2, "one failed attempt + one completing attempt");
    // Pass 3: the I1 re-run finds the release; nothing is re-created.
    transport.enqueue(response(
        200,
        r#"{"id":1,"tag_name":"core-v0.5.0","html_url":"https://github.com/octo/halyard-demo/releases/tag/core-v0.5.0"}"#,
    ));
    let rerun = forge.create_release(&spec)?;
    assert!(!rerun.created);
    Ok(())
}
