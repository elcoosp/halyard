//! The §11.1 Release-PR state machine (M4.2a): the forge-side lifecycle
//! executor behind `halyard release` / `halyard pr`. One pass takes the
//! caller-reconciled [`MachineInput`] — the pending forge releases (tag
//! phase, §11.1 step 2) and the Release-PR spec derived from the plan
//! (prepare phase, §11.1 step 3) — and drives it against a
//! [`Forge`] adapter, returning a deterministic
//! [`MachineReport`].
//!
//! Division of labor (§11.1 as-built): the machine owns the **forge**
//! effects — idempotent release creation, Release-PR upsert/close — and
//! the shell owns the **git** effects (annotated tag creation,
//! `push --force-with-lease` with CAS) plus the manifest-vs-tags
//! reconciliation that decides what is "pending". The machine never
//! plans: the caller hands it the already-rendered [`PrSpec`]; planning
//! stays a pure pipeline above this seam (§4).
//!
//! Semantics pinned here:
//!
//! * **Tag phase first, PROPAGATE rule** — while pending releases are
//!   supplied, the prepare phase never runs (§11.1: "PROPAGATE to step 3
//!   only if nothing pending"). A merged Release PR or a partially
//!   failed run lands here; the re-run reports the existing releases
//!   (GET-by-tag-first) and still propagates.
//! * **I1 idempotence** — an identical Release-PR body is a no-op
//!   ([`MachineEffect::PrUnchanged`], zero mutating calls); a stale PR
//!   under an empty plan is closed exactly once; releases are
//!   GET-by-tag-first; a release branch that already carries the plan's
//!   content tree is reported ([`MachineEffect::ContentMatched`]) and
//!   never re-committed.
//! * **The content step** (§11.1 step 3, the M4.2 remainder) — the
//!   caller applies the plan's `EditSet` in memory, hashes the built tree
//!   and probes the remote release branch (`ls-remote`, 0 REST); the
//!   machine compares and, on a mismatch, commits through
//!   [`Forge::commit_via_api`] (§11.5 `commit_mode = "api"` — the
//!   forge-signed path) *before* the Release-PR upsert.
//! * **§11.5 call budgets** — the update path is find + upsert (2
//!   calls), the no-op paths spend the one find probe and nothing
//!   else. [`MachineReport::calls`] counts this pass's forge calls from
//!   the adapter's own log, so a REST adapter inherits the accounting.
//! * **§15.2 failure injection (I2)** — a failing adapter aborts the
//!   pass fail-closed before the next effect; the re-run completes to
//!   the identical end state with zero duplicates.
//!
//! The input is validated before the first call: an empty pending tag
//! is a caller bug (`E-FORGE-REQUEST`), never an effect.
//!
//! ```no_run
//! use halyard_forge::{BranchName, Forge, MachineInput, MemoryForge, PrSpec, ReleaseSpec, run_release_machine};
//!
//! let forge = MemoryForge::new();
//! let branch = BranchName::new("halyard/release/main").unwrap();
//! // Pass 1: pending releases gate the prepare phase (§11.1 step 2).
//! let report = run_release_machine(&forge, &MachineInput {
//!     branch: branch.clone(),
//!     pending_releases: vec![ReleaseSpec {
//!         tag: "core-v0.5.0".to_owned(),
//!         notes: "## Features".to_owned(),
//!     }],
//!     release_pr: Some(PrSpec {
//!         branch,
//!         title: "chore: release v0.5.0".to_owned(),
//!         body: "## 🚀 Release plan".to_owned(),
//!     }),
//!     content_commit: None,
//! }).unwrap();
//! assert!(report.propagated, "pending releases ⇒ prepare skipped");
//! ```

use crate::Forge;
use crate::error::ForgeError;
use crate::spec::{ApiCommit, BranchName, ContentCommit, PrSpec, ReleaseSpec};

