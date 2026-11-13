//! The Forge seam consumer demo (§11.4, M4.1a): drives the §11.1
//! Release-PR lifecycle primitives against the deterministic
//! [`MemoryForge`] — the same sequence the M4.2 state machine runs
//! against a real forge adapter.
//!
//! Run: cargo run -p halyard-forge --example release_lifecycle

use halyard_forge::{ApiCommit, ApiFile, BranchName, Forge, MemoryForge, PrSpec, ReleaseSpec};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let forge = MemoryForge::new();
    let branch = BranchName::new("halyard/release/main")?;

    // Prepare phase (§11.1 step 3): find, then upsert — the update path
    // stays within the §11.5 two-call budget.
    let found = forge.find_release_pr(&branch)?;
    println!(
        "pass 1: existing Release PR on {branch}: {:?}",
        found.is_some()
    );
    let pr = forge.upsert_pr(&PrSpec {
        branch: branch.clone(),
        title: "chore: release v0.2.0".to_owned(),
        body: "## 🚀 Release plan\n\n| core | 0.1.0 | 0.2.0 | minor | `feat: …` |".to_owned(),
    })?;
    println!("pass 1: opened PR #{} at {}", pr.number, pr.url);

    // Idempotence (I1): the second pass re-finds the same PR and
    // rewrites its body in place — no duplicate artifact.
    let again = forge.upsert_pr(&PrSpec {
        branch: branch.clone(),
        title: pr.title.clone(),
        body: "## 🚀 Release plan\n\n| core | 0.1.1 | 0.2.0 | minor |".to_owned(),
    })?;
    println!(
        "pass 2: PR #{} rewritten in place (same number: {})",
        again.number,
        again.number == pr.number
    );

    // Tag phase (§11.1 step 2): GET-by-tag-first, so a rerun after a
    // partial failure reports the existing release instead of a duplicate.
    let release = forge.create_release(&ReleaseSpec {
        tag: "core-v0.2.0".to_owned(),
        notes: "## Features\n\n- the painter walk".to_owned(),
    })?;
    let rerun = forge.create_release(&ReleaseSpec {
        tag: "core-v0.2.0".to_owned(),
        notes: "## Features\n\n- the painter walk".to_owned(),
    })?;
    println!(
        "tag phase: release {} (created: {created}); rerun created: {created2}",
        release.tag,
        created = release.created,
        created2 = rerun.created
    );
    println!(
        "tag phase: `core-v0.2.0` released: {}",
        forge.release_exists("core-v0.2.0")?
    );

    // commit_mode = "api" (§11.5): the mock mints a synthetic, clearly
    // `fe`-prefixed object id; a real adapter routes through the forge's
    // Git Data API so the commit is forge-signed.
    let oid = forge.commit_via_api(&ApiCommit {
        branch: branch.clone(),
        message: "chore: apply the release plan".to_owned(),
        files: vec![ApiFile {
            path: "crates/halyard-core/Cargo.toml".to_owned(),
            content: b"version = \"0.2.0\"".to_vec(),
        }],
    })?;
    println!("api commit: {oid}");

    // Observability (§11.5): the call log is the end-of-run usage
    // summary's source — every forge operation, oldest first.
    println!("call log:");
    for record in forge.calls() {
        println!("  {:>16}  {}", record.method, record.target);
    }
    Ok(())
}
