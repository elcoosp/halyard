//! The App-JWT auth contract (M4.1c, §11.5, ADR-0014): the GitHub App
//! credential path — `AppCredentials` (PKCS#1/PKCS#8 PEM → RS256 signer),
//! the JWT mint, the installation-token exchange and the adapter's
//! cache/re-mint behavior — driven over a scripted
//! [`MemoryTransport`](halyard_forge::MemoryTransport) with a fixed
//! clock. Wire shapes per the §7 verification section of
//! `docs/research/github-rest-forge.md`.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;

use base64::Engine as _;
use halyard_forge::transport::{HttpResponse, MemoryTransport};
use halyard_forge::{Forge, ForgeError};
use halyard_forge_github::{AppCredentials, ForgeCredentials, ForgeEndpoint, GitHubRestForge};

type BoxError = Box<dyn std::error::Error>;

const OWNER: &str = "octo";
const REPO: &str = "halyard-demo";
const INSTALLATION: u64 = 42;
const T0: u64 = 1_700_000_000;

/// The test-only RSA fixture (never a production credential; both PEM
/// shapes carry the same key so the two `from_pem` paths are equivalent).
const PEM_PKCS1: &str = include_str!("fixtures/app_key_pkcs1.pem");
const PEM_PKCS8: &str = include_str!("fixtures/app_key_pkcs8.pem");

/// Wraps `Mutex::lock` in the house no-panic style (the state is only
/// corrupted by an already-panicking thread; the panic then propagates
/// from the original site).
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A fixed clock (`T0`), or a mutable one shared with the test.
fn fixed_clock() -> halyard_forge_github::EpochClock {
    Arc::new(move || T0)
}

fn mutable_clock() -> (halyard_forge_github::EpochClock, Arc<Mutex<u64>>) {
    let shared = Arc::new(Mutex::new(T0));
    let handle = Arc::clone(&shared);
    (Arc::new(move || *lock(&handle)), shared)
}

fn endpoint() -> ForgeEndpoint {
    ForgeEndpoint::with_base(OWNER, REPO, "https://api.github.com")
        .unwrap_or_else(|error| unreachable!("static endpoint is valid: {error}"))
}

fn app_credentials() -> AppCredentials {
    AppCredentials::from_pem("12345", PEM_PKCS1, INSTALLATION)
        .unwrap_or_else(|error| unreachable!("the fixture key is a valid RSA key: {error}"))
}

fn response(status: u16, body: &str) -> HttpResponse {
    HttpResponse {
        status,
        headers: Vec::new(),
        body: body.to_owned(),
    }
}

fn mint_body(token: &str) -> String {
    format!(
        r#"{{"token":"{token}","expires_at":"2030-01-01T00:00:00Z","permissions":{{"contents":"write"}}}}"#
    )
}

fn empty_pr_list() -> HttpResponse {
    response(200, "[]")
}

/// Decodes a JWT's `header.payload` into `(claims_json, signed_input,
/// signature_bytes)` — the assertion surface for the minted token.
fn decode_jwt(jwt: &str) -> Result<(String, String, Vec<u8>), BoxError> {
    let mut parts = jwt.split('.');
    let header = parts.next().ok_or("jwt: missing header")?;
    let payload = parts.next().ok_or("jwt: missing payload")?;
    let signature = parts.next().ok_or("jwt: missing signature")?;
    if parts.next().is_some() {
        return Err("jwt: more than three segments".into());
    }
    let payload_json =
        String::from_utf8(base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload)?)?;
    let signed_input = format!("{header}.{payload}");
    let sig = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(signature)?;
    Ok((payload_json, signed_input, sig))
}

// --- credential construction -------------------------------------------------

