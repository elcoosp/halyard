//! The OIDC trusted-publishing contract (M5.3 step 3, ADR-0031): the
//! GitHub Actions runtime-context detection, the id-token request and
//! the crates.io token exchange over the forge transport vocabulary,
//! the [`PublishAuth`] fallback composition (the documented env grammar
//! first, the exchange only when nothing covers the target), the
//! dry-mode projection (no network), and the scrub contract for the
//! exchanged tokens. Written first (red on the unresolved
//! `halyard_publish` OIDC API), implemented second.
//!
//! Grammar sources are VERIFIED in
//! `docs/research/oidc-trusted-publishing.md` (the reference
//! `rust-lang/crates-io-auth-action` source and the actions toolkit's
//! `oidc-utils.ts` — fetched quotes). No test mutates the process
//! environment: the context reader and the transport are injected.

use std::collections::BTreeMap;
use std::sync::Arc;

use halyard_forge::MemoryTransport;
use halyard_publish::{
    AuthOutcome, EnvPublishAuth, OidcContext, OidcPublishAuth, PublishAuth, PublishTarget,
    audience_for, auth_summary, encode_audience, gha_oidc_context, tokens_endpoint,
};

const ID_TOKEN_BODY: &str = r#"{"value":"gha-jwt-value"}"#;
const EXCHANGE_BODY: &str = r#"{"token":"exchanged-token-value"}"#;

/// One cargo publish target on the default crates.io registry.
fn crates_io() -> PublishTarget {
    PublishTarget {
        package: "p1".to_owned(),
        version: "0.2.0".to_owned(),
        ecosystem: "cargo".to_owned(),
        registry: "crates.io".to_owned(),
        manifest_dir: "crates/p1".to_owned(),
        manifest: "crates/p1/Cargo.toml".to_owned(),
    }
}

/// One npm publish target (the other documented grammar family).
fn npm() -> PublishTarget {
    PublishTarget {
        package: "pkg".to_owned(),
        version: "0.2.0".to_owned(),
        ecosystem: "npm".to_owned(),
        registry: "npm".to_owned(),
        manifest_dir: "packages/pkg".to_owned(),
        manifest: "packages/pkg/package.json".to_owned(),
    }
}

/// One cargo publish target on a custom registry (outside trusted
/// publishing's crates.io scope).
fn custom_registry() -> PublishTarget {
    PublishTarget {
        package: "p2".to_owned(),
        version: "0.2.0".to_owned(),
        ecosystem: "cargo".to_owned(),
        registry: "corp".to_owned(),
        manifest_dir: "crates/p2".to_owned(),
        manifest: "crates/p2/Cargo.toml".to_owned(),
    }
}

/// An injected context reader over a plain table (never the process
/// environment).
fn reader(rows: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let table: BTreeMap<String, String> = rows
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect();
    move |name| table.get(name).cloned()
}

/// A context with both runtime variables present.
fn context() -> OidcContext {
    OidcContext {
        request_url: "https://actions.example.com/oidc?audience=".to_owned(),
        request_token: "request-token-value".to_owned(),
    }
}

/// A live provider over a scripted transport.
fn live_auth(base: EnvPublishAuth, transport: &MemoryTransport) -> OidcPublishAuth {
    OidcPublishAuth::new(
        base,
        Some(context()),
        Arc::new(transport.clone()),
        "https://crates.io".to_owned(),
        true,
    )
}

/// Scripts the documented happy path: the id-token GET, then the
/// exchange POST.
fn script_happy_path(transport: &MemoryTransport) {
    transport.enqueue(halyard_forge::HttpResponse {
        status: 200,
        headers: Vec::new(),
        body: ID_TOKEN_BODY.to_owned(),
    });
    transport.enqueue(halyard_forge::HttpResponse {
        status: 200,
        headers: Vec::new(),
        body: EXCHANGE_BODY.to_owned(),
    });
}

