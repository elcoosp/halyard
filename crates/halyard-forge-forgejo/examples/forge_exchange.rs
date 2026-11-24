//! The Forgejo REST adapter consumer demo (M7 step 2, ADR-0050): drives
//! the §11.1 machine's forge effects through [`ForgejoRestForge`] over
//! the scripted [`MemoryTransport`] — every Forgejo-shaped exchange
//! visible, fully offline. The live run swaps the transport for
//! [`UreqTransport`] and points the endpoint at a real forge (the
//! adapter rides the CLI's Forgejo/Gitea origin detection, ADR-0050).
//!
//! Run: `cargo run -p halyard-forge-forgejo --example forge_exchange`

use halyard_forge::transport::{HttpResponse, MemoryTransport};
use halyard_forge::{BranchName, Forge, MachineInput, PrSpec, ReleaseSpec, run_release_machine};
use halyard_forge_forgejo::{ForgejoEndpoint, ForgejoRestForge, UreqTransport};

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

/// One Forgejo-shaped pull body.
fn pull(number: u64) -> String {
    format!(
        r###"{{"number":{number},"title":"chore: release v0.5.0","body":"## 🚀 Release plan","state":"open","html_url":"https://codeberg.org/octo/halyard-demo/pulls/{number}","head":{{"ref":"halyard/release/main"}}}}"###
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = ForgejoEndpoint::new("octo/halyard-demo")?;
    let branch = BranchName::new("halyard/release/main")?;

    // The scripted forge: Forgejo-shaped bodies exactly as the research
    // note pins them (docs/research/forgejo-rest-forge.md). Pass 1 = the
    // machine's find probe (empty list); pass 2 = the upsert's own
    // lookup + the create POST; pass 3 = the I1 re-run (list unchanged,
    // identical body ⇒ no mutating call); pass 4 = the tag phase
    // (GET-by-tag 404, then the release POST).
    let transport = MemoryTransport::new();
    let enqueue = |status: u16, body: &str| {
        transport.enqueue(HttpResponse {
            status,
            headers: Vec::new(),
            body: body.to_owned(),
        });
    };
    enqueue(200, "[]");
    enqueue(200, "[]");
    enqueue(201, &pull(9));
    enqueue(200, &format!("[{}]", pull(9)));
    enqueue(200, &format!("[{}]", pull(9)));

    let forge =
        ForgejoRestForge::with_transport(endpoint.clone(), "forgejo-token", transport.clone());

    // Pass 1: the tag phase is empty; the prepare phase opens the pull
    // (find + upsert find + POST = the observable call shapes).
    let report = run_release_machine(&forge, &pr_input(&branch))?;
    println!("pass 1 (prepare): {:?}", report.effects);

    // Pass 2: the identical body is a no-op (I1) — the §11.5 budget's
    // find-only path (this adapter sends unconditional probes: research
    // §7 records the no-ETag difference from the GitHub adapter).
    let report = run_release_machine(&forge, &pr_input(&branch))?;
    println!(
        "pass 2 (no-op):   {:?} (calls: {})",
        report.effects, report.calls
    );

    // Pass 3: a pending release (the tag phase) — GET-by-tag-first then
    // the create; the prepare phase stays untouched (PROPAGATE).
    enqueue(404, r#"{"message":"Not Found"}"#);
    enqueue(
        201,
        r#"{"tag_name":"core-v0.5.0","html_url":"https://codeberg.org/octo/halyard-demo/releases/tag/core-v0.5.0","id":3}"#,
    );
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
    println!(
        "pass 3 (tag):     {:?} (calls: {})",
        report.effects, report.calls
    );

    // The §11.5 call log: every exchange recorded, failures included.
    println!("call log:");
    for record in forge.calls() {
        println!("  {} {}", record.method, record.target);
    }

    // The documented decline (research §5): the contents API has no
    // atomic multi-file commit, so the Forgejo lane rides the git half
    // (`commit_mode = "git"`) and the capability answers
    // `E-FORGE-UNSUPPORTED` naming the adapter.
    let declined = forge.commit_via_api(&halyard_forge::ApiCommit {
        branch: branch.clone(),
        message: "chore: release v0.5.0".to_owned(),
        files: vec![],
    });
    println!("commit_via_api:   {declined:?}");

    // The live construction (not executed): the same adapter over the
    // real transport, pointed at a self-hosted instance, gitea.com or
    // codeberg.org.
    let _live = ForgejoRestForge::with_transport(
        endpoint,
        &std::env::var("FORGEJO_TOKEN").unwrap_or_default(),
        UreqTransport::new(),
    );
    Ok(())
}
