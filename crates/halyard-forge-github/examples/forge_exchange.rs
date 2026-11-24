//! The GitHub REST adapter consumer demo (M4.1b, ADR-0013): drives the
//! §11.1 machine's forge effects through [`GitHubRestForge`] over the
//! scripted [`MemoryTransport`] — every GitHub-shaped exchange visible,
//! fully offline. The live run swaps the transport for
//! [`UreqTransport`] and points the endpoint at a real forge (the
//! `forge-smoke` command is the CLI's live probe).
//!
//! Run: `cargo run -p halyard-forge-github --example forge_exchange`

use halyard_forge::transport::{HttpResponse, MemoryTransport};
use halyard_forge::{BranchName, Forge, MachineInput, PrSpec, ReleaseSpec, run_release_machine};
use halyard_forge_github::{ForgeEndpoint, GitHubRestForge, UreqTransport};

/// The prepare-phase input the first two passes share: no pending
/// releases, the same Release-PR spec (the identical body is the I1
/// no-op key).
fn pr_input(branch: &BranchName) -> MachineInput {
    MachineInput {
        branch: branch.clone(),
        pending_releases: Vec::new(),
        release_pr: Some(PrSpec {
            branch: branch.clone(),
            title: "chore: release v0.5.0".to_owned(),
            body: "## 🚀 Release plan".to_owned(),
        }),
        content_commit: None,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = ForgeEndpoint::new("octo", "halyard-demo")?;
    let branch = BranchName::new("halyard/release/main")?;

    // The scripted forge: GitHub-shaped bodies exactly as the research
    // note pins them (docs/research/github-rest-forge.md). Pass 1 = the
    // machine's find probe (empty list); pass 2 = the upsert's own
    // lookup + the create POST; pass 3 = the I1 re-run (list unchanged).
    let transport = MemoryTransport::new();
    transport.enqueue(HttpResponse {
        status: 200,
        headers: vec![("etag".to_owned(), "\"v1\"".to_owned())],
        body: "[]".to_owned(),
    });
    transport.enqueue(HttpResponse {
        status: 200,
        headers: vec![("etag".to_owned(), "\"v1\"".to_owned())],
        body: "[]".to_owned(),
    });
    transport.enqueue(HttpResponse {
        status: 201,
        headers: Vec::new(),
        body: r###"{"number":9,"title":"chore: release v0.5.0","body":"## 🚀 Release plan","html_url":"https://github.com/octo/halyard-demo/pull/9","head":{"ref":"halyard/release/main"}}"###.to_owned(),
    });
    transport.enqueue(HttpResponse {
        status: 200,
        headers: vec![("etag".to_owned(), "\"v2\"".to_owned())],
        body: r###"[{"number":9,"title":"chore: release v0.5.0","body":"## 🚀 Release plan","html_url":"https://github.com/octo/halyard-demo/pull/9","head":{"ref":"halyard/release/main"}}]"###.to_owned(),
    });

    let forge = GitHubRestForge::with_transport(endpoint.clone(), "token", transport.clone());

    // Pass 1: the tag phase is empty; the prepare phase opens the PR
    // (find + upsert find + POST = the observable call shapes).
    let report = run_release_machine(&forge, &pr_input(&branch))?;
    println!("pass 1 (prepare): {:?}", report.effects);

    // Pass 2: the identical body is a no-op (I1) — the conditional
    // re-find rides the cached ETag (a 304 under the primary limit).
    transport.enqueue(HttpResponse {
        status: 304,
        headers: Vec::new(),
        body: String::new(),
    });
    let report = run_release_machine(&forge, &pr_input(&branch))?;
    println!(
        "pass 2 (no-op):   {:?} (calls: {})",
        report.effects, report.calls
    );

    // Pass 3: a pending release (the tag phase) — GET-by-tag-first
    // creation with the notes payload.
    transport.enqueue(HttpResponse {
        status: 404,
        headers: Vec::new(),
        body: "{\"message\":\"Not Found\"}".to_owned(),
    });
    transport.enqueue(HttpResponse {
        status: 201,
        headers: Vec::new(),
        body: r#"{"id":1,"tag_name":"core-v0.5.0","html_url":"https://github.com/octo/halyard-demo/releases/tag/core-v0.5.0"}"#.to_owned(),
    });
    let report = run_release_machine(
        &forge,
        &MachineInput {
            branch: branch.clone(),
            pending_releases: vec![ReleaseSpec {
                tag: "core-v0.5.0".to_owned(),
                notes: "## Features\n\n- the painter walk".to_owned(),
            }],
            release_pr: None,
            content_commit: None,
        },
    )?;
    println!("pass 3 (tag phase): {:?}", report.effects);

    // The §11.5 surfaces: the rate snapshot (from response headers) and
    // the call log (one record per HTTP call).
    let rate = forge.rate_limit();
    println!(
        "rate snapshot: {}/{} remaining/limit",
        rate.remaining, rate.limit
    );
    println!("call log:");
    for record in forge.calls() {
        println!("  {:>5} {}", record.method, record.target);
    }

    // The real client swaps in one line (offline here — the loopback
    // test drives it for real; the wire-up builds exactly this shape).
    let live = GitHubRestForge::with_transport(
        ForgeEndpoint::new("octo", "halyard-demo")?,
        std::env::var("GITHUB_TOKEN").unwrap_or_default().as_str(),
        UreqTransport::new(),
    );
    println!(
        "live adapter built (token set: {})",
        !std::env::var("GITHUB_TOKEN").unwrap_or_default().is_empty()
    );
    let _ = live;
    Ok(())
}