/// Both PEM shapes produce the same signing identity, and the public
/// components round-trip (the conformance-test accessor the signature
/// verification below relies on).
#[test]
fn from_pem_accepts_pkcs1_and_pkcs8_shapes() -> Result<(), BoxError> {
    let from_pkcs1 = AppCredentials::from_pem("12345", PEM_PKCS1, INSTALLATION)?;
    let from_pkcs8 = AppCredentials::from_pem("12345", PEM_PKCS8, INSTALLATION)?;
    assert_eq!(from_pkcs1.app_id(), "12345");
    assert_eq!(from_pkcs1.installation_id(), INSTALLATION);
    assert_eq!(
        from_pkcs1.public_components(),
        from_pkcs8.public_components(),
        "both PEM shapes carry the same key pair"
    );
    Ok(())
}

/// Garbage input fails closed with `E-FORGE-AUTH` — never a synthetic
/// signer, never a panic.
#[test]
fn from_pem_fails_closed_on_unusable_input() -> Result<(), BoxError> {
    let cases: Vec<(&str, String)> = vec![
        ("empty app id", String::new()),
        ("garbage pem", "not a pem at all".to_owned()),
        (
            "foreign pem label",
            "-----BEGIN FOO-----\nAAAA\n-----END FOO-----".to_owned(),
        ),
        ("truncated pem", {
            let mut cut = PEM_PKCS1.lines().take(5).collect::<Vec<_>>().join("\n");
            cut.push_str("\n-----END RSA PRIVATE KEY-----\n");
            cut
        }),
        ("non-key pem", {
            // A validly armored blob that is not an RSA key.
            use base64::engine::general_purpose::STANDARD as B64;
            let der = B64.encode(b"not a key");
            format!("-----BEGIN RSA PRIVATE KEY-----\n{der}\n-----END RSA PRIVATE KEY-----")
        }),
    ];
    for (label, pem) in cases {
        let error = AppCredentials::from_pem("12345", &pem, INSTALLATION)
            .err()
            .ok_or_else(|| format!("{label}: expected a fail-closed error"))?;
        assert_eq!(error.code(), ForgeError::AUTH_CODE, "{label}: {error}");
    }
    // A non-positive installation id is a caller bug, fail-closed too.
    let error = AppCredentials::from_pem("12345", PEM_PKCS1, 0)
        .err()
        .ok_or("zero installation id must be rejected")?;
    assert_eq!(error.code(), ForgeError::AUTH_CODE, "{error}");
    Ok(())
}

/// The credential enum reports its mode — the observability surface the
/// CLI reports (doctor / reports).
#[test]
fn credentials_report_their_kind() {
    assert_eq!(ForgeCredentials::from_token("t").kind(), "token");
    assert_eq!(
        ForgeCredentials::App(Box::new(app_credentials())).kind(),
        "app"
    );
}

// --- the JWT mint ------------------------------------------------------------

/// The minted JWT: RS256 header, the verified claims (`iat` 60 s in the
/// past, `exp` ≤ 10 min out — the documented window), and a signature
/// that verifies against the key's public components.
#[test]
fn mint_jwt_claims_and_signature_verify() -> Result<(), BoxError> {
    let app = app_credentials();
    let jwt = app.mint_jwt(T0)?;
    let (claims, signed_input, signature) = decode_jwt(&jwt)?;
    let claims: serde_json::Value = serde_json::from_str(&claims)?;
    assert_eq!(
        claims.get("iss").and_then(serde_json::Value::as_str),
        Some("12345"),
        "the issuer is the App id"
    );
    let iat = claims
        .get("iat")
        .and_then(serde_json::Value::as_u64)
        .ok_or("iat claim missing")?;
    let exp = claims
        .get("exp")
        .and_then(serde_json::Value::as_u64)
        .ok_or("exp claim missing")?;
    assert_eq!(iat, T0 - 60, "iat rides 60 s in the past (drift guard)");
    assert_eq!(
        exp - iat,
        600,
        "the JWT window is the documented 10-minute max"
    );
    assert!(exp > T0, "exp is in the future");
    // RS256 verification against the parsed public key (round trip).
    let (n, e) = app.public_components();
    let key = ring::signature::RsaPublicKeyComponents { n: &n, e: &e };
    key.verify(
        &ring::signature::RSA_PKCS1_2048_8192_SHA256,
        signed_input.as_bytes(),
        &signature,
    )
    .map_err(|_| "the minted JWT signature does not verify against the public key")?;
    Ok(())
}

