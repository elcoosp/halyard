//! The tag-phase **target precision** seam (§11.1): resolving which
//! commit a release tag should point at — "the commit that introduced
//! V" — instead of the snapshot HEAD fallback.
//!
//! The §11.1 reconciliation decides *what* is pending; the write path
//! ([`crate::tag`]) creates the tags; this module decides *where* the
//! tag lands. The architecture's precision rule finds the introducing
//! commit by a walk on manifest blob-OID change, with a documented HEAD
//! fallback. It was deferred with the tag-phase slice because naive blob
//! reads would break `--filter=blob:none` clones; this implementation
//! keeps that contract honest:
//!
//! * **first-parent walk** — from the request tip down to (exclusive)
//!   the package's release base, bounded by `max_commits`; the first
//!   parent chain is what the engine's diff policy and the tag phase's
//!   target semantics both ride, so a merge landing pinpoints the merge
//!   commit the manifest change arrived in;
//! * **verification, not guessing** — a manifest-OID *change* marks a
//!   candidate (identical OIDs share bytes, hence a version, and are
//!   skipped without blob reads); a candidate counts only when the
//!   caller-supplied [`ManifestVersionProbe`] reads the release version
//!   from the candidate's manifest bytes **and** not from its parent's
//!   — the newest manifest edit that merely touched the file cannot
//!   steal the pinpoint;
//! * **honest degradation** — any unreadable object inside the walk
//!   (the blob-less clone case: missing manifest blobs; the shallow
//!   case: missing parent commits) stops the walk and falls back to the
//!   snapshot HEAD. The resolver never guesses, never errors on
//!   degradation, and always returns a target the tag phase can tag
//!   (HEAD exists by construction);
//! * **I3 determinism** — the walk is a pure function of the repository
//!   content and the request: identical inputs resolve identically.
//!
//! Errors stay on the existing [`crate::GitError`] codes: only an
//! unreadable request *tip* is a hard `E-GIT-OBJECT` (the caller-supplied
//! HEAD is the same contract the write path enforces for tag targets);
//! everything older than the tip degrades instead.

use gix::hash::ObjectId;
use halyard_core::Oid;
use halyard_core::history::HistoryDigest;
use halyard_core::package::Workspace;
use halyard_core::release_state::PendingRelease;
use tracing::trace;

use crate::GitError;
use crate::tag::TagDirective;

/// Extracts the release version from manifest **bytes** — the ecosystem
/// knowledge the git engine must not own (the engine walks; the caller
/// knows how a `Cargo.toml`, a `package.json` or a `pyproject.toml`
/// spells its version — and that a `go.mod` never carries one, M8
/// ADR-0057).
///
/// The shell implements this over its ecosystem adapters (one probe
/// dispatching on the ecosystem string); tests use a minimal text probe.
/// Returning `None` (no parsable version in these bytes) simply means
/// "this commit did not introduce the release version" — the walk
/// continues.
pub trait ManifestVersionProbe {
    /// The version `manifest` (the bytes found at `manifest_path` — the
    /// request's manifest path — in one walked commit) carries under
    /// `ecosystem`'s grammar; `None` when the bytes carry no parsable
    /// version. The path rides the probe because a config-declared
    /// locator picks its pattern PER FILE (M8 ADR-0062: the `generic`
    /// ecosystem's version files are declared, not scanned — the probe
    /// cannot disambiguate two declared patterns without the path).
    fn manifest_version(
        &self,
        ecosystem: &str,
        manifest_path: &str,
        manifest: &[u8],
    ) -> Option<String>;
}

/// One tag-target resolution request: the package's manifest location,
/// the release version the tag phase owes, and the walk window
/// (`head` down to, exclusive, `base` — the package's release base;
/// `None` walks to the root, bounded by the resolver's `max_commits`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagTargetRequest {
    /// Stable ecosystem identifier (`cargo`, `npm`, …) forwarded verbatim
    /// to the probe (the shell dispatches its adapters on it).
    pub ecosystem: String,
    /// Repo-relative manifest path (`crates/a/Cargo.toml`) — the file
    /// whose blob-OID changes drive the walk.
    pub manifest: String,
    /// The release version the tag phase owes (the manifest version the
    /// reconciliation classified as pending).
    pub version: String,
    /// The commit the walk starts at (the snapshot HEAD).
    pub head: Oid,
    /// The package's release base — the last release's tag commit. The
    /// introducing commit lies strictly *after* it; `None` = initial
    /// release, the walk runs to the root (bounded).
    pub base: Option<Oid>,
}