// ---------------------------------------------------------------------------
// Context detection
// ---------------------------------------------------------------------------

#[test]
fn context_detection_happy_path() {
    let detected = gha_oidc_context(&reader(&[
        ("ACTIONS_ID_TOKEN_REQUEST_URL", "https://u"),
        ("ACTIONS_ID_TOKEN_REQUEST_TOKEN", "t"),
    ]))
    .unwrap_or_else(|e| panic!("context: {e}"));
    let ctx = detected.expect("both variables present");
    assert_eq!(ctx.request_url, "https://u");
    // The request token is a secret: Debug and Display never render it.
    assert!(!format!("{ctx:?}").contains("t-va"));
    assert!(!format!("{ctx}").contains("t-va"));
}

#[test]
fn context_detection_absent_is_none() {
    let detected = gha_oidc_context(&reader(&[])).unwrap_or_else(|e| panic!("context: {e}"));
    assert!(
        detected.is_none(),
        "no GitHub Actions context is the normal shape"
    );
}

#[test]
fn context_detection_empty_variable_refuses() {
    // Present but empty is the seam's misconfiguration rule (the same
    // one the empty-token refusal uses): fail closed naming the
    // variable, before any network.
    let error = gha_oidc_context(&reader(&[
        ("ACTIONS_ID_TOKEN_REQUEST_URL", ""),
        ("ACTIONS_ID_TOKEN_REQUEST_TOKEN", "t"),
    ]))
    .expect_err("empty URL refuses");
    assert_eq!(error.code(), "E-PUB-AUTH");
    assert!(
        format!("{error}").contains("ACTIONS_ID_TOKEN_REQUEST_URL"),
        "the message names the variable: {error}"
    );
}

#[test]
fn context_detection_half_present_refuses() {
    // Exactly one variable is a half-configured workflow (id-token
    // write missing one of the two); the fix is named, the run refuses.
    let error = gha_oidc_context(&reader(&[("ACTIONS_ID_TOKEN_REQUEST_URL", "https://u")]))
        .expect_err("half context refuses");
    assert_eq!(error.code(), "E-PUB-AUTH");
    assert!(
        format!("{error}").contains("ACTIONS_ID_TOKEN_REQUEST_TOKEN"),
        "the message names the missing variable: {error}"
    );
}

// ---------------------------------------------------------------------------
// The documented URL grammar
// ---------------------------------------------------------------------------

#[test]
fn audience_strips_the_scheme() {
    // The reference action's rule: the audience is the registry URL
    // sans the https?:// scheme (crates.io's audience is `crates.io`).
    assert_eq!(audience_for("https://crates.io"), "crates.io");
    assert_eq!(
        audience_for("https://staging.crates.io"),
        "staging.crates.io"
    );
}

#[test]
fn audience_encoding_is_rfc3986_unreserved() {
    // The toolkit's `encodeURIComponent` shape (verified): reserved
    // characters are percent-encoded, unreserved pass through.
    assert_eq!(encode_audience("crates.io"), "crates.io");
    assert_eq!(encode_audience("a b"), "a%20b");
}

#[test]
fn tokens_endpoint_derivation() {
    assert_eq!(
        tokens_endpoint("https://crates.io"),
        "https://crates.io/api/v1/trusted_publishing/tokens"
    );
    // The action strips a trailing slash before composing the URL.
    assert_eq!(
        tokens_endpoint("https://staging.crates.io/"),
        "https://staging.crates.io/api/v1/trusted_publishing/tokens"
    );
}

// ---------------------------------------------------------------------------
// The exchange (live: enabled + context)
// ---------------------------------------------------------------------------

