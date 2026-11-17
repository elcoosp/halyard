//! The §12 action-output seam (M10/§12 step 1, ADR-0037): a sample
//! plan rendered through the preview writer — the exact §12 outputs,
//! the env-file entries a `GITHUB_OUTPUT` file would receive and the
//! §12 job summary. Offline: the preview records every call, nothing
//! is written anywhere; inside a real workflow step (`GITHUB_OUTPUT`
//! set) the same calls land in the env files verbatim.
//!
//! Run: `cargo run -p halyard-action --example plan_forecast`

use std::path::Path;

use halyard_action::{
    ActionCall, ActionWriter, PreviewActionWriter, env_file_entry, plan_outputs, plan_summary,
};
use halyard_core::{
    Bump, PLAN_SCHEMA_V1, Plan, PlanEdit, PlanPatch, PlanReason, PlanRelease, PlanSkip,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The two-package shape (`core` minor cargo bump, `app` patch npm
    // bump) the CLI's `halyard action` renders from the real walk.
    let plan = Plan {
        schema: PLAN_SCHEMA_V1.to_owned(),
        base_branch: "main".to_owned(),
        head: "9f2ca67".to_owned(),
        plan_hash: "blake3:ab12".to_owned(),
        releases: vec![
            PlanRelease {
                package: "core".to_owned(),
                ecosystem: "cargo".to_owned(),
                from: "0.4.1".to_owned(),
                to: "0.5.0".to_owned(),
                bump: Bump::Minor,
                tag: "core-v0.5.0".to_owned(),
                reasons: vec![PlanReason::Commit {
                    oid: "ab12cd34".to_owned(),
                    kind: "feat".to_owned(),
                    breaking: false,
                }],
                changelog: None,
                publish: None,
            },
            PlanRelease {
                package: "app".to_owned(),
                ecosystem: "npm".to_owned(),
                from: "1.0.0".to_owned(),
                to: "1.1.0".to_owned(),
                bump: Bump::Patch,
                tag: "app-v1.1.0".to_owned(),
                reasons: vec![PlanReason::Commit {
                    oid: "ef34ab12".to_owned(),
                    kind: "fix".to_owned(),
                    breaking: false,
                }],
                changelog: None,
                publish: None,
            },
        ],
        edits: vec![
            PlanEdit {
                file: "crates/core/Cargo.toml".to_owned(),
                patches: vec![PlanPatch {
                    start: 123,
                    end: 128,
                    text: "0.5.0".to_owned(),
                }],
            },
            PlanEdit {
                file: "packages/app/package.json".to_owned(),
                patches: vec![PlanPatch {
                    start: 21,
                    end: 26,
                    text: "1.1.0".to_owned(),
                }],
            },
        ],
        order: vec!["core".to_owned(), "app".to_owned()],
        skipped: vec![PlanSkip {
            package: "docs".to_owned(),
            reason: "no_changes".to_owned(),
        }],
    };

    // The forecast renders the §12 vocabulary over the preview writer:
    // every call is recorded, in order, exactly as the real writer
    // would perform it against the env files.
    let outputs = plan_outputs(&plan, Path::new("halyard-plan.json"))?;
    let preview = PreviewActionWriter::new();
    for (name, value) in &outputs {
        preview.set_output(name, value)?;
    }
    preview.append_summary(&plan_summary(&plan))?;

    for call in preview.calls() {
        match call {
            ActionCall::SetOutput { name, value } => print!("{}", env_file_entry(&name, &value)?),
            ActionCall::AppendSummary { markdown } => {
                println!("# >> $GITHUB_STEP_SUMMARY");
                println!("{markdown}");
            }
            ActionCall::Warning { message } => println!("::warning::{message}"),
        }
    }
    Ok(())
}