/// Where a resolved target came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagTargetSource {
    /// The walk verified the newest first-parent commit whose manifest
    /// *introduced* the release version (its manifest carries V, its
    /// parent's does not) — the §11.1 precision rule.
    ManifestPinpoint,
    /// The documented §11.1 fallback: the snapshot HEAD. The walk could
    /// not verify a pinpoint (version never committed, bound exhausted,
    /// or an unreadable object — the blob-less clone case).
    HeadFallback,
}

/// One resolved tag target: the commit to tag and how the walk arrived
/// at it (the forecast renders the provenance; the live shell tags the
/// commit either way).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagTarget {
    /// The commit the tag directive should target.
    pub commit: ObjectId,
    /// Whether the walk pinpointed the introducing commit or fell back.
    pub source: TagTargetSource,
}

/// The tag-target resolution seam (§11.1 precision rule). Object-safe
/// (`&dyn`) so the shell and tests can substitute resolvers, mirroring
/// the [`crate::tag::TagWriter`] seam's live/preview split.
pub trait TagTargetResolver {
    /// Resolves the commit `request`'s release tag should target.
    ///
    /// # Errors
    ///
    /// [`GitError::Object`] only when the request's own `head` cannot be
    /// read (the write path's target-existence contract); every other
    /// unverifiable situation degrades to
    /// [`TagTargetSource::HeadFallback`] instead of erroring.
    fn resolve(&self, request: &TagTargetRequest) -> Result<TagTarget, GitError>;
}

/// The default walk bound (commits examined from the tip), mirroring the
/// engine's `initial_release.max_commits` default.
const DEFAULT_MAX_COMMITS: usize = 10_000;

/// The real resolver: owns an opened [`gix::Repository`] and walks the
/// first-parent chain with tree/blob reads through `gix` (no subprocess,
/// like the read path). The probe is caller-supplied — the git engine
/// never parses manifests itself.
pub struct GixTagTargetResolver {
    repo: gix::Repository,
    probe: Box<dyn ManifestVersionProbe>,
    max_commits: usize,
}

