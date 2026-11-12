//! The git engine (M2.1/M2.2, §8): ref enumeration + base resolution
//! ([`refs`]) and the painter walk ([`walk`]), assembled into a
//! [`GitHistory`] that satisfies the planner's
//! [`HistoryProvider`](halyard_core::HistoryProvider) seam.
//!
//! This crate is imperative-shell territory: it owns `gix` access, ODB
//! reads and shallow-clone detection. It performs **no subprocess** (no
//! `git` binary, invariant I6) and no network (I5). The planner consumes
//! only the materialized [`HistoryDigest`](halyard_core::HistoryDigest).
//!
//! Layer note (ADR-0009): this crate depends on `halyard-core` **and**
//! `halyard-conventional` — the walk emits parsed commits (§5.7), and the
//! parser is a zero-dependency leaf, so the edge stays cycle-free.

#![forbid(unsafe_code)]

pub mod refs;
pub mod walk;

// Re-export for downstream differential tests and integrators that need
// the exact `gix` version this engine was compiled against.
pub use gix;

use std::path::Path;

use gix::hash::ObjectId;
use halyard_core::history::{AuthorRef, BaseRef, HistoryDigest, RelevantCommit};
use halyard_core::{BString, HistoryProvider, Oid};

/// Git-engine failures (§15.4 `E-GIT-*`; each variant has a page under
/// `docs/errors/`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitError {
    /// The repository cannot be opened (`E-GIT-OPEN`).
    Open {
        /// The path that was opened.
        path: String,
        /// Underlying gix error text.
        message: String,
    },
    /// HEAD is unborn or missing (`E-GIT-HEAD`): nothing has been
    /// committed yet.
    Head {
        /// The repository path (for the error's guidance).
        path: String,
    },
    /// A shallow clone cannot support reliable history (§8.1,
    /// `E-GIT-SHALLOW`); deepen with `git fetch --unshallow` or use
    /// `halyard fetch`.
    Shallow {
        /// The repository path.
        path: String,
    },
    /// The reference database could not be read (`E-GIT-REF`).
    RefScan {
        /// What went wrong.
        message: String,
    },
    /// An object (commit/tag) could not be read (`E-GIT-OBJECT`).
    Object {
        /// What went wrong.
        message: String,
    },
}

impl GitError {
    /// Stable §15.4 code of this error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Open { .. } => "E-GIT-OPEN",
            Self::Head { .. } => "E-GIT-HEAD",
            Self::Shallow { .. } => "E-GIT-SHALLOW",
            Self::RefScan { .. } => "E-GIT-REF",
            Self::Object { .. } => "E-GIT-OBJECT",
        }
    }
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open { path, message } => {
                write!(f, "[E-GIT-OPEN] cannot open repository {path}: {message}")
            }
            Self::Head { path } => write!(
                f,
                "[E-GIT-HEAD] {path} has no commits yet (unborn HEAD); make an initial commit \
                 before planning"
            ),
            Self::Shallow { path } => write!(
                f,
                "[E-GIT-SHALLOW] {path} is a shallow clone; history is truncated so release \
                 bases cannot be trusted — run `git fetch --unshallow` (or `halyard fetch`)"
            ),
            Self::RefScan { message } => write!(f, "[E-GIT-REF] {message}"),
            Self::Object { message } => write!(f, "[E-GIT-OBJECT] {message}"),
        }
    }
}

impl std::error::Error for GitError {}

/// Builds a [`GitError`] with a code (helper for mapping gix errors).
pub(crate) fn map_git_err(code: &'static str, message: String) -> GitError {
    match code {
        "E-GIT-REF" => GitError::RefScan { message },
        "E-GIT-OBJECT" => GitError::Object { message },
        // The builder only forwards the two codes above.
        _ => GitError::Object { message },
    }
}

/// The materialized history of one repository (M2): head, per-package
/// release bases, and the digest of unreleased commits per package.
/// Implements [`HistoryProvider`] so the planner consumes it exactly like
/// the in-memory [`halyard_core::FixedHistory`].
#[derive(Clone, Debug)]
pub struct GitHistory {
    head: Option<Oid>,
    digest: HistoryDigest,
    /// True when the walk ran without a commit-graph (§8.3 degraded
    /// marker; correctness unaffected).
    pub degraded: bool,
    /// True when the walk stopped at `initial_release.max_commits` (§8.3).
    pub truncated: bool,
}

impl GitHistory {
    /// HEAD at snapshot time.
    pub fn head_oid(&self) -> Option<Oid> {
        self.head.clone()
    }

    /// The resolved release bases (aligned with the workspace packages).
    pub fn bases(&self) -> &[Option<BaseRef>] {
        &self.digest.bases
    }