/// The JWT is a function of (claims, key) only — the same `now` mints a
/// byte-identical token apart from the PKCS#1 blinding-randomized
/// signature, so the *claims* (the deterministic part) are pinned twice.
#[test]
fn mint_jwt_is_deterministic_in_claims() -> Result<(), BoxError> {
    let app = app_credentials();
    let first = app.mint_jwt(T0)?;
    let second = app.mint_jwt(T0)?;
    let (a, _, _) = decode_jwt(&first)?;
    let (b, _, _) = decode_jwt(&second)?;
    assert_eq!(a, b, "same clock ⇒ same claims (I3)");
    Ok(())
}

// --- the adapter's App path --------------------------------------------------

fn app_forge_with_script(
    script: Vec<HttpResponse>,
) -> (GitHubRestForge<MemoryTransport>, MemoryTransport) {
    let transport = MemoryTransport::new();
    for item in script {
        transport.enqueue(item);
    }
    let forge = GitHubRestForge::with_credentials_and_clock(
        endpoint(),
        ForgeCredentials::App(Box::new(app_credentials())),
        transport.clone(),
        fixed_clock(),
    );
    (forge, transport)
}

/// The first App-authenticated call mints the installation token (one
/// POST to the documented endpoint with the JWT as bearer) and then
/// carries the minted token on the repo call.
#[test]
fn app_auth_mints_before_the_first_repo_call() -> Result<(), BoxError> {
    let (forge, transport) = app_forge_with_script(vec![
        response(201, &mint_body("installation-token-1")),
        empty_pr_list(),
    ]);
    let branch = halyard_forge::BranchName::new("halyard/release/main")
        .unwrap_or_else(|error| unreachable!("static branch is valid: {error}"));
    let found = forge.find_release_pr(&branch)?;
    assert!(found.is_none());
    let sent = transport.sent();
    assert_eq!(sent.len(), 2, "mint + probe only");
    let mint = sent.first().ok_or("mint request recorded")?;
    assert_eq!(mint.method, "POST");
    assert_eq!(
        mint.url, "https://api.github.com/app/installations/42/access_tokens",
        "the mint posts to the documented endpoint"
    );
    let authorization = mint
        .header("Authorization")
        .ok_or("the mint carries a bearer")?
        .to_owned();
    let jwt = authorization
        .strip_prefix("Bearer ")
        .ok_or("the mint rides Authorization: Bearer <jwt>")?
        .to_owned();
    let (claims, _, _) = decode_jwt(&jwt)?;
    let claims: serde_json::Value = serde_json::from_str(&claims)?;
    assert_eq!(
        claims.get("iss").and_then(serde_json::Value::as_str),
        Some("12345")
    );
    assert_eq!(mint.header("Accept"), Some("application/vnd.github+json"));
    let probe = sent.get(1).ok_or("probe request recorded")?;
    assert_eq!(
        probe.header("Authorization"),
        Some("Bearer installation-token-1"),
        "the repo call carries the minted installation token"
    );
    // The §11.5 call log records the mint like any other call.
    let calls = forge.calls();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(
        calls.first().ok_or("first call recorded")?.target,
        "/app/installations/42/access_tokens"
    );
    Ok(())
}