impl GixTagTargetResolver {
    /// Opens `path` (worktree or `.git`) for resolution.
    ///
    /// # Errors
    ///
    /// [`GitError::Open`] when the repository cannot be opened.
    pub fn open(
        path: &std::path::Path,
        probe: Box<dyn ManifestVersionProbe>,
    ) -> Result<Self, GitError> {
        let repo = gix::open(path).map_err(|e| GitError::Open {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        Ok(Self {
            repo,
            probe,
            max_commits: DEFAULT_MAX_COMMITS,
        })
    }

    /// Caps the walk at `max_commits` examined commits (the bound is a
    /// cost guard: exhausting it degrades to the HEAD fallback).
    #[must_use]
    pub fn with_max_commits(mut self, max_commits: usize) -> Self {
        self.max_commits = max_commits;
        self
    }

    /// The oid of the blob `path` names inside `tree` (`None` when the
    /// path does not exist there). Tree-read failures and malformed
    /// paths are reported as `Err` — the caller degrades to the fallback
    /// (a missing or corrupt tree means nothing older can be verified
    /// either).
    fn blob_oid_in_tree(&self, tree: ObjectId, path: &[u8]) -> Result<ManifestState, ()> {
        let components: Vec<&[u8]> = path.split(|b: &u8| *b == b'/').collect();
        if components.iter().any(|comp| comp.is_empty()) {
            // A malformed path (`""`, a double slash, a trailing slash)
            // cannot name a manifest file; treat it as absent.
            return Ok(ManifestState::Absent);
        }
        let Some((last, dirs)) = components.split_last() else {
            return Ok(ManifestState::Absent);
        };
        let mut cur = tree;
        for component in dirs {
            // Intermediate components must be directories; anything else
            // (or an absent one) is unverifiable for this path — degrade.
            let (oid, is_tree) = self.tree_entry(cur, component)?;
            if !is_tree {
                return Err(());
            }
            cur = oid;
        }
        // The final component: blobs (or submodule commits) are the
        // manifest carriers; a tree here means the path names a
        // directory — no manifest.
        let (oid, is_tree) = self.tree_entry(cur, last)?;
        if is_tree {
            return Ok(ManifestState::Absent);
        }
        Ok(ManifestState::Present(oid))
    }

    /// Finds one entry by name inside `tree`: its oid and whether it is
    /// a directory. `Err` = unreadable tree or the entry is absent.
    fn tree_entry(&self, tree: ObjectId, name: &[u8]) -> Result<(ObjectId, bool), ()> {
        let object = self.repo.find_object(tree).map_err(|_| ())?;
        if object.kind != gix::objs::Kind::Tree {
            // A non-tree where the path expects one (corrupt or foreign
            // layout) — unverifiable, degrade.
            return Err(());
        }
        let mut iter = gix::objs::TreeRefIter::from_bytes(object.data.as_ref(), tree.kind());
        while let Some(entry) = iter.next().transpose().map_err(|_| ())? {
            if entry.filename == name {
                return Ok((entry.oid.into(), entry.mode.is_tree()));
            }
        }
        Err(())
    }

    /// Reads a blob's bytes; `Err` = unreadable (missing in a blob-less
    /// clone, corrupt) — the caller degrades to the fallback.
    fn read_blob(&self, oid: ObjectId) -> Result<Vec<u8>, ()> {
        let object = self.repo.find_object(oid).map_err(|_| ())?;
        if object.kind != gix::objs::Kind::Blob {
            return Err(());
        }
        let bytes: &[u8] = object.data.as_ref();
        Ok(bytes.to_vec())
    }
}

/// The manifest's presence in one tree (the change-event comparison's
/// two-sided vocabulary: absent counts as a different state, so an
/// addition or a removal of the manifest is a change event).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ManifestState {
    /// The path names a blob (the manifest file).
    Present(ObjectId),
    /// The path is absent from the tree (or names a directory).
    Absent,
}

/// What one change-event verification decided (§11.1 precision rule).
enum Verdict {
    /// The commit introduced the release version — tag it.
    Pinpoint,
    /// No verdict here; the walk continues with the parent.
    Continue,
    /// Verification hit an unreadable object — degrade to the fallback
    /// (the walk never guesses from oids alone).
    Degrade,
}

impl GixTagTargetResolver {
    /// The commit's tree oid and first parent (`None` = root commit);
    /// `None` = the commit is unreadable (shallow cuts, corrupt objects).
    fn commit_shape(&self, oid: ObjectId) -> Option<(ObjectId, Option<ObjectId>)> {
        let commit = self.repo.find_commit(oid).ok()?;
        let tree = commit.tree_id().ok()?.detach();
        let parent = commit.parent_ids().next().map(gix::Id::detach);
        Some((tree, parent))
    }

    /// The manifest's state inside one tree; `None` = the tree is
    /// unreadable.
    fn manifest_oid(&self, tree: ObjectId, path: &str) -> Option<ManifestState> {
        self.blob_oid_in_tree(tree, path.as_bytes()).ok()
    }

    /// The parent commit's manifest state (`Absent` when there is no
    /// parent — the root has nothing to compare against);
    /// `None` = the parent commit or its tree is unreadable.
    fn parent_manifest(&self, parent: Option<ObjectId>, manifest: &str) -> Option<ManifestState> {
        let Some(oid) = parent else {
            return Some(ManifestState::Absent);
        };
        let (tree, _) = self.commit_shape(oid)?;
        self.manifest_oid(tree, manifest)
    }

