//! Plan-driven apply (M3.6, §9.1 + §7.3 edit subset): converts a
//! [`Plan`]'s edits into an [`EditSet`], routes each manifest through its
//! ecosystem adapter's semantic patcher (`edit_versions`, M3.1/M3.3) and
//! writes every edited file with the atomic protocol (temp sibling →
//! fsync → rename, §9.1). The M3.2 lockfile stage patches the root
//! `Cargo.lock` from the plan's version moves through the adapter's
//! `patch_lockfile` seam (§7.3 lockfile subset, §9.2): a decline
//! ([`halyard_core::lockfile::LockfileOutcome::NeedsNative`]) is reported,
//! never guessed.
//!
//! Order of operations (the §9.1 apply protocol): the whole plan converts
//! and validates first, then every file is patched **in memory** through
//! its adapter (which re-parses and verifies the read-back per M3.1's
//! acceptance) and only then are the files written — a failure anywhere
//! leaves the working tree untouched. The per-file summary is
//! deterministic: sorted paths, applied/skipped counts (invariant I3).

use std::collections::BTreeMap;
use std::path::Path;

use halyard_core::EcosystemAdapter;
use halyard_core::edit::EditFailure;
use halyard_core::lockfile::{LockfileFailure, LockfileOutcome, LockfileVersionChange};
use halyard_core::package::{RelPath, Workspace};
use halyard_core::plan::Plan;

use crate::EditSet;
use crate::atomic::atomic_write;
use crate::error::EditError;

/// Repo-relative path of the Cargo workspace lockfile (§9.2): always at
/// the repository root, never inside a member directory.
const CARGO_LOCKFILE: &str = "Cargo.lock";

/// Deterministic per-file result of one apply pass (M3.6 summary row).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSummary {
    /// Repo-relative manifest path.
    pub file: RelPath,
    /// Rows the adapter rewrote.
    pub applied: usize,
    /// Rows the adapter skipped (protocols, inherited rows, already
    /// applied).
    pub skipped: usize,
}

/// Deterministic result of the M3.2 lockfile stage for one apply pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockfileSummary {
    /// Repo-relative lockfile path (always `Cargo.lock`, §9.2).
    pub file: RelPath,
    /// Rows/entries the adapter rewrote (member version rows and
    /// `"name version"` entries, §9.2 rules 1–2).
    pub applied: usize,
    /// Rows/entries already at the target version (idempotent re-apply).
    pub skipped: usize,
    /// `Some(reason)` when the patcher declined
    /// ([`LockfileOutcome::NeedsNative`], §9.2 rule 4): the lockfile was
    /// left untouched and the orchestrator should run the native refresh
    /// effect step (`cargo update --workspace --offline`).
    pub native_reason: Option<String>,
}

/// Deterministic result of one apply pass over a whole plan.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApplyReport {
    /// Per-file summaries, sorted by path (invariant I3).
    pub files: Vec<FileSummary>,
    /// The lockfile stage result: `None` when the plan carries no cargo
    /// version moves or the repository has no root `Cargo.lock`.
    pub lockfile: Option<LockfileSummary>,
}

impl ApplyReport {
    /// Total rows rewritten across all files.
    pub fn total_applied(&self) -> usize {
        self.files.iter().map(|file| file.applied).sum()
    }

    /// Total rows skipped across all files.
    pub fn total_skipped(&self) -> usize {
        self.files.iter().map(|file| file.skipped).sum()
    }
}

/// Converts a plan's edits into the [`EditSet`] the apply machinery
/// writes (M3.6a). The plan is the contract: file paths are validated as
/// repo-relative paths and byte patches keep their ranges; overlaps and
/// duplicates fail the conversion.
///
/// # Errors
///
/// [`EditError::InvalidPatches`] when a plan path is not a valid
/// repo-relative path or the patch list is not ascending/non-overlapping.
pub fn edit_set_from_plan(plan: &Plan) -> Result<EditSet, EditError> {
    EditSet::from_plan_edits(&plan.edits)
}