/// Within the conservative TTL the minted token is reused — the re-run
/// spends no additional mint (§11.5 budget: the token mint is not a
/// per-call tax).
#[test]
fn app_auth_reuses_the_token_within_the_ttl() -> Result<(), BoxError> {
    let (forge, transport) = app_forge_with_script(vec![
        response(201, &mint_body("installation-token-1")),
        empty_pr_list(),
        empty_pr_list(),
    ]);
    let branch = halyard_forge::BranchName::new("halyard/release/main")
        .unwrap_or_else(|error| unreachable!("static branch is valid: {error}"));
    forge.find_release_pr(&branch)?;
    forge.find_release_pr(&branch)?;
    let posts = forge.call_count("POST");
    assert_eq!(posts, 1, "one mint total");
    let sent = transport.sent();
    assert_eq!(sent.len(), 3, "mint + two probes");
    let second = sent.get(2).ok_or("second probe recorded")?;
    assert_eq!(
        second.header("Authorization"),
        Some("Bearer installation-token-1"),
        "the cached installation token rides the re-run"
    );
    Ok(())
}

/// Past the TTL the token is re-minted (the clock is injected — the
/// test drives it, the adapter never reads the wall clock for auth
/// state).
#[test]
fn app_auth_re_mints_after_the_ttl() -> Result<(), BoxError> {
    let transport = MemoryTransport::new();
    transport.enqueue(response(201, &mint_body("installation-token-1")));
    transport.enqueue(empty_pr_list());
    transport.enqueue(response(201, &mint_body("installation-token-2")));
    transport.enqueue(empty_pr_list());
    let (clock, shared) = mutable_clock();
    let forge = GitHubRestForge::with_credentials_and_clock(
        endpoint(),
        ForgeCredentials::App(Box::new(app_credentials())),
        transport.clone(),
        clock,
    );
    let branch = halyard_forge::BranchName::new("halyard/release/main")
        .unwrap_or_else(|error| unreachable!("static branch is valid: {error}"));
    forge.find_release_pr(&branch)?;
    *lock(&shared) = T0 + halyard_forge_github::INSTALLATION_TOKEN_TTL_SECS;
    forge.find_release_pr(&branch)?;
    assert_eq!(forge.call_count("POST"), 2, "re-minted once past the TTL");
    let sent = transport.sent();
    let third = sent.get(3).ok_or("post-re-mint probe")?;
    assert_eq!(
        third.header("Authorization"),
        Some("Bearer installation-token-2"),
        "the fresh token rides the re-run"
    );
    Ok(())
}

/// A failed mint fails closed before any repo call: the taxonomy maps
/// the mint's statuses exactly like the research note pins them.
#[test]
fn app_auth_fails_closed_on_a_failed_mint() -> Result<(), BoxError> {
    let cases: Vec<(u16, &str, &'static str)> = vec![
        (
            401,
            "{\"message\":\"Bad credentials\"}",
            ForgeError::AUTH_CODE,
        ),
        (404, "{\"message\":\"Not Found\"}", ForgeError::AUTH_CODE),
        (
            500,
            "{\"message\":\"Server Error\"}",
            ForgeError::TRANSPORT_CODE,
        ),
    ];
    for (status, body, code) in cases {
        let (forge, transport) = app_forge_with_script(vec![response(status, body)]);
        let branch = halyard_forge::BranchName::new("halyard/release/main")
            .unwrap_or_else(|error| unreachable!("static branch is valid: {error}"));
        let error = forge
            .find_release_pr(&branch)
            .err()
            .ok_or_else(|| format!("status {status}: expected the mint to fail"))?;
        assert_eq!(error.code(), code, "status {status}: {error}");
        assert!(
            error.to_string().contains("installation"),
            "the reason names the mint: {error}"
        );
        assert_eq!(
            transport.sent().len(),
            1,
            "status {status}: no repo call rides a failed mint"
        );
    }
    Ok(())
}

/// A 201 mint whose body carries no token is an unusable exchange —
/// `E-FORGE-TRANSPORT`, never a fabricated credential.
#[test]
fn app_auth_fails_closed_on_a_tokenless_mint_body() -> Result<(), BoxError> {
    let (forge, _) = app_forge_with_script(vec![response(
        201,
        "{\"expires_at\":\"2030-01-01T00:00:00Z\"}",
    )]);
    let branch = halyard_forge::BranchName::new("halyard/release/main")
        .unwrap_or_else(|error| unreachable!("static branch is valid: {error}"));
    let error = forge
        .find_release_pr(&branch)
        .err()
        .ok_or("expected the tokenless body to fail")?;
    assert_eq!(error.code(), ForgeError::TRANSPORT_CODE, "{error}");
    Ok(())
}

