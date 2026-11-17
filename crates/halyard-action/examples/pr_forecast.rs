//! The §12 `pr` mode's renderers (ADR-0043): a sample §11.1 machine
//! pass — the Release PR opened, one release minted by the tag phase —
//! mapped onto the plain [`PrPass`] view and rendered through the
//! preview writer. The exact §12 outputs a release-PR workflow step
//! receives (`pr_number` / `pr_url` carry the forge's own values) and
//! the job summary naming the pass's posture. Offline: the preview
//! records every call, nothing is written anywhere.
//!
//! Run: `cargo run -p halyard-action --example pr_forecast`

use std::collections::BTreeMap;
use std::path::Path;

use halyard_action::{
    ActionCall, ActionWriter, PrEffect, PrPass, PreviewActionWriter, env_file_entry, pr_outputs,
    pr_summary,
};
use halyard_core::{
    Bump, PLAN_SCHEMA_V1, Plan, PlanEdit, PlanPatch, PlanReason, PlanRelease, PlanSkip,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The two-package plan the release PR would carry (the same shape
    // the plan_forecast example renders — the pr mode's plan vocabulary
    // is identical, only the pass keys ride on top).
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

    // A prepare-phase pass: the forge opened Release PR #7 and minted
    // one release the plan's row names (the url fills by tag). The
    // shell maps the machine report onto this view — the forge's own
    // number and URL ride verbatim.
    let pass = PrPass {
        pr_effect: Some(PrEffect::Opened {
            number: 7,
            url: "https://github.com/example/monorepo/pull/7".to_owned(),
        }),
        propagated: false,
        released: true,
        release_urls: BTreeMap::from([(
            "core-v0.5.0".to_owned(),
            "https://github.com/example/monorepo/releases/tag/core-v0.5.0".to_owned(),
        )]),
        branch: "halyard/release/main".to_owned(),
        forecast: false,
    };

    // The pass view renders the §12 vocabulary over the preview writer:
    // every call is recorded, in order, exactly as the real writer
    // would perform it against the env files.
    let outputs = pr_outputs(&plan, Path::new("halyard-plan.json"), &pass)?;
    let preview = PreviewActionWriter::new();
    for (name, value) in &outputs {
        preview.set_output(name, value)?;
    }
    preview.append_summary(&pr_summary(&plan, &pass))?;

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
