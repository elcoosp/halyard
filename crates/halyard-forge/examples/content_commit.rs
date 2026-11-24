//! The §11.1 step-3 content commit (the M4.2 remainder): the machine's
//! prepare-phase content step driven over the deterministic
//! [`MemoryForge`]. The caller applies the plan's `EditSet` in memory and
//! hashes the content tree (the shell's `pr` executor does this through
//! `halyard-edit::stage_plan_edits` + `halyard-git::content_tree_oid`);
//! the machine compares against the caller-probed remote tree and
//! commits through [`Forge::commit_via_api`] when they differ — the
//! §11.5 `commit_mode = "api"` path, whose ref update is the forge-side
//! CAS (§11.2). The re-run with the landed tree matched skips the commit
//! (I1): the idempotence key is the tree, not the commit.
//!
//! Run: `cargo run -p halyard-forge --example content_commit`

use halyard_core::reason::Oid;
use halyard_forge::{
    ApiFile, BranchName, ContentCommit, MachineEffect, MachineInput, MemoryForge, PrSpec,
    run_release_machine,
};

/// A 40-hex OID fixture (deterministic; at this seam the OID is opaque
/// data — only equality matters to the machine's compare).
fn tree_oid(seed: u8) -> Oid {
    let digits: String = std::iter::repeat_n(format!("{seed:02x}"), 20).collect();
    Oid::from_hex(digits.as_bytes()).unwrap_or_else(|| unreachable!("hex fixture is 40 digits"))
}

/// The applied `EditSet` the caller assembled: the version row of one
/// manifest plus the changelog the plan renders.
fn applied_files() -> Vec<ApiFile> {
    vec![
        ApiFile {
            path: "crates/core/Cargo.toml".to_owned(),
            content: b"[package]\nname = \"core\"\nversion = \"0.5.0\"\n".to_vec(),
        },
        ApiFile {
            path: "crates/core/CHANGELOG.md".to_owned(),
            content: b"# Changelog\n\n## 0.5.0\n\n### Features\n\n- the painter walk\n".to_vec(),
        },
    ]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let branch = BranchName::new("halyard/release/main")?;
    let forge = MemoryForge::new();

    // Pass 1 (divergent): the branch does not exist remotely yet (the
    // probe is None) — the machine commits the applied payloads through
    // `commit_via_api`, then opens the Release PR (§11.1 order).
    let first = MachineInput {
        branch: branch.clone(),
        pending_releases: Vec::new(),
        release_pr: Some(PrSpec {
            branch: branch.clone(),
            title: "chore: release v0.5.0".to_owned(),
            body: "## 🚀 Release plan\n\n| Package | Current | Next | Bump | Why |\n|---|---|---|---|---|\n| `core` | 0.4.1 | **0.5.0** | minor | `feat(parser): …` (#412) |".to_owned(),
        }),
        content_commit: Some(ContentCommit {
            message: "chore: release v0.5.0".to_owned(),
            files: applied_files(),
            tree_oid: tree_oid(0xaa),
            remote_tree_oid: None,
        }),
    };
    let report = run_release_machine(&forge, &first)?;
    println!("pass 1 (divergent):");
    for effect in &report.effects {
        match effect {
            MachineEffect::ContentCommitted { oid } => {
                println!("  content committed onto {branch} ({oid})");
            }
            MachineEffect::PrOpened { number, url } => {
                println!("  Release PR opened: #{number} at {url}");
            }
            other => println!("  {other:?}"),
        }
    }
    println!("  forge calls: {}", report.calls);

    // Pass 2 (matched): the branch tip's tree now equals the applied
    // tree (the caller re-probed) — the commit is skipped, the PR body
    // is already current: zero mutating calls (I1).
    let second = MachineInput {
        branch: branch.clone(),
        pending_releases: Vec::new(),
        release_pr: first.release_pr.clone(),
        content_commit: Some(ContentCommit {
            message: "chore: release v0.5.0".to_owned(),
            files: applied_files(),
            tree_oid: tree_oid(0xaa),
            remote_tree_oid: Some(tree_oid(0xaa)),
        }),
    };
    let rerun = run_release_machine(&forge, &second)?;
    println!("pass 2 (matched re-run):");
    for effect in &rerun.effects {
        match effect {
            MachineEffect::ContentMatched { tree_oid } => {
                println!("  branch already carries the content tree {tree_oid}");
            }
            MachineEffect::PrUnchanged { number, url } => {
                println!("  Release PR #{number} already current ({url})");
            }
            other => println!("  {other:?}"),
        }
    }
    println!(
        "  forge calls: {} (find + body compare, no commit)",
        rerun.calls
    );
    assert_eq!(
        forge.commits().len(),
        1,
        "the re-run never duplicates the content commit (I1)"
    );
    Ok(())
}
