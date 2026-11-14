//! The App-JWT auth path baseline (M4.1c, ADR-0014): the JWT mint
//! (canonical claims + RS256 signature over the test fixture key) and
//! the full scripted mint+probe pass — the §11.5 budget shape the App
//! credentials add (one POST per [`INSTALLATION_TOKEN_TTL_SECS`],
//! amortized across the run's calls). Crypto cost is the point here:
//! the mint is the adapter's own CPU overhead, never a benchmark of
//! the network.
//!
//! Run: `cargo bench -p halyard-forge-github`

use criterion::Criterion;
use halyard_forge::transport::{HttpResponse, MemoryTransport};
use halyard_forge::{BranchName, Forge};
use halyard_forge_github::{AppCredentials, ForgeCredentials, ForgeEndpoint, GitHubRestForge};

/// The test-only fixture key (the same PEM the contract tests ride;
/// never a production credential).
const PEM_PKCS1: &str = include_str!("../tests/fixtures/app_key_pkcs1.pem");

fn app_credentials() -> AppCredentials {
    AppCredentials::from_pem("12345", PEM_PKCS1, 42)
        .unwrap_or_else(|e| unreachable!("the fixture key is a valid RSA key: {e}"))
}

fn endpoint() -> ForgeEndpoint {
    ForgeEndpoint::with_base("octo", "halyard-demo", "https://api.github.com")
        .unwrap_or_else(|e| unreachable!("valid endpoint: {e}"))
}

fn branch() -> BranchName {
    BranchName::new("halyard/release/main").unwrap_or_else(|e| unreachable!("valid branch: {e}"))
}

fn mint_response() -> HttpResponse {
    HttpResponse {
        status: 201,
        headers: Vec::new(),
        body: "{\"token\":\"installation-token-1\",\"expires_at\":\"2030-01-01T00:00:00Z\"}"
            .to_owned(),
    }
}

fn ok(body: &str) -> HttpResponse {
    HttpResponse {
        status: 200,
        headers: Vec::new(),
        body: body.to_owned(),
    }
}

/// The JWT mint alone: canonical claims + base64url + one RS256
/// signature (the per-mint CPU floor the TTL amortizes).
fn bench_jwt_mint(c: &mut Criterion) {
    let app = app_credentials();
    c.bench_function("app_auth/mint_jwt/2048-bit", |b| {
        b.iter(|| {
            app.mint_jwt(1_700_000_000)
                .unwrap_or_else(|e| unreachable!("signing succeeds on the fixture key: {e}"));
        });
    });
}

/// The full App-authenticated probe pass over the scripted transport:
/// mint (JWT + POST + token parse) + the repo probe with the minted
/// token — the first-call shape of every App-authenticated run.
fn bench_app_probe_pass(c: &mut Criterion) {
    c.bench_function("app_auth/mint_plus_find_probe", |b| {
        b.iter_batched(
            || {
                let transport = MemoryTransport::new();
                transport.enqueue(mint_response());
                transport.enqueue(ok("[]"));
                let forge = GitHubRestForge::with_credentials_and_clock(
                    endpoint(),
                    ForgeCredentials::App(Box::new(app_credentials())),
                    transport.clone(),
                    std::sync::Arc::new(|| 1_700_000_000),
                );
                (forge, transport)
            },
            |(forge, _transport)| {
                forge
                    .find_release_pr(&branch())
                    .unwrap_or_else(|e| unreachable!("scripted pass succeeds: {e}"));
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

/// The TTL amortization shape: the re-run inside the TTL spends no
/// mint — one probe (the I1 no-op path's App-auth cost).
fn bench_app_cached_probe(c: &mut Criterion) {
    c.bench_function("app_auth/cached_token_find_probe", |b| {
        b.iter_batched(
            || {
                let transport = MemoryTransport::new();
                transport.enqueue(mint_response());
                transport.enqueue(ok("[]"));
                transport.enqueue(ok("[]"));
                let forge = GitHubRestForge::with_credentials_and_clock(
                    endpoint(),
                    ForgeCredentials::App(Box::new(app_credentials())),
                    transport.clone(),
                    std::sync::Arc::new(|| 1_700_000_000),
                );
                let branch = branch();
                forge
                    .find_release_pr(&branch)
                    .unwrap_or_else(|e| unreachable!("scripted warm-up succeeds: {e}"));
                (forge, transport)
            },
            |(forge, _transport)| {
                forge
                    .find_release_pr(&branch())
                    .unwrap_or_else(|e| unreachable!("scripted pass succeeds: {e}"));
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn main() {
    let mut criterion = Criterion::default();
    bench_jwt_mint(&mut criterion);
    bench_app_probe_pass(&mut criterion);
    bench_app_cached_probe(&mut criterion);
    criterion.final_summary();
}