/// One effect the machine performed (or found already performed), in
/// execution order inside [`MachineReport::effects`]. Deterministic data
/// only — the report is a value, comparable across runs (I3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MachineEffect {
    /// The forge release was created for `tag` (first pass, §11.1 step 2).
    ReleaseCreated {
        /// The released tag.
        tag: String,
        /// Forge URL of the release (synthetic `mem://` on the reference
        /// adapter).
        url: String,
    },
    /// The forge release already existed (GET-by-tag-first re-run, I1):
    /// nothing was created.
    ReleaseExisted {
        /// The released tag.
        tag: String,
    },
    /// The Release PR was opened (first prepare pass).
    PrOpened {
        /// The forge's PR number.
        number: u64,
        /// The forge's own PR URL — the report carries what the forge
        /// minted, never a composed guess (the §12 `pr_url` output
        /// renders it verbatim, ADR-0043).
        url: String,
    },
    /// The Release PR's body/title differed and was rewritten in place —
    /// same number, no duplicate (I1).
    PrUpdated {
        /// The forge's PR number.
        number: u64,
        /// The forge's own PR URL (unchanged by the rewrite).
        url: String,
    },
    /// The Release PR already carried exactly this title and body:
    /// no mutating call was made (the §11.5 no-op path).
    PrUnchanged {
        /// The forge's PR number.
        number: u64,
        /// The forge's own PR URL.
        url: String,
    },
    /// The stale Release PR was closed under an empty plan (§11.1 step 3);
    /// the re-run finds nothing and never double-closes.
    PrClosed {
        /// The forge's PR number.
        number: u64,
        /// The forge's own PR URL of the closed PR.
        url: String,
    },
    /// The release branch already carried the plan's content tree: the
    /// caller's probe matched the applied tree, so no commit was needed
    /// (§11.1 step 3 "if equal", I1 — zero mutating calls).
    ContentMatched {
        /// The applied content tree the probe matched.
        tree_oid: halyard_core::reason::Oid,
    },
    /// The plan's content was committed onto the release branch through
    /// the forge's API (§11.1 "else commit", §11.5 `commit_mode = "api"`
    /// — the forge signs the commit "Verified"). On the preview adapter
    /// the oid is the documented forecast digest.
    ContentCommitted {
        /// The commit the forge minted.
        oid: halyard_core::reason::Oid,
    },
}

/// The outcome of one [`run_release_machine`] pass: what the machine did,
/// in order, and what it cost (the §11.5 budget surface).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineReport {
    /// `true` when the tag phase ran and the prepare phase was skipped
    /// (§11.1 PROPAGATE: pending releases gate the Release PR).
    pub propagated: bool,
    /// The effects of this pass, oldest first. Empty on the no-op paths
    /// (nothing pending to create, no PR to open/update/close).
    pub effects: Vec<MachineEffect>,
    /// Forge calls this pass consumed, counted from the adapter's own
    /// call log ([`Forge::calls`]) — one entry per call, injected
    /// failures included (§11.5: a dropped call is an observable one).
    pub calls: usize,
}

/// The caller-reconciled input of one machine pass (§11.1 steps 2–3).
///
/// The caller owns everything the machine cannot see: the manifest-vs-tags
/// reconciliation that decides which releases are pending (the shell's
/// git/refs work, §11.1 step 2) and the plan rendering that produces the
/// [`PrSpec`] body (§11.3). The machine is the forge effect executor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineInput {
    /// The release branch the machine operates on (§11.2
    /// `halyard/release/<base>`; found by branch name, §11.4).
    pub branch: BranchName,
    /// Forge releases still owed, in order. Non-empty ⇒ the tag phase
    /// runs and the prepare phase is skipped (PROPAGATE, §11.1 step 2).
    /// Tags must be rendered from the package's `tag_template` before
    /// they reach this list — the machine refuses to invent tag names.
    pub pending_releases: Vec<ReleaseSpec>,
    /// The Release PR to upsert (`Some` when the plan has releases);
    /// `None` means the plan is empty — the machine closes a stale PR
    /// instead of upserting one (§11.1 step 3).
    pub release_pr: Option<PrSpec>,
    /// The prepare phase's content (§11.1 step 3): the caller-applied
    /// `EditSet` plus the caller's `ls-remote` probe of the release branch.
    /// `None` = the plan is empty (no content to commit) or the caller
    /// executes the git-side commit itself — the machine then performs
    /// no content step.
    pub content_commit: Option<ContentCommit>,
}

