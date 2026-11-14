//! The tag-phase write path (§11.1 step 2, git-side effects) — the first
//! write surface in the git engine.
//!
//! The §11.1 reconciliation ([`halyard_core::release_state`]) decides
//! *what* is pending; this module performs the git-side half of the tag
//! phase: creating annotated release tags. It is deliberately small and
//! fail-closed, and it rides the same conventions as the read path:
//!
//! * **I1 idempotence** — a directive whose tag already exists at the
//!   requested target reports [`TagOutcome::AlreadyTagged`] and writes
//!   nothing (a lightweight tag at the target counts: the ref is what
//!   ships releases, not the object behind it);
//! * **I2 resumability** — the tag exists at a *different* target ⇒
//!   `E-GIT-TAG-CONFLICT`, fail-closed: the reconciliation never moves a
//!   release tag (a moved tag would misstate what was released). A
//!   partially-completed phase re-runs to completion with zero
//!   duplicates — created tags report `AlreadyTagged`, the rest create;
//! * **I3 determinism** — the tag object is a pure function of
//!   (name, target, message, tagger identity): identical inputs produce
//!   byte-identical objects (and therefore identical oids) in any
//!   repository.
//!
//! The executor ([`run_tag_phase`]) consumes directives — built from the
//! reconciliation queue by [`directives_for_pending`] — through the
//! [`TagWriter`] seam, so the live shell, the offline forecast and tests
//! share one code path.

use bstr::ByteSlice as _;
use gix::hash::ObjectId;
use halyard_core::Oid;
use halyard_core::release_state::PendingRelease;

use crate::GitError;

/// The tagger identity stamped into every annotated tag. The shell
/// supplies it (git config + clock); the writer never invents one — an
/// incomplete identity is a fail-closed `E-GIT-TAG`, never a synthetic
/// default (I3: the identity is part of the tag object's bytes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaggerIdentity {
    /// Tagger name (e.g. the repository's `user.name`).
    pub name: String,
    /// Tagger email (e.g. the repository's `user.email`).
    pub email: String,
    /// Tag creation instant, seconds since the unix epoch (UTC).
    pub seconds: i64,
}

impl TaggerIdentity {
    /// Builds an identity at `seconds` (UTC, offset `+0000`).
    #[must_use]
    pub fn new(name: impl Into<String>, email: impl Into<String>, seconds: i64) -> Self {
        Self {
            name: name.into(),
            email: email.into(),
            seconds,
        }
    }

    /// `true` when both name and email are non-empty.
    fn is_complete(&self) -> bool {
        !self.name.trim().is_empty() && !self.email.trim().is_empty()
    }

    /// The `gix` signature this identity renders to.
    fn signature(&self) -> gix::actor::Signature {
        gix::actor::Signature {
            name: self.name.clone().into(),
            email: self.email.clone().into(),
            time: gix::date::Time {
                seconds: self.seconds,
                offset: 0,
            },
        }
    }
}

/// One annotated tag the §11.1 step-2 tag phase owes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagDirective {
    /// Short tag name (`v0.1.0`, `core-v0.4.1`) — rendered through the
    /// package's [`halyard_core::TagTemplate`], exactly what the
    /// reconciliation compared against. Validated before anything is
    /// written (invalid names are `E-GIT-TAG`, nothing lands).
    pub name: String,
    /// The commit to tag. The §11.1 precision rule ("the commit that
    /// introduced V") resolves through the [`crate::tag_target`] seam
    /// ([`crate::tag_target::directives_for_pending_resolved`]); when a
    /// target cannot be pinpointed, the documented §11.1 fallback
    /// applies: the snapshot HEAD.
    pub target: ObjectId,
    /// The annotation message. The release-notes pipeline (§9.4) renders
    /// real notes once it lands; until then the directive carries the
    /// deterministic minimal text (`"{package} {version}"`) — the forecast
    /// does not fabricate entries that do not exist yet.
    pub message: String,
}

/// The result of one directive against the repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TagOutcome {
    /// The annotated tag object was written and the ref created
    /// (create-only: the ref must not have existed, so two racers cannot
    /// both claim a create).
    Created {
        /// The oid of the written tag object.
        tag_oid: ObjectId,
    },
    /// The tag already exists at the requested target — the phase is a
    /// no-op for this directive (I1; the I2 resume shape after a partial
    /// failure).
    AlreadyTagged {
        /// The target the directive asked for (equal to the existing
        /// tag's peeled commit).
        target: ObjectId,
    },
}

