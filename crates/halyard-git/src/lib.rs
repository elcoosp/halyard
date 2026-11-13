//! The git engine (M2.1–M2.5, §8): ref enumeration + base resolution
//! ([`refs`]), the painter walk ([`walk`]), the trie-guided tree diff
//! ([`tree_diff`], §8.4), the merge policy ([`merge`], §8.5) and the
//! persistent cache wiring (§8.7), assembled into a [`GitHistory`] that
//! satisfies the planner's [`HistoryProvider`](halyard_core::HistoryProvider)
//! seam.
//!
//! This crate is imperative-shell territory: it owns `gix` access, ODB
//! reads and shallow-clone detection. It performs **no subprocess** (no
//! `git` binary, invariant I6) and no network (I5). The planner consumes
//! only the materialized [`HistoryDigest`](halyard_core::HistoryDigest);
//! the cache is consulted *before* the tree diff and never leaks into
//! `halyard-core` (planner purity, §4).
//!
//! Layer note (ADR-0009): this crate depends on `halyard-core` **and**
//! `halyard-conventional` — the walk emits parsed commits (§5.7), and the
//! parser is a zero-dependency leaf, so the edge stays cycle-free. Slice
//! D adds `halyard-cache` (the cache feeds the walk; `git → cache →
//! core`, no cycle).

#![forbid(unsafe_code)]

pub mod bloom;
pub mod merge;
pub mod refs;
pub mod tree_diff;
pub mod walk;

// Re-export for downstream differential tests and integrators that need
// the exact `gix` version this engine was compiled against.
pub use gix;

use std::path::Path;

use gix::hash::ObjectId;
use halyard_core::history::{AuthorRef, BaseRef, HistoryDigest, RelevantCommit};
use bstr::ByteSlice as _;
use halyard_core::trie::build_trie;
use halyard_core::{BString, HistoryProvider, Oid, SmallBitset};
use halyard_cache::{CacheKey, CacheRecord};

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

/// Options for [`GitHistory::build_with_options`] (additive slice-D
/// seam; the frozen [`GitHistory::build`] signature keeps working with
/// the defaults).
#[derive(Debug, Default)]
pub struct GitHistoryOptions {
    /// Walk bound for packages with no release base (§8.3).
    pub initial_max_commits: u32,
    /// Merge-commit handling (§8.5, M2.4). Default `FirstParentDiff`
    /// (§16.4 `merge_policy = "first-parent"`).
    pub merge_policy: merge::MergePolicy,
    /// The persistent per-commit cache (§8.7, M2.5); `None` = uncached.
    /// Consulted before the tree diff; hits must produce byte-identical
    /// digests (determinism test).
    pub cache: Option<halyard_cache::Cache>,
    /// Trie layout hash (`halyard_core::layout_hash`) — a cache-key
    /// component (§8.7).
    pub layout_hash: String,
    /// Config hash (`halyard-config`) — a cache-key component (§8.7).
    pub config_hash: String,
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
    /// Tree-diff counters of the last build (§17 M2.3 acceptance
    /// metric: entries read vs a naive diff).
    pub tree_diff_stats: tree_diff::TreeDiffStats,
    /// Merge-policy counters of the last build (§8.5).
    pub merge_stats: merge::MergeStats,
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
    /// paints the unreleased history (§8.3) with the default options
    /// (frozen Slice C signature; see [`GitHistory::build_with_options`]).
    ///
    /// # Errors
    ///
    /// [`GitError`] for open/head/shallow/ref/object failures.
    pub fn build(
        path: &Path,
        ws: &halyard_core::Workspace,
        initial_max_commits: u32,
    ) -> Result<Self, GitError> {
        Self::build_with_options(
            path,
            ws,
            GitHistoryOptions {
                initial_max_commits,
                ..GitHistoryOptions::default()
            },
        )
    }

