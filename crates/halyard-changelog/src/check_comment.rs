//! The §11.6 check-comment renderer (M4.5 step 1): the stable, parseable
//! PR comment the `check --pr-title --comment` mode posts to the PR
//! under review — §11.6's "optional PR comment showing *if merged, this
//! will release: core minor*", rendered for both verdict shapes (an
//! accepted title predicts the bumps; a rejected one carries the lint's
//! diagnosis, because the comment is the check that teaches).
//!
//! The body is deterministic (I3): a given verdict renders byte-for-byte
//! identically on every run, so the forge seam's marker-based comment
//! upsert (`Forge::upsert_pr_comment` in `halyard-forge`) rewrites the
//! same comment in place on every re-run instead of piling up
//! duplicates. The marker is the upsert's find key — the renderer embeds
//! it as the body's first line, and the CLI passes it through to the
//! seam verbatim. (The forge dependency stays on the consumer side: this
//! crate renders markdown only, §7.2.)
//!
//! Layout:
//!
//! ```markdown
//! <!-- halyard:check v1 -->
//!
//! ## 🧪 halyard check
//!
//! If merged, `feat(parser): accept spans` — this will release:
//!
//! | Package | Current | Bump |
//! |---|---|---|
//! | `p1` | 0.4.1 | patch (0.x policy: remapped for pre-major) |
//! | `p2` | 1.0.3 | minor |
//! ```
//!
//! The remap note renders only when the §10.2 pre-major policy changed
//! the conveyed bump for the package — the prediction teaches its own
//! cause, exactly like the CLI's human renderer does.

use std::fmt::Write as _;

use halyard_core::plan::Bump;

/// The comment marker (the forge upsert's find key): the stable HTML
/// comment the renderer writes as the body's first line. Version `v1`
/// lets the body layout evolve without re-finding old comments' shape.
pub const CHECK_MARKER: &str = "<!-- halyard:check v1 -->";

/// One pending package's predicted bump under the squash-merged title
/// (the CLI's prediction row, carried across the crate boundary as
/// plain data — the renderer stays pure, §4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckPrediction {
    /// The package name.
    pub package: String,
    /// The version in the manifest (`PlanRelease::from`).
    pub from: String,
    /// The bump the title conveys for this package (after the §10.2
    /// pre-major remap where the package is `0.y.z`).
    pub bump: Bump,
    /// Whether the §10.2 pre-major policy changed the conveyed bump.
    pub remapped: bool,
}

/// The title verdict the comment carries (§11.6: the prediction for an
/// accepted title, the diagnosis for a rejected one).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckVerdict {
    /// The title passed the lint (`kind` as written, `breaking` for the
    /// `!`/`BREAKING CHANGE` titles the planner maps to major).
    Accepted {
        /// The conventional-commit type as written.
        kind: String,
        /// Whether the title is breaking (`!` or a `BREAKING CHANGE`
        /// footer grammar).
        breaking: bool,
    },
    /// The title failed the lint — `reason` is the diagnosis text
    /// (`E-CLI-TITLE`'s message: the grammar refusal, the fixup skip or
    /// the did-you-mean), surfaced verbatim so the comment teaches the
    /// same fix the job log does.
    Rejected {
        /// The diagnosis text.
        reason: String,
    },
}

/// The comment's inputs: the linted title verbatim, the verdict and the
/// per-package predictions (empty when the verdict is rejected or
/// nothing is pending).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckComment {
    /// The linted squash-merge title (rendered verbatim).
    pub title: String,
    /// The lint's verdict.
    pub verdict: CheckVerdict,
    /// One row per pending release, in plan order.
    pub predictions: Vec<CheckPrediction>,
}

/// Renders the §11.6 comment body for `input`. Total over inputs: every
/// verdict shape renders a complete comment (marker first, heading,
/// verdict body) — the upsert never posts a half-formed comment.
#[must_use]
pub fn render_check_comment(input: &CheckComment) -> String {
    let mut body = String::with_capacity(512);
    body.push_str(CHECK_MARKER);
    body.push_str("\n\n## 🧪 halyard check\n\n");
    match &input.verdict {
        CheckVerdict::Accepted { breaking, .. } => {
            let breaking_note = if *breaking { " (breaking)" } else { "" };
            if input.predictions.is_empty() {
                let _ = writeln!(
                    body,
                    "No pending releases — merging `{}`{breaking_note} releases nothing.",
                    input.title
                );
            } else {
                let _ = writeln!(
                    body,
                    "If merged, `{}`{breaking_note} — this will release:",
                    input.title
                );
                body.push_str("| Package | Current | Bump |\n|---|---|---|\n");
                for prediction in &input.predictions {
                    let remap_note = if prediction.remapped {
                        " (0.x policy: remapped for pre-major)"
                    } else {
                        ""
                    };
                    let _ = writeln!(
                        body,
                        "| `{}` | {} | {}{} |",
                        prediction.package,
                        prediction.from,
                        bump_word(prediction.bump),
                        remap_note
                    );
                }
            }
        }
        CheckVerdict::Rejected { reason } => {
            let _ = writeln!(body, "The merge title was **rejected**: {reason}");
        }
    }
    body
}

/// The lowercase bump word the table renders (`Bump` serializes
/// lowercase on the wire; the table matches it verbatim — the same
/// rendering the §11.3 Release-PR body uses).
fn bump_word(bump: Bump) -> &'static str {
    match bump {
        Bump::None => "none",
        Bump::Patch => "patch",
        Bump::Minor => "minor",
        Bump::Major => "major",
    }
}