/// Runs one §11.1 machine pass against `forge`.
///
/// The pass is fail-closed: the input is validated before the first
/// call, and the first failing forge operation aborts the pass with the
/// adapter's own error (the §15.4 code passes through verbatim). The
/// re-run after a failure reaches the uninterrupted end state with zero
/// duplicates (I2) — every effect is idempotent by construction.
///
/// # Errors
///
/// [`ForgeError`] as produced by the adapter (`E-FORGE-*`), or
/// [`ForgeError::Request`] when [`MachineInput::pending_releases`]
/// carries an empty tag.
pub fn run_release_machine(
    forge: &dyn Forge,
    input: &MachineInput,
) -> Result<MachineReport, ForgeError> {
    // Fail-closed input validation: nothing is attempted with a caller
    // bug in the input (an empty tag would otherwise reach a real forge
    // call and be rejected there, after the cheaper effects ran). The
    // content commit's shared contract rides the same gate.
    for (index, spec) in input.pending_releases.iter().enumerate() {
        if spec.tag.is_empty() {
            return Err(ForgeError::request(format!(
                "pending release {index} has an empty tag; render the tag from the \
                 package's tag_template before handing releases to the machine"
            )));
        }
    }
    if let Some(content) = &input.content_commit {
        content.validate()?;
    }
    let calls_before = forge.calls().len();
    let mut effects = Vec::new();

    // TAG PHASE (§11.1 step 2) — always first. One create call per
    // pending release: the adapter's own GET-by-tag-first idempotence
    // (§11.1 "create forge releases (idempotent: GET-by-tag first)")
    // distinguishes created from already-existing, so the machine never
    // spends a separate probe.
    for spec in &input.pending_releases {
        let release = forge.create_release(spec)?;
        if release.created {
            effects.push(MachineEffect::ReleaseCreated {
                tag: release.tag.clone(),
                url: release.url.clone(),
            });
        } else {
            effects.push(MachineEffect::ReleaseExisted { tag: release.tag });
        }
    }
    if !input.pending_releases.is_empty() {
        // PROPAGATE (§11.1 step 2): pending forge releases mean the tag
        // phase owns this pass — the Release PR is not touched. The
        // caller finishes its git-side tag effects; the next pass sees
        // an empty pending list and reaches the prepare phase.
        return Ok(MachineReport {
            propagated: true,
            effects,
            calls: forge.calls().len() - calls_before,
        });
    }

    // PREPARE PHASE (§11.1 step 3) — only when nothing is pending. The
    // content step runs before the Release-PR upsert (§11.1 order: the
    // branch carries the content the PR previews). The caller probed the
    // remote branch (`ls-remote`, 0 REST) and hashed the applied tree;
    // a match means the branch already carries the plan's content — the
    // commit is skipped (I1, zero mutating calls) — and a mismatch (or
    // an absent branch) means the content rides `commit_via_api` (§11.5
    // `commit_mode = "api"`), whose ref update is the forge-side CAS
    // (§11.2): a racing writer surfaces as the adapter's conflict.
    if let Some(content) = &input.content_commit {
        if content.remote_tree_oid.as_ref() == Some(&content.tree_oid) {
            effects.push(MachineEffect::ContentMatched {
                tree_oid: content.tree_oid.clone(),
            });
        } else {
            let oid = forge.commit_via_api(&ApiCommit {
                branch: input.branch.clone(),
                message: content.message.clone(),
                files: content.files.clone(),
            })?;
            effects.push(MachineEffect::ContentCommitted { oid });
        }
    }

    let existing = forge.find_release_pr(&input.branch)?;
    match (&input.release_pr, existing) {
        // Plan non-empty: upsert, skipping the write when the forge
        // already carries exactly this title+body (I1: a no-op pass
        // makes zero mutating calls; the §11.5 budget says find only).
        (Some(spec), Some(pr)) if pr.title == spec.title && pr.body == spec.body => {
            effects.push(MachineEffect::PrUnchanged {
                number: pr.number,
                url: pr.url.clone(),
            });
        }
        (Some(spec), found) => {
            let pr = forge.upsert_pr(spec)?;
            effects.push(if found.is_some() {
                MachineEffect::PrUpdated {
                    number: pr.number,
                    url: pr.url.clone(),
                }
            } else {
                MachineEffect::PrOpened {
                    number: pr.number,
                    url: pr.url.clone(),
                }
            });
        }
        // Plan empty: close the stale Release PR if one exists (§11.1
        // step 3); the re-run finds none and double-closes nothing.
        (None, Some(pr)) => {
            let closed = forge.close_pr(&input.branch)?;
            effects.push(if closed {
                MachineEffect::PrClosed {
                    number: pr.number,
                    url: pr.url.clone(),
                }
            } else {
                // The PR vanished between the find and the close (a
                // racing actor closed it): the end state is already the
                // intended one, so the pass reports nothing (I1).
                MachineEffect::PrUnchanged {
                    number: pr.number,
                    url: pr.url.clone(),
                }
            });
        }
        // Plan empty and no stale PR: exit having spent the one find
        // probe — "0 API calls when no PR exists" (§11.1 step 3).
        (None, None) => {}
    }
    Ok(MachineReport {
        propagated: false,
        effects,
        calls: forge.calls().len() - calls_before,
    })
}