#[test]
fn exchange_happy_path_resolves_and_injects() {
    let transport = MemoryTransport::new();
    script_happy_path(&transport);
    let auth = live_auth(EnvPublishAuth::default(), &transport);
    let outcome = auth
        .credentials(&crates_io())
        .unwrap_or_else(|e| panic!("credentials: {e}"));
    let AuthOutcome::Token(credential) = outcome else {
        panic!("the exchange resolves a token, got {outcome:?}");
    };
    assert_eq!(credential.key, "CARGO_REGISTRY_TOKEN");
    assert_eq!(credential.value, "exchanged-token-value");
    // The documented two-call shape: the id-token GET (bearer, audience
    // query), then the exchange POST (JSON jwt body).
    let sent = transport.sent();
    assert_eq!(sent.len(), 2, "one id-token GET + one exchange POST");
    let id_request = sent.first().expect("the id-token request is recorded");
    assert_eq!(id_request.method, "GET");
    assert!(
        id_request.url.ends_with("&audience=crates.io"),
        "got {}",
        id_request.url
    );
    assert_eq!(
        id_request.header("Authorization"),
        Some("Bearer request-token-value")
    );
    let exchange = sent.get(1).expect("the exchange request is recorded");
    assert_eq!(exchange.method, "POST");
    assert_eq!(
        exchange.url,
        "https://crates.io/api/v1/trusted_publishing/tokens"
    );
    assert_eq!(exchange.header("Content-Type"), Some("application/json"));
    assert_eq!(exchange.body.as_deref(), Some(r#"{"jwt":"gha-jwt-value"}"#));
}

#[test]
fn exchange_is_memoized_per_registry() {
    // One exchange serves the whole run: a second crates.io target
    // reuses the memoized credential without new transport calls.
    let transport = MemoryTransport::new();
    script_happy_path(&transport);
    let auth = live_auth(EnvPublishAuth::default(), &transport);
    let first = auth
        .credentials(&crates_io())
        .unwrap_or_else(|e| panic!("first: {e}"));
    let second = auth
        .credentials(&crates_io())
        .unwrap_or_else(|e| panic!("second: {e}"));
    assert_eq!(first, second);
    assert_eq!(
        transport.sent().len(),
        2,
        "the memo absorbs the second target"
    );
}

#[test]
fn env_grammar_wins_over_oidc() {
    // An explicit CARGO_REGISTRY_TOKEN is never overridden by the
    // exchange: the documented grammar resolves first, zero calls.
    let transport = MemoryTransport::new();
    let base = EnvPublishAuth::from_table(table(&[("CARGO_REGISTRY_TOKEN", "env-token")]));
    let auth = live_auth(base, &transport);
    let outcome = auth
        .credentials(&crates_io())
        .unwrap_or_else(|e| panic!("credentials: {e}"));
    let AuthOutcome::Token(credential) = outcome else {
        panic!("the env token resolves, got {outcome:?}");
    };
    assert_eq!(credential.value, "env-token");
    assert!(transport.sent().is_empty(), "no exchange fired");
}

#[test]
fn oidc_scope_is_crates_io_only() {
    // Trusted publishing is GitHub-Actions + crates.io today (the
    // verified sources): npm targets and custom cargo registries keep
    // the documented grammar — honest Absent, zero calls.
    let transport = MemoryTransport::new();
    let auth = live_auth(EnvPublishAuth::default(), &transport);
    for target in [npm(), custom_registry()] {
        let outcome = auth
            .credentials(&target)
            .unwrap_or_else(|e| panic!("{}: {e}", target.registry));
        assert_eq!(
            outcome,
            AuthOutcome::Absent,
            "{} stays absent",
            target.registry
        );
    }
    assert!(
        transport.sent().is_empty(),
        "no exchange fires outside the crates.io scope"
    );
}

// ---------------------------------------------------------------------------
// Fail-closed exchange failures (all E-PUB-AUTH, all scrubbed)
// ---------------------------------------------------------------------------

#[test]
fn exchange_refuses_a_missing_value_field() {
    let transport = MemoryTransport::new();
    transport.enqueue(halyard_forge::HttpResponse {
        status: 200,
        headers: Vec::new(),
        body: r#"{"other":"field"}"#.to_owned(),
    });
    let auth = live_auth(EnvPublishAuth::default(), &transport);
    let error = auth
        .credentials(&crates_io())
        .expect_err("no value field refuses");
    assert_eq!(error.code(), "E-PUB-AUTH");
}

#[test]
fn exchange_refuses_a_registry_error_body_with_details() {
    // crates.io's documented error shape {"errors":[{"detail":…}]} is
    // parsed into the message; the id-token never appears in it.
    let transport = MemoryTransport::new();
    transport.enqueue(halyard_forge::HttpResponse {
        status: 200,
        headers: Vec::new(),
        body: ID_TOKEN_BODY.to_owned(),
    });
    transport.enqueue(halyard_forge::HttpResponse {
        status: 403,
        headers: Vec::new(),
        body: r#"{"errors":[{"detail":"not a trusted publisher"}]}"#.to_owned(),
    });
    let auth = live_auth(EnvPublishAuth::default(), &transport);
    let error = auth.credentials(&crates_io()).expect_err("403 refuses");
    assert_eq!(error.code(), "E-PUB-AUTH");
    let message = format!("{error}");
    assert!(
        message.contains("not a trusted publisher"),
        "the parsed detail rides: {message}"
    );
    assert!(
        !message.contains("gha-jwt-value"),
        "the id-token is scrubbed from the message: {message}"
    );
}

#[test]
fn exchange_refuses_a_missing_token_field() {
    let transport = MemoryTransport::new();
    transport.enqueue(halyard_forge::HttpResponse {
        status: 200,
        headers: Vec::new(),
        body: ID_TOKEN_BODY.to_owned(),
    });
    transport.enqueue(halyard_forge::HttpResponse {
        status: 200,
        headers: Vec::new(),
        body: r#"{"nope":true}"#.to_owned(),
    });
    let auth = live_auth(EnvPublishAuth::default(), &transport);
    let error = auth
        .credentials(&crates_io())
        .expect_err("no token field refuses");
    assert_eq!(error.code(), "E-PUB-AUTH");
}

#[test]
fn exchange_maps_transport_failures_fail_closed() {
    // A connection-level failure is not a silent absence — it refuses
    // with the seam's auth code and the transport's reason.
    let transport = MemoryTransport::new();
    transport.enqueue_failure(halyard_forge::ForgeError::transport("dns: no such host"));
    let auth = live_auth(EnvPublishAuth::default(), &transport);
    let error = auth
        .credentials(&crates_io())
        .expect_err("transport failure refuses");
    assert_eq!(error.code(), "E-PUB-AUTH");
    assert!(
        format!("{error}").contains("dns: no such host"),
        "the transport reason rides: {error}"
    );
}

#[test]
fn exchange_refuses_an_exhausted_script() {
    // No scripted responses: the exchange never pretends to succeed
    // (the reference fake's own exhaustion rule surfaces as E-PUB-AUTH).
    let transport = MemoryTransport::new();
    let auth = live_auth(EnvPublishAuth::default(), &transport);
    let error = auth
        .credentials(&crates_io())
        .expect_err("exhausted script refuses");
    assert_eq!(error.code(), "E-PUB-AUTH");
}

// ---------------------------------------------------------------------------
// Scrub contract
// ---------------------------------------------------------------------------

#[test]
fn secrets_cover_the_context_token_and_the_exchange() {
    // The scrubbing contract: a value `credentials` can resolve is a
    // value `secrets` lists — including the injected request token and
    // the exchanged registry token.
    let transport = MemoryTransport::new();
    script_happy_path(&transport);
    let auth = live_auth(EnvPublishAuth::default(), &transport);
    let before = auth.secrets();
    assert!(
        before.contains(&"request-token-value".to_owned()),
        "the context token is scrub-listed before any exchange"
    );
    auth.credentials(&crates_io())
        .unwrap_or_else(|e| panic!("credentials: {e}"));
    let after = auth.secrets();
    assert!(
        after.contains(&"exchanged-token-value".to_owned()),
        "the exchanged token is scrub-listed once resolved: {after:?}"
    );
}

// ---------------------------------------------------------------------------
// The dry projection + provenance (no network)
// ---------------------------------------------------------------------------

#[test]
fn dry_mode_projects_without_network() {
    // PublishMode::Dry disables the exchange (the forecast touches no
    // network, I5) but keeps the provenance projection honest.
    let transport = MemoryTransport::new();
    let auth = OidcPublishAuth::new(
        EnvPublishAuth::default(),
        Some(context()),
        Arc::new(transport.clone()),
        "https://crates.io".to_owned(),
        false,
    );
    let outcome = auth
        .credentials(&crates_io())
        .unwrap_or_else(|e| panic!("credentials: {e}"));
    assert_eq!(
        outcome,
        AuthOutcome::Absent,
        "the dry spawn stays tokenless"
    );
    assert!(transport.sent().is_empty(), "zero calls in dry");
    assert_eq!(
        auth.provenance(&crates_io())
            .unwrap_or_else(|e| panic!("provenance: {e}")),
        "oidc:crates.io"
    );
}

#[test]
fn provenance_names_the_oidc_source_without_firing_it() {
    // The summary's projection: oidc:<audience> for an eligible target,
    // resolved from the provider alone — zero transport calls.
    let transport = MemoryTransport::new();
    let auth = live_auth(EnvPublishAuth::default(), &transport);
    assert_eq!(
        auth.provenance(&crates_io())
            .unwrap_or_else(|e| panic!("provenance: {e}")),
        "oidc:crates.io"
    );
    assert!(
        transport.sent().is_empty(),
        "the projection never exchanges"
    );
}

#[test]
fn provenance_still_spells_the_env_grammar() {
    let transport = MemoryTransport::new();
    let base = EnvPublishAuth::from_table(table(&[("CARGO_REGISTRY_TOKEN", "env-token")]));
    let auth = live_auth(base, &transport);
    assert_eq!(
        auth.provenance(&crates_io())
            .unwrap_or_else(|e| panic!("provenance: {e}")),
        "env:CARGO_REGISTRY_TOKEN"
    );
    assert_eq!(
        auth.provenance(&custom_registry())
            .unwrap_or_else(|e| panic!("provenance: {e}")),
        "none"
    );
}

#[test]
fn auth_summary_projects_oidc_provenance() {
    // The renderers' input (M5.3 step 2) rides the provider's own
    // provenance: an OIDC-eligible registry renders `oidc:<audience>`,
    // others keep the env/none spellings.
    let transport = MemoryTransport::new();
    let auth = live_auth(EnvPublishAuth::default(), &transport);
    let summary =
        auth_summary(&auth, &[crates_io(), npm()]).unwrap_or_else(|e| panic!("summary: {e}"));
    assert_eq!(
        summary.get("crates.io").map(String::as_str),
        Some("oidc:crates.io")
    );
    assert_eq!(summary.get("npm").map(String::as_str), Some("none"));
    assert!(
        transport.sent().is_empty(),
        "the summary is a projection, never an exchange"
    );
}

#[test]
fn exchanged_credential_never_leaks_through_debug() {
    let transport = MemoryTransport::new();
    script_happy_path(&transport);
    let auth = live_auth(EnvPublishAuth::default(), &transport);
    auth.credentials(&crates_io())
        .unwrap_or_else(|e| panic!("credentials: {e}"));
    assert!(
        !format!("{auth:?}").contains("exchanged-token-value"),
        "the exchanged token stays out of the provider's Debug"
    );
}

/// Builds a name→value table (the tests' base-provider injection).
fn table(rows: &[(&str, &str)]) -> BTreeMap<String, String> {
    rows.iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}
