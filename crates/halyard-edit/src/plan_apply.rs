//! Plan-driven apply (M3.6, §9.1 + §7.3 edit subset): converts a
//! [`Plan`]'s edits into an [`EditSet`], routes each manifest through its
//! ecosystem adapter's semantic patcher (`edit_versions`, M3.1/M3.3) and
//! writes every edited file with the atomic protocol (temp sibling →
//! fsync → rename, §9.1). The M3.2/M3.4 lockfile stages patch the
//! workspace's lockfiles from the plan's version moves through each
//! adapter's `patch_lockfile` seam (§9.2 Cargo.lock, §9.3
//! `pnpm-lock.yaml`): a decline
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

/// The lockfile targets of the apply flow, as `(repo-relative path,
/// ecosystem)` pairs: the root `Cargo.lock` (§9.2, M3.2), the root
/// `pnpm-lock.yaml` (§9.3, M3.4) and the root `uv.lock` (the M8 PyPI
/// cell's lockfile half, ADR-0056). All live at the repository root,
/// never inside a member directory. Other npm-family lockfiles
/// (`package-lock.json`, `yarn.lock`, `bun.lock`) are native-only in v1
/// and get no in-memory stage; the pypi stage rides the same native-only
/// shape (uv's documented management posture — the adapter declines and
/// the §9.2 rule-4 refresh owns the file).
const LOCKFILE_TARGETS: [(&str, &str); 3] = [
    ("Cargo.lock", "cargo"),
    ("pnpm-lock.yaml", "npm"),
    ("uv.lock", "pypi"),
];

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

/// Deterministic result of one lockfile stage for one apply pass (M3.2
/// Cargo.lock / M3.4 pnpm-lock.yaml).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockfileSummary {
    /// Repo-relative lockfile path (`Cargo.lock`, `pnpm-lock.yaml`).
    pub file: RelPath,
    /// The ecosystem whose adapter ran the stage (`cargo`, `npm`).
    pub ecosystem: String,
    /// Rows/entries the adapter rewrote (§9.2 member version rows and
    /// `"name version"` entries; §9.3 importer specifier/link-ref rows).
    pub applied: usize,
    /// Rows/entries already at the target version or needing no change
    /// (idempotent re-apply, protocol rows).
    pub skipped: usize,
    /// `Some(reason)` when the patcher declined
    /// ([`LockfileOutcome::NeedsNative`], §9.2 rule 4 / §9.3): the
    /// lockfile was left untouched and the orchestrator should run the
    /// ecosystem's native refresh effect step.
    pub native_reason: Option<String>,
}

/// Deterministic result of one apply pass over a whole plan.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApplyReport {
    /// Per-file summaries, sorted by path (invariant I3).
    pub files: Vec<FileSummary>,
    /// The lockfile stage results, in `LOCKFILE_TARGETS` order: one row
    /// per staged lockfile (a plan with no version moves for an
    /// ecosystem, or without the lockfile on disk, contributes none).
    pub lockfiles: Vec<LockfileSummary>,
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

/// The staged half of the apply flow (§11.1 step 3 "apply `EditSet` in
/// memory"): every manifest and lockfile the plan edits, patched and
/// read-back-verified through the adapters — **without any write**. The
/// content-commit flow consumes [`StagedEdits::payloads`]; the local
/// apply flow writes them ([`apply_plan`] is exactly stage + write, so
/// the two flows can never disagree about what a plan edits).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedEdits {
    /// Every staged file (manifests in plan order, then the patched
    /// lockfiles), as `(repo-relative path, content)` pairs — the
    /// content commit's payloads (§11.1 step 3).
    pub payloads: Vec<(RelPath, Vec<u8>)>,
    /// Per-manifest summary rows, plan order (the apply report's
    /// `files`).
    pub files: Vec<FileSummary>,
    /// Per-lockfile summary rows, `LOCKFILE_TARGETS` order (the apply
    /// report's `lockfiles`).
    pub lockfiles: Vec<LockfileSummary>,
}

