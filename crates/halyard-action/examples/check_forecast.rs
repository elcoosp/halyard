//! The §12 `check` mode's renderer (ADR-0044): a sample pull-request
//! verdict — the squash-merge title accepted, the conveyed bumps per
//! pending package (the §10.2 pre-major remap teaching its own cause)
//! — rendered as the job summary beside the plan vocabulary. Offline:
//! the verdict is a pure function of the title, the config and the
//! plan (I5).
//!
//! Run: `cargo run -p halyard-action --example check_forecast`

use halyard_action::{
    ActionCall, ActionWriter, CheckRow, CheckVerdict, PreviewActionWriter, TitleOutcome,
    check_summary, env_file_entry, plan_outputs,
};
use halyard_core::{
    Bump, PLAN_SCHEMA_V1, Plan, PlanEdit, PlanPatch, PlanReason, PlanRelease, PlanSkip,
};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The two-package pending forecast (`core` on 0.4.1 — a 0.y.z
    // version, so the feat's minor remaps to patch; `app` on 1.0.0
    // rides the conveyed level as-is).
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
        edits: vec![PlanEdit {
            file: "crates/core/Cargo.toml".to_owned(),
            patches: vec![PlanPatch {
                start: 123,
                end: 128,
                text: "0.5.0".to_owned(),
            }],
        }],
        order: vec!["core".to_owned(), "app".to_owned()],
        skipped: vec![PlanSkip {
            package: "docs".to_owned(),
            reason: "no_changes".to_owned(),
        }],
    };

    // The verdict: the title accepted (the shell's lint), the conveyed
    // bumps predicted per pending package.
    let verdict = CheckVerdict {
        title: Some(TitleOutcome::Accepted {
            kind: "feat".to_owned(),
            level: "minor".to_owned(),
            predictions: vec![
                CheckRow {
                    package: "core".to_owned(),
                    from: "0.4.1".to_owned(),
                    bump: "patch".to_owned(),
                },
                CheckRow {
                    package: "app".to_owned(),
                    from: "1.0.0".to_owned(),
                    bump: "minor".to_owned(),
                },
            ],
        }),
    };

    // The outputs ride the plan vocabulary; the summary carries the
    // verdict. Rendered through the preview writer — the exact calls a
    // real workflow step would make.
    let outputs = plan_outputs(&plan, Path::new("halyard-plan.json"))?;
    let preview = PreviewActionWriter::new();
    for (name, value) in &outputs {
        preview.set_output(name, value)?;
    }
    preview.append_summary(&check_summary(&plan, &verdict))?;

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