/// Applies a computed plan to the working tree under `root` (M3.6b, the
/// local `apply` flow): discovery → plan (the caller's pipeline) →
/// [`EditSet`] → adapter patchers → atomic writes, plus the M3.2 lockfile
/// stage (the root `Cargo.lock` patched from the plan's version moves).
///
/// The adapter for each edited manifest resolves from the workspace's
/// packages (the manifest owner's ecosystem); unclaimed files (workspace
/// root manifests) fall back to the manifest file name. Every file is
/// patched in memory and verified by its adapter before anything is
/// written (§9.1: re-parse + semantic read-back, all or none). The
/// lockfile stage runs before any write as well: a hard lockfile failure
/// (`E-LOCK-PARSE`) aborts the whole apply, while a decline
/// ([`LockfileOutcome::NeedsNative`]) is reported in
/// [`ApplyReport::lockfile`] with the file untouched.
///
/// # Errors
///
/// [`EditError::InvalidPatches`] for stale patch spans
/// (`E-EDIT-PATCHES`) or a manifest no adapter covers,
/// [`EditError::Unrewritable`] for required-to-edit rows the rewriter
/// cannot admit (`E-EDIT-UNREWRITABLE`), [`EditError::VerifyFailed`]
/// when an edited manifest fails the read-back re-parse,
/// [`EditError::LockfileParse`] when the lockfile patch pass fails
/// outright (`E-LOCK-PARSE`), and [`EditError::Io`] for filesystem
/// failures.
pub fn apply_plan(
    root: &Path,
    plan: &Plan,
    workspace: &Workspace,
    adapters: &[&dyn EcosystemAdapter],
) -> Result<ApplyReport, EditError> {
    // Validate the whole plan up front (paths, overlaps) — nothing is
    // touched while this runs.
    edit_set_from_plan(plan)?;
    let mut staged: Vec<(RelPath, halyard_core::edit::EditOutcome)> = Vec::new();
    for edit in &plan.edits {
        let file = RelPath::new(&edit.file).ok_or(EditError::InvalidPatches {
            reason: format!(
                "plan edit path {:?} is not a valid repo-relative path",
                edit.file
            ),
        })?;
        let adapter =
            adapter_for(workspace, &file, adapters).ok_or_else(|| EditError::InvalidPatches {
                reason: format!("no registered adapter covers manifest {:?}", edit.file),
            })?;
        let original =
            std::fs::read(root.join(file.to_path_buf())).map_err(|source| EditError::Io {
                path: file.to_string(),
                source,
            })?;
        let outcome = adapter
            .edit_versions(&file, &original, &edit.patches)
            .map_err(|failure| map_failure(failure, &file))?;
        staged.push((file, outcome));
    }
    // M3.2 lockfile stage (§9.2): the plan's cargo version moves drive
    // the root Cargo.lock rewrite through the adapter's patch_lockfile
    // seam. Runs before any write, so a hard failure aborts cleanly.
    let staged_lockfile = stage_lockfile(root, plan, adapters)?;
    // Every manifest patched and verified in memory — write them all.
    let mut files = Vec::with_capacity(staged.len());
    for (file, outcome) in &staged {
        atomic_write(&root.join(file.to_path_buf()), &outcome.content)?;
        files.push(FileSummary {
            file: file.clone(),
            applied: outcome.applied,
            skipped: outcome.skipped,
        });
    }
    let mut lockfile = staged_lockfile.summary;
    if let Some((file, content, applied, skipped)) = staged_lockfile.written {
        atomic_write(&root.join(file.to_path_buf()), &content)?;
        if let Some(summary) = lockfile.as_mut() {
            summary.applied = applied;
            summary.skipped = skipped;
        }
    }
    Ok(ApplyReport { files, lockfile })
}

/// The M3.2 lockfile stage outcome: the summary row plus, when the
/// patcher produced new bytes, the (path, content, applied, skipped)
/// tuple to write after the manifests land.
struct StagedLockfile {
    summary: Option<LockfileSummary>,
    written: Option<(RelPath, Vec<u8>, usize, usize)>,
}