/// The write-path seam of the tag phase (§11.1 step 2, git-side).
///
/// Object-safe (`&dyn`) so the live shell runs the real writer
/// ([`GixTagWriter`]) and the offline forecast runs the preview writer —
/// one executor, two effects, mirroring the forge seam's `MemoryForge` /
/// `PreviewForge` split.
pub trait TagWriter {
    /// Creates the annotated tag `directive` describes, idempotently:
    /// an existing tag at the requested target is
    /// [`TagOutcome::AlreadyTagged`]; an existing tag at a different
    /// target is a fail-closed `E-GIT-TAG-CONFLICT` (never moved, I2).
    ///
    /// # Errors
    ///
    /// [`GitError::TagWrite`] for validation/object/ref-write failures,
    /// [`GitError::TagConflict`] when the tag exists elsewhere.
    fn create_annotated_tag(&self, directive: &TagDirective) -> Result<TagOutcome, GitError>;
}

/// The real writer: owns an opened [`gix::Repository`] and writes the
/// annotated tag objects and `refs/tags/*` updates through `gix` (no
/// subprocess, invariant I6's effect-step exception does not apply — the
/// write path is in-process like the read path).
pub struct GixTagWriter {
    repo: gix::Repository,
    tagger: TaggerIdentity,
}

impl GixTagWriter {
    /// Opens `path` (worktree or `.git`) for writing.
    ///
    /// # Errors
    ///
    /// [`GitError::Open`] when the repository cannot be opened.
    pub fn open(path: &std::path::Path, tagger: TaggerIdentity) -> Result<Self, GitError> {
        let repo = gix::open(path).map_err(|e| GitError::Open {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        Ok(Self { repo, tagger })
    }
}

impl TagWriter for GixTagWriter {
    fn create_annotated_tag(&self, directive: &TagDirective) -> Result<TagOutcome, GitError> {
        let full_name = format!("refs/tags/{}", directive.name);
        // Validation first (E-GIT-TAG): git's reference-name rules on the
        // full name — a rejected directive must not write anything.
        if let Err(error) = gix::refs::FullName::try_from(full_name.as_str()) {
            return Err(GitError::TagWrite {
                message: format!(
                    "tag name {:?} is not a valid reference name: {error}",
                    directive.name
                ),
            });
        }
        // The target must exist and be a commit (§11.1 tags commits).
        self.repo
            .find_commit(directive.target)
            .map_err(|e| GitError::TagWrite {
                message: format!(
                    "tag target {} does not name an existing commit: {e}",
                    directive.target
                ),
            })?;
        // Idempotence pre-check (I1): an existing ref at the target is the
        // no-op outcome; elsewhere is the fail-closed conflict.
        if let Some(existing) = self
            .repo
            .try_find_reference(full_name.as_str())
            .map_err(|e| GitError::TagWrite {
                message: format!("cannot read {full_name}: {e}"),
            })?
        {
            let peeled = existing
                .into_fully_peeled_id()
                .map_err(|e| GitError::TagWrite {
                    message: format!("cannot peel {full_name}: {e}"),
                })?
                .detach();
            return if peeled == directive.target {
                Ok(TagOutcome::AlreadyTagged {
                    target: directive.target,
                })
            } else {
                Err(conflict(directive, peeled))
            };
        }
        // The identity is part of the tag object's bytes — an incomplete
        // identity would fabricate a malformed tagger header.
        if !self.tagger.is_complete() {
            return Err(GitError::TagWrite {
                message: "tagger identity is incomplete; configure user.name/user.email \
                  or pass an explicit identity"
                    .to_owned(),
            });
        }
        // Write the annotated tag object (I3: pure function of the
        // directive + identity).
        let tag = gix::objs::Tag {
            target: directive.target,
            target_kind: gix::objs::Kind::Commit,
            name: directive.name.as_bytes().as_bstr().to_owned(),
            tagger: Some(self.tagger.signature()),
            message: directive.message.as_bytes().as_bstr().to_owned(),
            signature: None,
        };
        let tag_oid = self
            .repo
            .write_object(tag)
            .map_err(|e| GitError::TagWrite {
                message: format!("cannot write tag object for {}: {e}", directive.name),
            })?
            .detach();
        // Create-only ref update: `MustNotExist` makes two concurrent
        // creators race to one winner; a loser falls into the reclassify
        // below instead of duplicating or moving the tag.
        let log = format!("halyard: tag phase (create {})", directive.name);
        if let Err(error) = self.repo.reference(
            full_name.as_str(),
            tag_oid,
            gix::refs::transaction::PreviousValue::MustNotExist,
            log,
        ) {
            // Re-read and classify: a racer may have created the very same
            // tag between the pre-check and the write (I2, zero duplicates).
            if let Some(existing) =
                self.repo
                    .try_find_reference(full_name.as_str())
                    .map_err(|e| GitError::TagWrite {
                        message: format!("cannot re-read {full_name} after a failed create: {e}"),
                    })?
            {
                let peeled = existing
                    .into_fully_peeled_id()
                    .map_err(|e| GitError::TagWrite {
                        message: format!("cannot peel {full_name} after a failed create: {e}"),
                    })?
                    .detach();
                return if peeled == directive.target {
                    Ok(TagOutcome::AlreadyTagged {
                        target: directive.target,
                    })
                } else {
                    Err(conflict(directive, peeled))
                };
            }
            return Err(GitError::TagWrite {
                message: format!("cannot create {full_name}: {error}"),
            });
        }
        Ok(TagOutcome::Created { tag_oid })
    }
}

/// Builds the fail-closed conflict error for a tag that exists at
/// `peeled` while the directive asked for `directive.target`.
fn conflict(directive: &TagDirective, peeled: ObjectId) -> GitError {
    GitError::TagConflict {
        tag: directive.name.clone(),
        existing: peeled.to_string(),
        requested: directive.target.to_string(),
    }
}

/// The identity the forecast stamps into its shape hash — a documented
/// constant, never read from the environment (I3: the forecast must not
/// depend on the machine's git config or clock). The live tag object
/// differs by construction (real identity + wall clock); the forecast
/// oid is the *byte-shape key* of the tag the directive describes, not a
/// promise of the live oid.
const FORECAST_IDENTITY: TaggerIdentity = TaggerIdentity {
    name: String::new(),
    email: String::new(),
    seconds: 0,
};

/// The would-be tag object oid of `directive` under the neutral forecast
/// identity — the same serialization the live writer performs, hashed
/// (`tag {size}\0{body}`), without touching the object database.
///
/// # Errors
///
/// [`GitError::TagWrite`] when the tag object cannot be serialized.
fn forecast_tag_oid(directive: &TagDirective) -> Result<ObjectId, GitError> {
    let tag = gix::objs::Tag {
        target: directive.target,
        target_kind: gix::objs::Kind::Commit,
        name: directive.name.as_bytes().as_bstr().to_owned(),
        tagger: Some(FORECAST_IDENTITY.signature()),
        message: directive.message.as_bytes().as_bstr().to_owned(),
        signature: None,
    };
    let mut body = Vec::new();
    gix::objs::WriteTo::write_to(&tag, &mut body).map_err(|e| GitError::TagWrite {
        message: format!(
            "cannot serialize the forecast tag object for {}: {e}",
            directive.name
        ),
    })?;
    let mut hasher = gix::hash::hasher(gix::hash::Kind::Sha1);
    hasher.update(format!("tag {}\0", body.len()).as_bytes());
    hasher.update(&body);
    Ok(
        // Sha1 finalization only fails on a detected collision —
        // unreachable for forecast-sized inputs; the unreachable fallback
        // keeps the mapping total (§15.3).
        hasher.try_finalize().unwrap_or_else(|e| {
            unreachable!("sha1 finalization of the forecast tag object failed: {e}")
        }),
    )
}

/// The dry-run writer (the tag-phase counterpart of the forge seam's
/// preview adapter): probes the **real** repository — validation, target
/// existence and the existing-tag classification are live reads, so the
/// forecast can never disagree with what the run over
/// [`GixTagWriter`] would do — and forecasts the mutations: a pending
/// directive reports [`TagOutcome::Created`] with the neutral-shape oid
/// (the documented `FORECAST_IDENTITY` constant) and writes nothing (no
/// object, no ref).
pub struct PreviewTagWriter {
    repo: gix::Repository,
}

impl PreviewTagWriter {
    /// Opens `path` for forecasting (probes only).
    ///
    /// # Errors
    ///
    /// [`GitError::Open`] when the repository cannot be opened.
    pub fn open(path: &std::path::Path) -> Result<Self, GitError> {
        let repo = gix::open(path).map_err(|e| GitError::Open {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        Ok(Self { repo })
    }
}

impl TagWriter for PreviewTagWriter {
    fn create_annotated_tag(&self, directive: &TagDirective) -> Result<TagOutcome, GitError> {
        let full_name = format!("refs/tags/{}", directive.name);
        // The forecast raises exactly the failures the live writer would:
        // name validation and target existence first (fail-closed before
        // any classification).
        if let Err(error) = gix::refs::FullName::try_from(full_name.as_str()) {
            return Err(GitError::TagWrite {
                message: format!(
                    "tag name {:?} is not a valid reference name: {error}",
                    directive.name
                ),
            });
        }
        self.repo
            .find_commit(directive.target)
            .map_err(|e| GitError::TagWrite {
                message: format!(
                    "tag target {} does not name an existing commit: {e}",
                    directive.target
                ),
            })?;
        // The idempotence/conflict classification is a real probe.
        if let Some(existing) = self
            .repo
            .try_find_reference(full_name.as_str())
            .map_err(|e| GitError::TagWrite {
                message: format!("cannot read {full_name}: {e}"),
            })?
        {
            let peeled = existing
                .into_fully_peeled_id()
                .map_err(|e| GitError::TagWrite {
                    message: format!("cannot peel {full_name}: {e}"),
                })?
                .detach();
            return if peeled == directive.target {
                Ok(TagOutcome::AlreadyTagged {
                    target: directive.target,
                })
            } else {
                Err(conflict(directive, peeled))
            };
        }
        // Mutation → forecast (no object write, no ref update).
        Ok(TagOutcome::Created {
            tag_oid: forecast_tag_oid(directive)?,
        })
    }
}

/// One row of the [`TagPhaseReport`], in canonical (name-sorted) order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TagPhaseRow {
    /// The directive applied (created or already tagged).
    Applied {
        /// The short tag name.
        tag: String,
        /// What happened.
        outcome: TagOutcome,
    },
    /// The directive failed; the batch continued (I2) and the shell
    /// renders the error with its stable code.
    Failed {
        /// The short tag name.
        tag: String,
        /// The failure with its `E-GIT-*` code.
        error: GitError,
    },
}

/// The tag phase's git-side report (§11.1 step 2): one row per
/// (deduplicated) directive, in name order, with the outcome counters.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TagPhaseReport {
    /// Per-directive rows, name-sorted (I3: input order cannot leak).
    pub rows: Vec<TagPhaseRow>,
    /// Directives that created a tag.
    pub created: usize,
    /// Directives that were already satisfied.
    pub already_tagged: usize,
    /// Directives that failed (the phase continues past them, I2).
    pub failed: usize,
}

/// Runs the git-side tag phase (§11.1 step 2) over `writer`.
///
/// The executor owns the canonical pass: directives are sorted by tag
/// name and deduplicated (the reconciliation queue is already canonical —
/// [`halyard_core::release_state::pending_releases`] — so a caller that
/// lost the order still cannot produce a divergent phase), each directive
/// applies through `writer`, one failure does not abort the batch (I2:
/// the re-run reaches the same end state with zero duplicates) and the
/// report is deterministic.
#[must_use]
pub fn run_tag_phase(writer: &dyn TagWriter, directives: &[TagDirective]) -> TagPhaseReport {
    // Canonical order + dedup by name (I3; defense in depth on top of the
    // reconciliation queue's own sort+dedup).
    let mut canonical: Vec<&TagDirective> = directives.iter().collect();
    canonical.sort_by(|a, b| a.name.cmp(&b.name));
    canonical.dedup_by(|a, b| a.name == b.name);
    let mut report = TagPhaseReport::default();
    for directive in canonical {
        match writer.create_annotated_tag(directive) {
            Ok(outcome) => {
                match outcome {
                    TagOutcome::Created { .. } => report.created += 1,
                    TagOutcome::AlreadyTagged { .. } => report.already_tagged += 1,
                }
                report.rows.push(TagPhaseRow::Applied {
                    tag: directive.name.clone(),
                    outcome,
                });
            }
            Err(error) => {
                report.failed += 1;
                report.rows.push(TagPhaseRow::Failed {
                    tag: directive.name.clone(),
                    error,
                });
            }
        }
    }
    report
}

/// Turns the reconciliation queue into tag-phase directives: one annotated
/// tag per pending release, targeting `head` (the documented §11.1
/// fallback). The §11.1 precision rule — the commit that introduced the
/// release version — resolves through
/// [`crate::tag_target::directives_for_pending_resolved`], which falls
/// back to this shape whenever a target cannot be verified (e.g. a
/// `--filter=blob:none` clone). The message is the deterministic minimal
/// annotation (`"{package} {version}"`); the §9.4 notes pipeline replaces
/// it when it lands.
#[must_use]
pub fn directives_for_pending(pending: &[PendingRelease], head: &Oid) -> Vec<TagDirective> {
    pending
        .iter()
        .map(|release| TagDirective {
            name: release.tag.clone(),
            target: core_oid_to_object_id(head),
            message: format!("{} {}", release.package, release.version),
        })
        .collect()
}

/// Maps the core hex-carrier oid onto a `gix` oid (total: core oids are
/// valid hex by construction; the static zero fallback keeps the mapping
/// panic-free — the writer then fails closed on the unknown target).
fn core_oid_to_object_id(oid: &Oid) -> ObjectId {
    ObjectId::from_hex(oid.as_bytes()).unwrap_or_else(|_| {
        ObjectId::from_hex(b"0000000000000000000000000000000000000000")
            .unwrap_or_else(|_| unreachable!("static zero hex is valid"))
    })
}