    /// Opens `path`, resolves bases for every package in `ws` (§8.2) and
    /// paints the unreleased history (§8.3). Shallow clones are rejected
    /// with [`GitError::Shallow`] (§8.1); `initial_max_commits` bounds
    /// initial-release walks.
    ///
    /// # Errors
    ///
    /// [`GitError`] for open/head/shallow/ref/object failures.
    pub fn build(
        path: &Path,
        ws: &halyard_core::Workspace,
        initial_max_commits: u32,
    ) -> Result<Self, GitError> {
        let repo = gix::open(path).map_err(|e| GitError::Open {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        let repo_path = path.display().to_string();
        if is_shallow(&repo) {
            return Err(GitError::Shallow { path: repo_path });
        }
        let head = repo
            .head_id()
            .ok()
            .map(|id| id.detach());
        let Some(head) = head else {
            return Err(GitError::Head { path: repo_path });
        };
        let mut generations = refs::Generations::new(&repo);
        let (base_refs, _tags) = refs::resolve_bases(&repo, head, ws, &mut generations)?;
        let base_oids: Vec<Option<ObjectId>> = base_refs
            .iter()
            .map(|base| {
                base.as_ref().and_then(|b| {
                    ObjectId::from_hex(b.oid.as_bytes()).ok()
                })
            })
            .collect();
        let painted = walk::paint(&repo, head, &base_oids, initial_max_commits, &mut generations)?;
        let digest = assemble(&repo, &painted, &base_refs)?;
        Ok(Self {
            head: Some(core_oid_of(head)),
            digest,
            degraded: painted.degraded,
            truncated: painted.truncated,
        })
    }
}

impl HistoryProvider for GitHistory {
    fn head(&self) -> Option<Oid> {
        self.head.clone()
    }

    fn digest(&self) -> &HistoryDigest {
        &self.digest
    }
}

/// True when the repository is a shallow clone (`.git/shallow` present,
/// §8.1). Works for worktrees too (the common dir carries the file).
fn is_shallow(repo: &gix::Repository) -> bool {
    let git_dir = repo.git_dir();
    if git_dir.join("shallow").is_file() {
        return true;
    }
    repo.common_dir()
        .join("shallow")
        .is_file()
}

/// Maps a gix oid onto the core hex-carrier (total: gix oids are hex).
fn core_oid_of(oid: ObjectId) -> Oid {
    let hex = oid.to_string();
    Oid::from_hex(hex.as_bytes()).unwrap_or_else(|| {
        Oid::from_hex(b"0000000000000000000000000000000000000000")
            .unwrap_or_else(|| Oid::from_hex(b"1111111111111111111111111111111111111111").expect("static hex is valid"))
    })
}

/// Assembles the [`HistoryDigest`] from the painted walk: every emitted
/// commit is read once, its message parsed through
/// `halyard-conventional` (non-conventional and autosquash commits are
/// walked but not carried — §8.5), and per-package attribution comes
/// straight from the walk's `new_for` bitsets (path-agnostic until M2.3's
/// tree diff).
///
/// # Errors
///
/// [`GitError::Object`] when a commit cannot be read.
fn assemble(
    repo: &gix::Repository,
    painted: &walk::PaintResult,
    base_refs: &[Option<BaseRef>],
) -> Result<HistoryDigest, GitError> {
    let mut commits: Vec<RelevantCommit> = Vec::new();
    for emitted in &painted.commits {
        let commit = repo
            .find_commit(emitted.oid)
            .map_err(|e| {
                let oid = emitted.oid;
                GitError::Object {
                    message: format!("cannot read commit {oid}: {e}"),
                }
            })?;
        let raw_message = commit
            .message_raw()
            .map_err(|e| {
                let oid = emitted.oid;
                GitError::Object {
                    message: format!("cannot read message of {oid}: {e}"),
                }
            })?
            .as_ref();
        // Skip non-conventional messages (including `fixup!`/`squash!`,
        // §8.5): they contribute no bump and carry no reason.
        let Some(parsed) = halyard_conventional::parse(raw_message) else {
            continue;
        };
        let subject_end = raw_message
            .iter()
            .position(|&b| b == b'\n')
            .unwrap_or(raw_message.len());
        // Body = everything after the blank line following the subject
        // (`0..0` when the message has no body, §5.7).
        let body_start = (subject_end..raw_message.len().saturating_sub(1))
            .find(|&i| raw_message.get(i) == Some(&b'\n') && raw_message.get(i + 1) == Some(&b'\n'))
            .map_or(0, |i| i + 2);
        let author = commit.author().map_err(|e| {
            let oid = emitted.oid;
            GitError::Object {
                message: format!("cannot read author of {oid}: {e}"),
            }
        })?;
        commits.push(RelevantCommit {
            oid: core_oid_of(emitted.oid),
            subject: BString::from(&raw_message[..subject_end]),
            body_range: u32::try_from(body_start)
                .map(|s| s..u32::try_from(raw_message.len()).unwrap_or(u32::MAX))
                .unwrap_or(0..0),
            parsed: halyard_core::history::ParsedCommit {
                kind: parsed.kind.clone(),
                scope: parsed.scope.clone(),
                breaking: parsed.breaking,
                description: parsed.description.clone(),
            },
            pr: parsed.pr,
            author: AuthorRef {
                name: BString::from(author.name),
                email: BString::from(author.email),
            },
            trailers: parsed
                .footers
                .iter()
                .map(|t| halyard_core::history::Trailer {
                    key: t.key.clone(),
                    value: t.value.clone(),
                })
                .collect(),
            touched: emitted.new_for.clone(),
        });
    }
    // Per-package lists from the commit bitsets (one pass, walk order
    // preserved).
    let mut per_pkg: Vec<Vec<usize>> = vec![Vec::new(); base_refs.len()];
    for (index, commit) in commits.iter().enumerate() {
        for id in commit.touched.iter() {
            if let Some(list) = per_pkg.get_mut(id.to_usize()) {
                list.push(index);
            }
        }
    }
    Ok(HistoryDigest {
        commits,
        per_pkg,
        bases: base_refs.to_vec(),
    })
}