    /// The full slice-D pipeline: open → bases → paint → **tree diff
    /// (§8.4)** → **merge policy (§8.5)** → digest, with the persistent
    /// cache (§8.7) consulted before every tree diff when `options.cache`
    /// is set.
    ///
    /// # Errors
    ///
    /// [`GitError`] for open/head/shallow/ref/object failures,
    /// [`GitError::Object`] for unreadable trees.
    pub fn build_with_options(
        path: &Path,
        ws: &halyard_core::Workspace,
        options: GitHistoryOptions,
    ) -> Result<Self, GitError> {
        let repo = gix::open(path).map_err(|e| GitError::Open {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        let repo_path = path.display().to_string();
        if is_shallow(&repo) {
            return Err(GitError::Shallow { path: repo_path });
        }
        let head = repo.head_id().ok().map(|id| id.detach());
        let Some(head) = head else {
            return Err(GitError::Head { path: repo_path });
        };
        let mut generations = refs::Generations::new(&repo);
        let (base_refs, _tags) = refs::resolve_bases(&repo, head, ws, &mut generations)?;
        let base_oids: Vec<Option<ObjectId>> = base_refs
            .iter()
            .map(|base| base.as_ref().and_then(|b| ObjectId::from_hex(b.oid.as_bytes()).ok()))
            .collect();
        let painted = walk::paint(&repo, head, &base_oids, options.initial_max_commits, &mut generations)?;
        // The persistent cache travels separately from the frozen options
        // shape: assemble needs `&mut` for inserts while `options` stays
        // shared.
        let mut cache = options.cache;

        // The §8.4 trie: package roots + shared paths; filters are
        // package-id aligned for the include/exclude refinement.
        let trie = build_trie(ws);
        let roots: Vec<BString> = ws
            .packages()
            .iter()
            .map(|p| BString::from(p.root.as_str()))
            .collect();
        let filters: Vec<halyard_core::PathFilter> =
            ws.packages().iter().map(|p| p.include.clone()).collect();
        let mut differ = tree_diff::TreeDiffer::new(&repo, trie, roots, filters);

        let (digest, merge_stats) = assemble(
            &repo,
            &painted,
            &base_refs,
            &mut differ,
            options.merge_policy,
            &options.layout_hash,
            &options.config_hash,
            &mut cache,
            &mut generations,
        )?;
        let tree_diff_stats = *differ.stats();
        // Durability point for the cache: appends become a clean,
        // checksum-verified file state (§8.7; kill-safety means a crash
        // before this point loses at most the tail record).
        if let Some(cache) = cache.as_mut() {
            cache.flush().map_err(|e| GitError::Object {
                message: format!("cache flush failed: {e}"),
            })?;
        }
        Ok(Self {
            head: Some(core_oid_of(head)),
            digest,
            degraded: painted.degraded,
            truncated: painted.truncated,
            tree_diff_stats,
            merge_stats,
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
    repo.common_dir().join("shallow").is_file()
}

/// Maps a gix oid onto the core hex-carrier (total: gix oids are hex).
fn core_oid_of(oid: ObjectId) -> Oid {
    let hex = oid.to_string();
    Oid::from_hex(hex.as_bytes()).unwrap_or_else(|| {
        Oid::from_hex(b"0000000000000000000000000000000000000000")
            .unwrap_or_else(|| Oid::from_hex(b"1111111111111111111111111111111111111111").expect("static hex is valid"))
    })
}

/// The tree oid of one commit.
fn commit_tree_id(repo: &gix::Repository, oid: ObjectId) -> Result<ObjectId, GitError> {
    let commit = repo
        .find_commit(oid)
        .map_err(|e| map_git_err("E-GIT-OBJECT", format!("cannot read commit {oid}: {e}")))?;
    let tree = commit
        .tree_id()
        .map_err(|e| map_git_err("E-GIT-OBJECT", format!("cannot read tree of {oid}: {e}")))?;
    Ok(tree.detach())
}

/// Element-wise intersection of two package bitsets (the tree-diff
/// attribution ∩ the painted window). Iteration order is ascending ids,
/// so the result never depends on bitset internals (I3).
fn intersect(a: &SmallBitset, b: &SmallBitset) -> SmallBitset {
    let mut out = SmallBitset::new();
    for pkg in a.iter() {
        if b.get(pkg) {
            out.set(pkg);
        }
    }
    out
}

/// Assembles the [`HistoryDigest`] from the painted walk (§5.7): every
/// emitted commit is read once, its message parsed through
/// `halyard-conventional`, gated by the merge policy (§8.5), attributed
/// through the trie-guided tree diff (§8.4, with the §8.7 cache consulted
/// first), and per-package attribution intersected with the painted
/// window. Commits that touch no package path leave the digest entirely.
///
/// # Errors
///
/// [`GitError::Object`] when a commit or tree cannot be read.
#[allow(clippy::too_many_lines)]
fn assemble(
    repo: &gix::Repository,
    painted: &walk::PaintResult,
    base_refs: &[Option<BaseRef>],
    differ: &mut tree_diff::TreeDiffer<'_>,
    policy: merge::MergePolicy,
    layout_hash: &str,
    config_hash: &str,
    cache: &mut Option<halyard_cache::Cache>,
    generations: &mut refs::Generations<'_>,
) -> Result<(HistoryDigest, merge::MergeStats), GitError> {
    // Parsed, policy-gated candidates plus the per-candidate extras (the
    // merge policy may remove entries; `origin` links a kept candidate
    // back to its extras slot).
    let mut candidates: Vec<merge::Candidate> = Vec::new();
    struct Extra {
        subject_end: u32,
        body_start: u32,
        author: AuthorRef,
        new_for: SmallBitset,
    }
    let mut extras: Vec<Extra> = Vec::new();

    for emitted in &painted.commits {
        let commit = repo
            .find_commit(emitted.oid)
            .map_err(|e| {
                let oid = emitted.oid;
                map_git_err("E-GIT-OBJECT", format!("cannot read commit {oid}: {e}"))
            })?;
        let raw_message = commit.message_raw().map_err(|e| {
            let oid = emitted.oid;
            map_git_err("E-GIT-OBJECT", format!("cannot read message of {oid}: {e}"))
        })?;
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
            map_git_err("E-GIT-OBJECT", format!("cannot read author of {oid}: {e}"))
        })?;
        let origin = candidates.len();
        candidates.push(merge::Candidate {
            origin,
            oid: BString::from(core_oid_of(emitted.oid).as_bytes()),
            parsed,
            message: BString::from(raw_message),
            is_merge: commit.parent_ids().skip(1).next().is_some(),
        });
        extras.push(Extra {
            subject_end: u32::try_from(subject_end).unwrap_or(u32::MAX),
            body_start: u32::try_from(body_start).unwrap_or(0),
            author: AuthorRef {
                name: BString::from(author.name),
                email: BString::from(author.email),
            },
            new_for: emitted.new_for.clone(),
        });
    }

    let (kept, merge_stats) = merge::apply_policy(candidates, policy);
    let mut commits: Vec<RelevantCommit> = Vec::new();
    for candidate in &kept {
        let extra = extras.get(candidate.origin).ok_or_else(|| {
            map_git_err(
                "E-GIT-OBJECT",
                format!("walk bookkeeping lost commit {}", candidate.oid),
            )
        })?;
        // §8.7 (M2.5): the cache is consulted BEFORE the tree diff; a hit
        // skips the tree comparison entirely and must produce the same
        // attribution the diff would (determinism test asserts this).
        let cache_key = CacheKey::compute(candidate.oid.as_bytes(), layout_hash, config_hash);
        let cached = cache.as_ref().and_then(|c| c.get(&cache_key));
        let attributed = match cached {
            Some(record) => intersect(&record.touched, &extra.new_for),
            None => {
                // Tree oid of the commit and its first parent (§8.4:
                // first-parent diffing).
                let oid = ObjectId::from_hex(candidate.oid.as_bytes())
                    .map_err(|e| map_git_err("E-GIT-OBJECT", format!("bad oid hex: {e}")))?;
                let tree_c = commit_tree_id(repo, oid)?;
                let first_parent = repo
                    .find_commit(oid)
                    .ok()
                    .and_then(|c| c.parent_ids().next())
                    .map(gix::Id::detach);
                let tree_p = match first_parent {
                    Some(parent) => Some(commit_tree_id(repo, parent)?),
                    None => None,
                };
                let position = generations.position(oid);
                let filtered = differ.diff_commit_at(tree_c, tree_p, position)?;
                if let Some(cache) = cache.as_mut() {
                    cache
                        .insert(
                            cache_key,
                            CacheRecord {
                                touched: filtered.clone(),
                                subject_end: extra.subject_end,
                                body_start: extra.body_start,
                            },
                        )
                        .map_err(|e| {
                            map_git_err("E-GIT-OBJECT", format!("cache insert failed: {e}"))
                        })?;
                }
                intersect(&filtered, &extra.new_for)
            }
        };
        // §8.4: a commit that touches no package path (e.g. docs-only in
        // a multi-package repo) carries no attribution and leaves the
        // digest entirely.
        if attributed.is_empty() {
            continue;
        }
        let oid = Oid::from_hex(candidate.oid.as_bytes())
            .unwrap_or_else(|| Oid::from_hex(b"0000000000000000000000000000000000000000").expect("static hex is valid"));
        commits.push(RelevantCommit {
            oid,
            subject: BString::from(
                candidate
                    .message
                    .get(..extra.subject_end as usize)
                    .unwrap_or_default(),
            ),
            body_range: extra.body_start
                ..u32::try_from(candidate.message.len()).unwrap_or(u32::MAX),
            parsed: halyard_core::history::ParsedCommit {
                kind: candidate.parsed.kind.clone(),
                scope: candidate.parsed.scope.clone(),
                breaking: candidate.parsed.breaking,
                description: candidate.parsed.description.clone(),
            },
            pr: candidate.parsed.pr,
            author: extra.author.clone(),
            trailers: candidate
                .parsed
                .footers
                .iter()
                .map(|t| halyard_core::history::Trailer {
                    key: t.key.clone(),
                    value: t.value.clone(),
                })
                .collect(),
            touched: attributed,
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
    Ok((
        HistoryDigest {
            commits,
            per_pkg,
            bases: base_refs.to_vec(),
        },
        merge_stats,
    ))
}