    /// Reads a manifest blob and probes its version. `Err` = the blob is
    /// unreadable (the blob-less clone case); `Ok(None)` = readable but
    /// the probe found no parsable version. The manifest path rides the
    /// probe (a config-declared locator picks its pattern per file, M8
    /// ADR-0062).
    fn probed_version(
        &self,
        ecosystem: &str,
        manifest_path: &str,
        blob: ObjectId,
    ) -> Result<Option<String>, ()> {
        let bytes = self.read_blob(blob)?;
        Ok(self
            .probe
            .manifest_version(ecosystem, manifest_path, &bytes))
    }

    /// Verifies one manifest-OID change event (§11.1 precision rule): a
    /// change event can only *introduce* the release version when the
    /// content actually carries V — verified by a probe read — and the
    /// parent does not carry V too (a manifest edit that kept the version
    /// must not steal the pinpoint; a removal cannot introduce one).
    fn verify_change_event(
        &self,
        request: &TagTargetRequest,
        manifest: ManifestState,
        parent_manifest: ManifestState,
    ) -> Verdict {
        let ManifestState::Present(blob) = manifest else {
            // The manifest disappeared at this commit — a removal, not an
            // introduction.
            return Verdict::Continue;
        };
        let Ok(version) = self.probed_version(&request.ecosystem, &request.manifest, blob) else {
            return Verdict::Degrade;
        };
        if version.as_deref() != Some(request.version.as_str()) {
            return Verdict::Continue;
        }
        let ManifestState::Present(parent_blob) = parent_manifest else {
            // Nothing comparable above (no parent, or the package's
            // manifest first appears here): this commit carries V and its
            // lineage did not — the bootstrap introduced it (the
            // initial-release state, §8.2).
            return Verdict::Pinpoint;
        };
        match self.probed_version(&request.ecosystem, &request.manifest, parent_blob) {
            Err(()) => Verdict::Degrade,
            Ok(parent_version) if parent_version.as_deref() == Some(request.version.as_str()) => {
                Verdict::Continue
            }
            Ok(_) => Verdict::Pinpoint,
        }
    }
}

impl TagTargetResolver for GixTagTargetResolver {
    fn resolve(&self, request: &TagTargetRequest) -> Result<TagTarget, GitError> {
        let head = core_oid_to_object_id(&request.head);
        // The tip is the caller-supplied HEAD: the write path's
        // target-existence contract applies to it — unreadable is a hard
        // object error, not a degradation.
        self.repo.find_commit(head).map_err(|e| GitError::Object {
            message: format!("cannot read the resolution tip {head}: {e}"),
        })?;
        let base = request.base.as_ref().map(core_oid_to_object_id);
        let fallback = TagTarget {
            commit: head,
            source: TagTargetSource::HeadFallback,
        };
        let mut cur = head;
        let mut examined = 0usize;
        loop {
            // The window is (base, head]: reaching the base (or the bound)
            // ends the search without a verified pinpoint.
            if Some(cur) == base || examined >= self.max_commits {
                trace!(
                    manifest = %request.manifest,
                    version = %request.version,
                    "tag-target walk exhausted without a verified pinpoint"
                );
                return Ok(fallback.clone());
            }
            // Everything below the tip is best-effort: an unreadable
            // commit/tree/parent degrades to the documented fallback
            // (shallow cuts and blob-less clones cannot be verified).
            let Some((tree, parent)) = self.commit_shape(cur) else {
                trace!(commit = %cur, "tag-target walk hit an unreadable commit");
                return Ok(fallback.clone());
            };
            let Some(cur_manifest) = self.manifest_oid(tree, &request.manifest) else {
                trace!(commit = %cur, "tag-target walk hit an unreadable tree");
                return Ok(fallback.clone());
            };
            let Some(parent_manifest) = self.parent_manifest(parent, &request.manifest) else {
                trace!(commit = %cur, "tag-target walk hit an unreadable parent commit or tree");
                return Ok(fallback.clone());
            };
            // Identical oids share bytes (hence a version): only a change
            // event is a candidate, so unchanged runs skip blob reads
            // entirely.
            if cur_manifest != parent_manifest {
                match self.verify_change_event(request, cur_manifest, parent_manifest) {
                    Verdict::Pinpoint => {
                        return Ok(TagTarget {
                            commit: cur,
                            source: TagTargetSource::ManifestPinpoint,
                        });
                    }
                    Verdict::Degrade => {
                        trace!(
                            commit = %cur,
                            manifest = %request.manifest,
                            "tag-target walk hit an unreadable manifest blob"
                        );
                        return Ok(fallback.clone());
                    }
                    Verdict::Continue => {}
                }
            }
            examined += 1;
            let Some(next) = parent else {
                trace!(
                    manifest = %request.manifest,
                    version = %request.version,
                    "tag-target walk reached the root without a match"
                );
                return Ok(fallback.clone());
            };
            cur = next;
        }
    }
}

/// One pending release turned into a tag directive *with* its resolved
/// target (§11.1 precision rule): the directive the executor consumes
/// plus the provenance the forecast renders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedDirective {
    /// The directive (name, resolved target commit, minimal annotation).
    pub directive: TagDirective,
    /// How the target was resolved.
    pub source: TagTargetSource,
}