/// Stages a plan's edits in memory (the §11.1 step-3 content commit's
/// input): each edited manifest routes through its ecosystem adapter's
/// semantic patcher (`edit_versions`, M3.1/M3.3) and each staged
/// lockfile through `patch_lockfile` (§9.2/§9.3) — every file patched
/// and verified before the caller sees anything, nothing written.
///
/// The adapter for each edited manifest resolves from the workspace's
/// packages (the manifest owner's ecosystem); unclaimed files (workspace
/// root manifests) fall back to the manifest file name — the same rule
/// [`apply_plan`] applies, because this is its first half.
///
/// # Errors
///
/// [`EditError::InvalidPatches`] for stale patch spans
/// (`E-EDIT-PATCHES`) or a manifest no adapter covers,
/// [`EditError::Unrewritable`] for required-to-edit rows the rewriter
/// cannot admit (`E-EDIT-UNREWRITABLE`), [`EditError::VerifyFailed`]
/// when an edited manifest fails the read-back re-parse,
/// [`EditError::LockfileParse`] when a lockfile patch pass fails
/// outright (`E-LOCK-PARSE`), and [`EditError::Io`] for filesystem
/// read failures.
pub fn stage_plan_edits(
    root: &Path,
    plan: &Plan,
    workspace: &Workspace,
    adapters: &[&dyn EcosystemAdapter],
) -> Result<StagedEdits, EditError> {
    // Validate the whole plan up front (paths, overlaps) — nothing is
    // staged while this runs.
    edit_set_from_plan(plan)?;
    let mut payloads: Vec<(RelPath, Vec<u8>)> = Vec::new();
    let mut files = Vec::with_capacity(plan.edits.len());
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
        payloads.push((file.clone(), outcome.content.clone()));
        files.push(FileSummary {
            file,
            applied: outcome.applied,
            skipped: outcome.skipped,
        });
    }
    // M3.2/M3.4 lockfile stages (§9.2/§9.3): the plan's per-ecosystem
    // version moves drive the workspace lockfile rewrites through each
    // adapter's patch_lockfile seam. Runs as part of the staging, so a
    // hard failure aborts before the caller sees any payload.
    let staged_lockfiles = stage_lockfiles(root, plan, adapters)?;
    let mut lockfiles = staged_lockfiles.summaries;
    for (file, content, applied, skipped) in staged_lockfiles.written {
        payloads.push((file.clone(), content));
        if let Some(summary) = lockfiles.iter_mut().find(|summary| summary.file == file) {
            summary.applied = applied;
            summary.skipped = skipped;
        }
    }
    Ok(StagedEdits {
        payloads,
        files,
        lockfiles,
    })
}

/// Applies a computed plan to the working tree under `root` (M3.6b, the
/// local `apply` flow): discovery → plan (the caller's pipeline) →
/// [`EditSet`] → adapter patchers → atomic writes, plus the M3.2/M3.4
/// lockfile stages (the workspace lockfiles patched from the plan's
/// version moves through each adapter's `patch_lockfile` seam).
///
/// The flow is exactly [`stage_plan_edits`] + write: every file is
/// patched in memory and verified by its adapter before anything is
/// written (§9.1: re-parse + semantic read-back, all or none), then the
/// staged payloads land atomically. A decline
/// ([`LockfileOutcome::NeedsNative`]) is reported in
/// [`ApplyReport::lockfiles`] with the file untouched.
///
/// # Errors
///
/// [`EditError::InvalidPatches`] for stale patch spans
/// (`E-EDIT-PATCHES`) or a manifest no adapter covers,
/// [`EditError::Unrewritable`] for required-to-edit rows the rewriter
/// cannot admit (`E-EDIT-UNREWRITABLE`), [`EditError::VerifyFailed`]
/// when an edited manifest fails the read-back re-parse,
/// [`EditError::LockfileParse`] when a lockfile patch pass fails
/// outright (`E-LOCK-PARSE`), and [`EditError::Io`] for filesystem
/// failures.
pub fn apply_plan(
    root: &Path,
    plan: &Plan,
    workspace: &Workspace,
    adapters: &[&dyn EcosystemAdapter],
) -> Result<ApplyReport, EditError> {
    let staged = stage_plan_edits(root, plan, workspace, adapters)?;
    // Every manifest patched and verified in memory — write them all,
    // manifests first then the patched lockfiles (the payload order).
    for (file, content) in &staged.payloads {
        atomic_write(&root.join(file.to_path_buf()), content)?;
    }
    Ok(ApplyReport {
        files: staged.files,
        lockfiles: staged.lockfiles,
    })
}

/// The lockfile stages' outcome: the summary rows plus the (path,
/// content, applied, skipped) tuples to write after the manifests land.
struct StagedLockfiles {
    summaries: Vec<LockfileSummary>,
    written: Vec<(RelPath, Vec<u8>, usize, usize)>,
}