/// Patches the root `Cargo.lock` in memory from the plan's version moves
/// (§9.2). No cargo releases, no lockfile on disk, or an empty move set
/// → a no-op stage (`summary: None`).
fn stage_lockfile(
    root: &Path,
    plan: &Plan,
    adapters: &[&dyn EcosystemAdapter],
) -> Result<StagedLockfile, EditError> {
    let changes: Vec<LockfileVersionChange> = plan
        .releases
        .iter()
        .filter_map(LockfileVersionChange::from_release)
        .collect();
    if changes.is_empty() {
        return Ok(StagedLockfile {
            summary: None,
            written: None,
        });
    }
    let lock_path = root.join(CARGO_LOCKFILE);
    if !lock_path.exists() {
        // A cargo workspace without a committed lockfile has nothing to
        // patch (I5: halyard never invents or fetches one).
        return Ok(StagedLockfile {
            summary: None,
            written: None,
        });
    }
    let adapter = adapters
        .iter()
        .copied()
        .find(|adapter| adapter.id() == "cargo")
        .ok_or_else(|| EditError::InvalidPatches {
            reason: format!(
                "plan releases cargo packages but no cargo adapter is registered; cannot patch \
                 {CARGO_LOCKFILE}"
            ),
        })?;
    let file = RelPath::new(CARGO_LOCKFILE).ok_or(EditError::InvalidPatches {
        reason: format!("{CARGO_LOCKFILE} is not a valid repo-relative path"),
    })?;
    let original = std::fs::read(&lock_path).map_err(|source| EditError::Io {
        path: CARGO_LOCKFILE.to_owned(),
        source,
    })?;
    match adapter.patch_lockfile(&file, &original, &changes) {
        Ok(LockfileOutcome::Patched {
            content,
            applied,
            skipped,
        }) => Ok(StagedLockfile {
            summary: Some(LockfileSummary {
                file: file.clone(),
                applied,
                skipped,
                native_reason: None,
            }),
            written: Some((file, content, applied, skipped)),
        }),
        Ok(LockfileOutcome::NeedsNative { reason }) => Ok(StagedLockfile {
            // The decline is a report, not an error: the lockfile stays
            // untouched and the orchestrator refreshes natively (§9.2
            // rule 4).
            summary: Some(LockfileSummary {
                file,
                applied: 0,
                skipped: 0,
                native_reason: Some(reason),
            }),
            written: None,
        }),
        Err(failure) => Err(map_lockfile_failure(failure)),
    }
}

/// Maps the adapter's [`LockfileFailure`] onto the stable [`EditError`]
/// codes.
fn map_lockfile_failure(failure: LockfileFailure) -> EditError {
    match failure {
        LockfileFailure::Parse { message, .. } => EditError::LockfileParse { reason: message },
    }
}

/// Resolves the adapter owning `file`: the manifest's owning package
/// decides the ecosystem; unclaimed manifests (workspace roots) fall back
/// to the file-name convention (`Cargo.toml` → `cargo`, `package.json` →
/// `npm`).
fn adapter_for<'a>(
    workspace: &Workspace,
    file: &RelPath,
    adapters: &'a [&dyn EcosystemAdapter],
) -> Option<&'a dyn EcosystemAdapter> {
    let ecosystem = workspace
        .packages()
        .iter()
        .find(|package| &package.manifest == file)
        .map(|package| package.ecosystem.resolve(workspace.interner()).to_owned())
        .or_else(|| {
            match file.file_name() {
                "Cargo.toml" => Some("cargo"),
                "package.json" => Some("npm"),
                _ => None,
            }
            .map(str::to_owned)
        })?;
    adapters
        .iter()
        .copied()
        .find(|adapter| adapter.id() == ecosystem)
}

/// Maps the adapter's [`EditFailure`] onto the stable [`EditError`] codes.
fn map_failure(failure: EditFailure, file: &RelPath) -> EditError {
    match failure {
        EditFailure::SpanMismatch { start, end, .. } => EditError::InvalidPatches {
            reason: format!(
                "patch span {start}..{end} matches no editable row of {file} (the manifest \
                 changed since the plan was computed — re-plan)"
            ),
        },
        EditFailure::Unrewritable { detail, .. } => EditError::Unrewritable {
            message: format!("{file}: {detail}"),
        },
        EditFailure::Parse { message, .. } => EditError::VerifyFailed {
            reason: format!("{file}: {message}"),
        },
    }
}

/// The dry-run view (`apply --dry`): the files and patches the plan would
/// write, keyed by path in sorted order — no filesystem access.
pub fn dry_run_preview(plan: &Plan) -> Result<BTreeMap<String, Vec<DryPatch>>, EditError> {
    let edit_set = edit_set_from_plan(plan)?;
    let mut preview = BTreeMap::new();
    for file in edit_set.files() {
        let patches = edit_set
            .patches(file)
            .unwrap_or(&[])
            .iter()
            .map(|patch| DryPatch {
                start: patch.start(),
                end: patch.end(),
                text: patch.text().to_owned(),
            })
            .collect();
        preview.insert(file.to_string(), patches);
    }
    Ok(preview)
}

/// One patch in the dry-run preview (`apply --dry`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DryPatch {
    /// Inclusive start byte offset.
    pub start: u32,
    /// Exclusive end byte offset.
    pub end: u32,
    /// Replacement text.
    pub text: String,
}