/// Turns the reconciliation queue into tag-phase directives with §11.1
/// precise targets: per pending release the workspace's package supplies
/// manifest path and ecosystem, the digest supplies the release base,
/// and `resolver` pinpoints the introducing commit. Every unresolvable
/// detail (a package the workspace no longer knows, a resolution error)
/// keeps the phase runnable: the directive targets the documented HEAD
/// fallback with the same minimal annotation. The output preserves the
/// pending queue's order (the executor re-canonicalizes anyway).
#[must_use]
pub fn directives_for_pending_resolved(
    pending: &[PendingRelease],
    ws: &Workspace,
    digest: &HistoryDigest,
    head: &Oid,
    resolver: &dyn TagTargetResolver,
) -> Vec<ResolvedDirective> {
    pending
        .iter()
        .map(|release| {
            let fallback = TagDirective {
                name: release.tag.clone(),
                target: core_oid_to_object_id(head),
                message: format!("{} {}", release.package, release.version),
            };
            let Some(pkg_id) = ws.find_by_name(&release.package) else {
                trace!(
                    package = %release.package,
                    "stale pending release: the workspace no longer knows the package"
                );
                return ResolvedDirective {
                    directive: fallback,
                    source: TagTargetSource::HeadFallback,
                };
            };
            let Some(pkg) = ws.package(pkg_id) else {
                return ResolvedDirective {
                    directive: fallback,
                    source: TagTargetSource::HeadFallback,
                };
            };
            let request = TagTargetRequest {
                ecosystem: pkg.ecosystem.resolve(ws.interner()).to_owned(),
                manifest: pkg.manifest.as_str().to_owned(),
                version: release.version.to_string(),
                head: head.clone(),
                base: digest.base_of(pkg_id).map(|base_ref| base_ref.oid.clone()),
            };
            match resolver.resolve(&request) {
                Ok(target) => ResolvedDirective {
                    directive: TagDirective {
                        name: release.tag.clone(),
                        target: target.commit,
                        message: fallback.message,
                    },
                    source: target.source,
                },
                Err(error) => {
                    trace!(
                        package = %release.package,
                        error = %error,
                        "tag-target resolution failed; falling back to HEAD"
                    );
                    ResolvedDirective {
                        directive: fallback,
                        source: TagTargetSource::HeadFallback,
                    }
                }
            }
        })
        .collect()
}

/// Maps the core hex-carrier oid onto a `gix` oid (total: core oids are
/// valid hex by construction; the static zero fallback keeps the mapping
/// panic-free — the write path then fails closed on the unknown target).
fn core_oid_to_object_id(oid: &Oid) -> ObjectId {
    ObjectId::from_hex(oid.as_bytes()).unwrap_or_else(|_| {
        ObjectId::from_hex(b"0000000000000000000000000000000000000000")
            .unwrap_or_else(|_| unreachable!("static zero hex is valid"))
    })
}
