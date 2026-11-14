//! The GitHub App credentials end to end, offline (M4.1c, ADR-0014):
//! the fixture PEM is parsed into an [`AppCredentials`], a JWT is
//! minted at a fixed epoch (determinism — the claims are a pure
//! function of the clock), and the adapter resolves a scripted
//! mint+probe exchange through [`MemoryTransport`]: the mint's bearer
//! is the JWT, the repo probe rides the minted installation token, and
//! the §11.5 call log shows both.
//!
//! Run: `cargo run -p halyard-forge-github --example app_exchange`

use halyard_forge::transport::{HttpResponse, MemoryTransport};
use halyard_forge::{BranchName, Forge};
use halyard_forge_github::{AppCredentials, ForgeCredentials, ForgeEndpoint, GitHubRestForge};

/// The test-only fixture key (the same PEM the contract tests ride;
/// never a production credential).
const PEM_PKCS1: &str = include_str!("../tests/fixtures/app_key_pkcs1.pem");

const FIXED_NOW: u64 = 1_700_000_000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Credentials: parse the fixture PEM (PKCS#1) into the validated
    //    bundle — the constructor is the fail-closed gate (`E-FORGE-AUTH`
    //    on any unusable input, before any call).
    let app = AppCredentials::from_pem("12345", PEM_PKCS1, 42)?;
    println!("app id           : {}", app.app_id());
    println!("installation id  : {}", app.installation_id());

    // 2. The JWT at a fixed epoch: the claims are deterministic (I3);
    //    only the RS256 signature varies (PKCS#1 blinding), so the
    //    claims are printed, the signature length reported.
    let jwt = app.mint_jwt(FIXED_NOW)?;
    let mut segments = jwt.split('.');
    let header = segments.next().unwrap_or_default();
    let payload = segments.next().unwrap_or_default();
    let signature = segments.next().unwrap_or_default();
    let decoded = String::from_utf8(decode_url(payload))?;
    println!("jwt header (b64) : {header}");
    println!("jwt claims       : {decoded}");
    println!(
        "jwt signature    : {} bytes (RS256)",
        decode_url(signature).len()
    );

    // 3. The adapter over App credentials and a fixed clock: the first
    //    call mints the installation token (one POST), the probe rides
    //    it, and the cached token amortizes the mint across the TTL.
    let transport = MemoryTransport::new();
    transport.enqueue(HttpResponse {
        status: 201,
        headers: Vec::new(),
        body: "{\"token\":\"installation-token-1\",\"expires_at\":\"2030-01-01T00:00:00Z\"}"
            .to_owned(),
    });
    transport.enqueue(HttpResponse {
        status: 200,
        headers: Vec::new(),
        body: "[]".to_owned(),
    });
    let endpoint = ForgeEndpoint::with_base("octo", "halyard-demo", "https://api.github.com")?;
    let forge = GitHubRestForge::with_credentials_and_clock(
        endpoint,
        ForgeCredentials::App(Box::new(app)),
        transport.clone(),
        std::sync::Arc::new(|| FIXED_NOW),
    );
    let branch = BranchName::new("halyard/release/main")?;
    let found = forge.find_release_pr(&branch)?;
    println!("probe result     : open Release PR = {found:?} (the §11.4 not-found value)");
    let sent = transport.sent();
    let mint = sent.first().ok_or("the mint exchange ran")?;
    println!("mint request     : {} {}", mint.method, mint.url);
    let authorization = mint.header("Authorization").unwrap_or_default();
    println!(
        "mint bearer      : {} … (the JWT rides Authorization: Bearer)",
        authorization.chars().take(30).collect::<String>()
    );
    let probe = sent.get(1).ok_or("the probe exchange ran")?;
    println!(
        "probe bearer     : {} (the minted installation token)",
        probe.header("Authorization").unwrap_or_default()
    );
    println!("§11.5 call log   :");
    for call in forge.calls() {
        println!("  {:<5} {}", call.method, call.target);
    }
    Ok(())
}

/// Decodes unpadded base64url (the JWT's encoding).
fn decode_url(segment: &str) -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(segment)
        .unwrap_or_default()
}