/// Patches the workspace lockfiles in memory from the plan's
/// per-ecosystem version moves (§9.2/§9.3). No releases for an
/// ecosystem, no lockfile on disk, or an empty move set → no stage row.
fn stage_lockfiles(
    root: &Path,
    plan: &Plan,
    adapters: &[&dyn EcosystemAdapter],
) -> Result<StagedLockfiles, EditError> {
    let mut staged = StagedLockfiles {
        summaries: Vec::new(),
        written: Vec::new(),
    };
    for (lock_name, ecosystem) in LOCKFILE_TARGETS {
        let changes: Vec<LockfileVersionChange> = plan
            .releases
            .iter()
            .filter_map(|release| {
                LockfileVersionChange::from_release_for_ecosystem(release, ecosystem)
            })
            .collect();
        if changes.is_empty() {
            continue;
        }
        let lock_path = root.join(lock_name);
        if !lock_path.exists() {
            // A workspace without a committed lockfile has nothing to
            // patch (I5: halyard never invents or fetches one).
            continue;
        }
        let adapter = adapters
            .iter()
            .copied()
            .find(|adapter| adapter.id() == ecosystem)
            .ok_or_else(|| EditError::InvalidPatches {
                reason: format!(
                    "plan releases {ecosystem} packages but no {ecosystem} adapter is \
                     registered; cannot patch {lock_name}"
                ),
            })?;
        let file = RelPath::new(lock_name).ok_or(EditError::InvalidPatches {
            reason: format!("{lock_name} is not a valid repo-relative path"),
        })?;
        let original = std::fs::read(&lock_path).map_err(|source| EditError::Io {
            path: lock_name.to_owned(),
            source,
        })?;
        match adapter.patch_lockfile(&file, &original, &changes) {
            Ok(LockfileOutcome::Patched {
                content,
                applied,
                skipped,
            }) => {
                staged.summaries.push(LockfileSummary {
                    file: file.clone(),
                    ecosystem: ecosystem.to_owned(),
                    applied,
                    skipped,
                    native_reason: None,
                });
                staged.written.push((file, content, applied, skipped));
            }
            Ok(LockfileOutcome::NeedsNative { reason }) => {
                // The decline is a report, not an error: the lockfile
                // stays untouched and the orchestrator refreshes natively
                // (§9.2 rule 4 / §9.3).
                staged.summaries.push(LockfileSummary {
                    file,
                    ecosystem: ecosystem.to_owned(),
                    applied: 0,
                    skipped: 0,
                    native_reason: Some(reason),
                });
            }
            Err(failure) => return Err(map_lockfile_failure(failure)),
        }
    }
    Ok(staged)
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
/// `npm`, `pyproject.toml` → `pypi`, `go.mod` → `go` — the tag-only
/// adapter, whose edit seam fails closed, M8 ADR-0057, `pom.xml` →
/// `maven` — the semantic version patch, M8 ADR-0058, any `*.csproj` →
/// `nuget` — the semantic version patch, M8 ADR-0059, `pubspec.yaml` →
/// `dart` — the semantic version patch, M8 ADR-0060, any `*.gemspec` →
/// `rubygems` — the semantic version patch, M8 ADR-0061, `Dockerfile` →
/// `docker` — the semantic version patch, M8 ADR-0063).
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
            let by_name = match file.file_name() {
                "Cargo.toml" => Some("cargo"),
                "package.json" => Some("npm"),
                "pyproject.toml" => Some("pypi"),
                "go.mod" => Some("go"),
                "pom.xml" => Some("maven"),
                // M8 (ADR-0060): the dart adapter's semantic patch owns
                // the pubspec version row.
                "pubspec.yaml" => Some("dart"),
                // M8 (ADR-0063): the docker adapter's semantic patch
                // owns the OCI version label.
                "Dockerfile" => Some("docker"),
                _ => None,
            };
            by_name
                .or_else(|| {
                    // M8 (ADR-0059): every SDK-style project file
                    // routes to the nuget adapter's semantic patch.
                    file.as_str().ends_with(".csproj").then_some("nuget")
                })
                .or_else(|| {
                    // M8 (ADR-0061): every gemspec file routes to the
                    // rubygems adapter's semantic patch.
                    file.as_str().ends_with(".gemspec").then_some("rubygems")
                })
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
