//! The GitLab REST adapter consumer demo (M7 step 1, ADR-0049): drives
//! the §11.1 machine's forge effects through [`GitLabRestForge`] over
//! the scripted [`MemoryTransport`] — every GitLab-shaped exchange
//! visible, fully offline. The live run swaps the transport for
//! [`UreqTransport`] and points the endpoint at a real forge (the
//! adapter rides the CLI's GitLab origin detection, ADR-0049).
//!
//! Run: `cargo run -p halyard-forge-gitlab --example forge_exchange`

use halyard_forge::transport::{HttpResponse, MemoryTransport};
use halyard_forge::{BranchName, Forge, MachineInput, PrSpec, ReleaseSpec, run_release_machine};
use halyard_forge_gitlab::{GitLabEndpoint, GitLabRestForge, UreqTransport};

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

/// One GitLab-shaped MR body.
fn mr(iid: u64) -> String {
    format!(
        r###"{{"iid":{iid},"title":"chore: release v0.5.0","description":"## 🚀 Release plan","state":"opened","source_branch":"halyard/release/main","target_branch":"main","web_url":"https://gitlab.com/group/halyard-demo/-/merge_requests/{iid}"}}"###
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = GitLabEndpoint::new("group/halyard-demo")?;
    let branch = BranchName::new("halyard/release/main")?;

    // The scripted forge: GitLab-shaped bodies exactly as the research
    // note pins them (docs/research/gitlab-rest-forge.md). Pass 1 = the
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
    enqueue(201, &mr(9));
    enqueue(200, &format!("[{}]", mr(9)));
    enqueue(200, &format!("[{}]", mr(9)));

    let forge = GitLabRestForge::with_transport(endpoint.clone(), "glpat-token", transport.clone());

    // Pass 1: the tag phase is empty; the prepare phase opens the MR
    // (find + upsert find + POST = the observable call shapes).
    let report = run_release_machine(&forge, &pr_input(&branch))?;
    println!("pass 1 (prepare): {:?}", report.effects);

    // Pass 2: the identical body is a no-op (I1) — the §11.5 budget's
    // find-only path (this adapter sends unconditional probes: research
    // §3 records the no-ETag difference from the GitHub adapter).
    let report = run_release_machine(&forge, &pr_input(&branch))?;
    println!(
        "pass 2 (no-op):   {:?} (calls: {})",
        report.effects, report.calls
    );

    // Pass 3: a pending release (the tag phase) — GET-by-tag-first then
    // the create; the prepare phase stays untouched (PROPAGATE).
    enqueue(404, r#"{"message":"404 Not Found"}"#);
    enqueue(
        201,
        r#"{"tag_name":"core-v0.5.0","_links":{"self":"https://gitlab.com/group/halyard-demo/-/releases/core-v0.5.0"}}"#,
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

    // The live construction (not executed): the same adapter over the
    // real transport, pointed at a self-hosted instance or gitlab.com.
    let _live = GitLabRestForge::with_transport(
        endpoint,
        &std::env::var("GITLAB_TOKEN").unwrap_or_default(),
        UreqTransport::new(),
    );
    Ok(())
}