/// The rate snapshot rides the mint response too (the mint is a call
/// like any other — §11.5 observability).
#[test]
fn app_auth_snapshots_rate_headers_from_the_mint() -> Result<(), BoxError> {
    let transport = MemoryTransport::new();
    transport.enqueue(HttpResponse {
        status: 201,
        headers: vec![
            ("x-ratelimit-limit".to_owned(), "15000".to_owned()),
            ("x-ratelimit-remaining".to_owned(), "14998".to_owned()),
        ],
        body: mint_body("installation-token-1"),
    });
    transport.enqueue(empty_pr_list());
    let forge = GitHubRestForge::with_credentials_and_clock(
        endpoint(),
        ForgeCredentials::App(Box::new(app_credentials())),
        transport.clone(),
        fixed_clock(),
    );
    let branch = halyard_forge::BranchName::new("halyard/release/main")
        .unwrap_or_else(|error| unreachable!("static branch is valid: {error}"));
    forge.find_release_pr(&branch)?;
    let rate = forge.rate_limit();
    assert_eq!(rate.limit, 15000);
    assert_eq!(rate.remaining, 14998);
    Ok(())
}

/// Token credentials keep the legacy constructor behavior: the static
/// bearer rides every call, no mint.
#[test]
fn token_credentials_still_ride_every_call() -> Result<(), BoxError> {
    let transport = MemoryTransport::new();
    transport.enqueue(empty_pr_list());
    let forge = GitHubRestForge::with_credentials_and_clock(
        endpoint(),
        ForgeCredentials::from_token("static-token"),
        transport.clone(),
        fixed_clock(),
    );
    let branch = halyard_forge::BranchName::new("halyard/release/main")
        .unwrap_or_else(|error| unreachable!("static branch is valid: {error}"));
    forge.find_release_pr(&branch)?;
    let sent = transport.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent.first()
            .ok_or("probe recorded")?
            .header("Authorization"),
        Some("Bearer static-token")
    );
    Ok(())
}

/// An anonymous adapter still fails closed before any call (the M4.1b
/// contract, preserved through the credentials refactor).
#[test]
fn anonymous_credentials_fail_closed_before_any_call() -> Result<(), BoxError> {
    let transport = MemoryTransport::new();
    let forge = GitHubRestForge::with_credentials_and_clock(
        endpoint(),
        ForgeCredentials::from_token(""),
        transport.clone(),
        fixed_clock(),
    );
    let branch = halyard_forge::BranchName::new("halyard/release/main")
        .unwrap_or_else(|error| unreachable!("static branch is valid: {error}"));
    let error = forge
        .find_release_pr(&branch)
        .err()
        .ok_or("expected the anonymous adapter to fail closed")?;
    assert_eq!(error.code(), ForgeError::AUTH_CODE, "{error}");
    assert!(transport.sent().is_empty(), "nothing reached the wire");
    Ok(())
}

/// The `with_app_transport` convenience constructor mirrors
/// `with_transport` (the CLI's two construction shapes).
#[test]
fn with_app_transport_builds_the_app_forge() -> Result<(), BoxError> {
    let transport = MemoryTransport::new();
    transport.enqueue(response(201, &mint_body("installation-token-1")));
    transport.enqueue(empty_pr_list());
    let forge =
        GitHubRestForge::with_app_transport(endpoint(), app_credentials(), transport.clone());
    let branch = halyard_forge::BranchName::new("halyard/release/main")
        .unwrap_or_else(|error| unreachable!("static branch is valid: {error}"));
    forge.find_release_pr(&branch)?;
    assert_eq!(forge.call_count("POST"), 1, "the mint happened");
    Ok(())
}
