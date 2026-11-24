//! The §12 `auto` mode's renderers (ADR-0047): a sample composed §11.1
//! pass — the tag phase created both tags, the machine's own PROPAGATE
//! rule owned the run (pending releases gate the Release PR, §11.1
//! step 2), the machine minted one URL — mapped onto the plain
//! [`AutoPass`] view and rendered through the preview writer. The exact
//! §12 outputs an auto workflow step receives (`released` is the
//! git-side tag phase's honest execution value, no `pr_number`/`pr_url`
//! keys exist on the PROPAGATE pass — the Release PR lands on the next
//! pass) and the job summary naming the pass's posture. Offline: the
//! preview records every call, nothing is written anywhere.
//!
//! Run: `cargo run -p halyard-action --example auto_forecast`

use std::collections::BTreeMap;
use std::path::Path;

use halyard_action::{
    ActionCall, ActionWriter, AutoPass, ForgePosture, PreviewActionWriter, auto_outputs,
    auto_summary, env_file_entry,
};
use halyard_core::{
    Bump, PLAN_SCHEMA_V1, Plan, PlanEdit, PlanPatch, PlanReason, PlanRelease, PlanSkip,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The two-package plan the composed pass renders (the same shape the
    // plan_forecast example renders — the auto mode's plan vocabulary is
    // identical, only the pass keys ride on top).
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

    // A live PROPAGATE pass: both tags created (the tag phase's own
    // name-sorted row order, I3), the machine minted one URL and its
    // own PROPAGATE rule owned the run — pending releases gate the
    // Release PR (§11.1 step 2), so no PR effect rides the view. The
    // shell maps the phase and machine reports onto this view.
    let pass = AutoPass {
        released: true,
        propagated: true,
        forecast: false,
        created_tags: vec!["app-v1.1.0".to_owned(), "core-v0.5.0".to_owned()],
        pr_effect: None,
        release_urls: BTreeMap::from([(
            "core-v0.5.0".to_owned(),
            "https://github.com/example/monorepo/releases/tag/core-v0.5.0".to_owned(),
        )]),
        forge: ForgePosture::Ran {
            repo: "example/monorepo".to_owned(),
            releases: 1,
        },
    };

    // The pass view renders the §12 vocabulary over the preview writer:
    // every call is recorded, in order, exactly as the real writer
    // would perform it against the env files.
    let outputs = auto_outputs(&plan, Path::new("halyard-plan.json"), &pass)?;
    let preview = PreviewActionWriter::new();
    for (name, value) in &outputs {
        preview.set_output(name, value)?;
    }
    preview.append_summary(&auto_summary(&plan, &pass))?;

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
