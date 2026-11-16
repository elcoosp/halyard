//! The OIDC trusted-publishing exchange (M5.3 step 3, ADR-0031) over a
//! scripted transport — the documented two-call shape (the runner's
//! id-token request, then the crates.io token exchange) exercised
//! offline so no OIDC context is ever needed. Nothing real is printed:
//! the example prints the provenance spellings and scrub markers, the
//! seam's own names-only convention — a resolved token is never
//! echoed.
//!
//! Run: `cargo run -p halyard-publish --example publish_oidc`

use std::sync::Arc;

use halyard_forge::{HttpResponse, MemoryTransport};
use halyard_publish::{
    AuthOutcome, EnvPublishAuth, OidcContext, OidcPublishAuth, PublishAuth, PublishTarget,
    auth_summary,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The scripted registry side: the runner's id-token answer, then
    // crates.io's trusted-publishing exchange answer — the documented
    // body shapes (see docs/research/oidc-trusted-publishing.md).
    let transport = MemoryTransport::new();
    transport.enqueue(HttpResponse {
        status: 200,
        headers: Vec::new(),
        body: r#"{"value":"gha-id-token-for-the-example"}"#.to_owned(),
    });
    transport.enqueue(HttpResponse {
        status: 200,
        headers: Vec::new(),
        body: r#"{"token":"temporary-registry-token-for-the-example"}"#.to_owned(),
    });

    // The live provider: the documented env grammar first, the OIDC
    // exchange as the fallback (here with an empty base table — the
    // shell's `EnvPublishAuth::capture()` fills it offline-free).
    let auth = OidcPublishAuth::new(
        EnvPublishAuth::default(),
        Some(OidcContext {
            request_url: "https://actions.example.test/oidc?audience=".to_owned(),
            request_token: "request-token-for-the-example".to_owned(),
        }),
        Arc::new(transport),
        "https://crates.io".to_owned(),
        true,
    );

    let cargo = PublishTarget {
        package: "halyard-demo".to_owned(),
        version: "0.1.0".to_owned(),
        ecosystem: "cargo".to_owned(),
        registry: "crates.io".to_owned(),
        manifest_dir: "crates/demo".to_owned(),
        manifest: "crates/demo/Cargo.toml".to_owned(),
    };

    // The live resolution: the exchange fires once, the credential
    // rides the documented variable name — and the provider's Debug
    // carries no secret, as the scrub contract demands.
    let outcome = PublishAuth::credentials(&auth, &cargo)?;
    match outcome {
        AuthOutcome::Token(credential) => println!(
            "resolved {} via the exchange (value redacted: {credential})",
            credential.key
        ),
        AuthOutcome::Absent => println!("nothing injected (the tool's own store governs)"),
    }
    println!("provider debug (names only): {auth:?}");

    // The dry forecast: the same provider shape with the exchange
    // disabled projects the provenance without any network.
    let dry = OidcPublishAuth::new(
        EnvPublishAuth::default(),
        Some(OidcContext {
            request_url: "https://actions.example.test/oidc?audience=".to_owned(),
            request_token: "request-token-for-the-example".to_owned(),
        }),
        Arc::new(MemoryTransport::new()),
        "https://crates.io".to_owned(),
        false,
    );
    let summary = auth_summary(&dry, &[cargo])?;
    let projected = match summary.get("crates.io") {
        Some(provenance) => provenance.as_str(),
        None => "?",
    };
    println!("dry projection: crates.io → {projected}");
    Ok(())
}
