//! The §11.1 step-3 content machinery (the M4.2 remainder): the pure
//! content-tree builder and the remote release-branch probe behind the
//! prepare phase's
//! "`apply EditSet in memory → build tree → compare tree OID with
//! remote release branch tip (`ls-remote`, 0 REST)".
//!
//! Division of labor: the caller (the shell's `pr` executor) applies the
//! plan's EditSet in memory (the `halyard-edit` staging half of the apply
//! flow) and hands the payloads here. [`content_tree_oid`] hashes the
//! resulting tree over the checkout's HEAD — a **pure** function (objects
//! are hashed, never written; the content tree exists only as its id
//! until a committer materializes it). [`remote_release_tip`] probes the
//! remote release branch: one `git ls-remote` for the tip commit plus one
//! `git fetch` of the tip when it exists (git-side, 0 REST — I5 holds
//! for the dry paths by never calling this), then the tip's tree is read
//! in-process through `gix`.
//!
//! The tree id is the idempotence key of the content commit: identical
//! base + identical edits ⇒ an identical tree, while the commit wrapping
//! it is not (commits carry timestamps). Comparing trees — not commits —
//! is what makes the machine's re-run a no-op (I1).
//!
//! Subprocess posture (I7): like the other §11.1 effect steps (the tag
//! writer, the push, the fetch), `git` runs with argv arrays only,
//! rooted at the opened repository path, and `GIT_TERMINAL_PROMPT=0` —
//! an auth prompt would hang CI, so it fails closed instead. The
//! planning read path stays subprocess-free; these are the sanctioned
//! effect-step exceptions.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use bstr::BString;
use gix::hash::ObjectId;
use gix::objs::tree::{EntryKind, EntryMode, name_order};
use halyard_core::Oid;

use gix::objs::Find as _;

use crate::GitError;

/// The remote release branch's tip, as the §11.1 compare needs it: the
/// tip commit and its root tree. The tree is the compare key; the
/// commit is the CAS lease value for the git-side push (§11.2
/// `--force-with-lease=<expected-oid>`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteTip {
    /// The release branch's tip commit.
    pub commit: Oid,
    /// The tip commit's root tree.
    pub tree: Oid,
}

/// One node of the in-memory tree the builder mutates. File entries
/// carry the mode they had in the base tree (payload overrides keep the
/// base mode; new files are regular blobs — the [`ApiFile`] vocabulary
/// pins no mode bits, §11.4).
enum Node {
    File { mode: EntryMode, oid: ObjectId },
    Dir(BTreeMap<BString, Node>),
}

impl Node {
    fn is_dir(&self) -> bool {
        matches!(self, Self::Dir(_))
    }
}

/// Maps a gix object id onto the core hex [`Oid`].
fn core_oid(id: &gix::hash::oid) -> Result<Oid, GitError> {
    Oid::from_hex(id.to_string().as_bytes()).ok_or_else(|| GitError::Content {
        message: format!("object id {id} is not a valid hex oid"),
    })
}

/// Runs a `git` probe/effect subprocess (argv arrays only, I7) rooted at
/// the repository path, with `GIT_TERMINAL_PROMPT=0` — an auth prompt
/// would hang CI, so the effect fails closed instead.
fn run_git(cwd: &Path, args: &[&str]) -> Result<std::process::Output, GitError> {
    Command::new("git")
        .current_dir(cwd)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| GitError::Content {
            message: format!(
                "cannot run `git {}` (the content step needs the git binary): {e}",
                args.first().unwrap_or(&"")
            ),
        })
}

/// The stderr text of a finished process (lossy: diagnostics only).
fn stderr_text(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_owned()
}

/// The SHA-1 of one git object of `kind` over `body` (the loose-object
/// hash: `"<kind> <len>\0" + body`). Deterministic — this is how git
/// names objects, so a locally hashed tree equals the tree the forge's
/// Git Data API builds from the same entries.
fn object_sha1(kind: gix::objs::Kind, body: &[u8]) -> Result<ObjectId, GitError> {
    let mut hasher = gix::hash::hasher(gix::hash::Kind::Sha1);
    hasher.update(format!("{} {}\0", kind, body.len()).as_bytes());
    hasher.update(body);
    // Sha1 finalization only fails on a detected collision; the error
    // keeps the mapping total instead of a panic surface (§15.3).
    hasher.try_finalize().map_err(|e| GitError::Content {
        message: format!("sha1 finalization of the {kind} object failed: {e}"),
    })
}

/// The SHA-1 of `content` as a git blob.
fn blob_oid(content: &[u8]) -> Result<ObjectId, GitError> {
    object_sha1(gix::objs::Kind::Blob, content)
}

/// Serializes one in-memory tree bottom-up and returns its object id.
/// Entry order is git's canonical tree order (directories compare as
/// `name + "/"`), pinned by the differential tests against `git
/// write-tree`.
fn serialize_tree(node: &BTreeMap<BString, Node>) -> Result<ObjectId, GitError> {
    let mut entries: Vec<(&BString, &Node)> = node.iter().collect();
    entries.sort_by(|(a, a_node), (b, b_node)| {
        name_order(a.as_slice(), a_node.is_dir(), b.as_slice(), b_node.is_dir())
    });
    let mut body = Vec::new();
    for (name, node) in entries {
        let (mode, oid) = match node {
            Node::Dir(children) => {
                let child_oid = serialize_tree(children)?;
                (EntryMode::from(EntryKind::Tree), child_oid)
            }
            Node::File { mode, oid } => (*mode, *oid),
        };
        let mut backing = [0_u8; 6];
        body.extend_from_slice(mode.as_bytes(&mut backing));
        body.push(b' ');
        body.extend_from_slice(name);
        body.push(0);
        body.extend_from_slice(oid.as_bytes());
    }
    object_sha1(gix::objs::Kind::Tree, &body)
}

/// Loads `tree_id`'s entries into `map` (recursively; subtrees become
/// [`Node::Dir`] maps).
fn load_tree(
    repo: &gix::Repository,
    tree_id: &gix::hash::oid,
    map: &mut BTreeMap<BString, Node>,
) -> Result<(), GitError> {
    let mut buffer = Vec::new();
    let data = repo
        .try_find(tree_id, &mut buffer)
        .map_err(|e| GitError::Object {
            message: format!("cannot read tree {tree_id}: {e}"),
        })?
        .ok_or_else(|| GitError::Object {
            message: format!("tree {tree_id} is missing from the object database"),
        })?;
    if data.kind != gix::objs::Kind::Tree {
        return Err(GitError::Content {
            message: format!("object {tree_id} is a {}, not a tree", data.kind),
        });
    }
    for entry in gix::objs::TreeRefIter::from_bytes(data.data, repo.object_hash()) {
        let entry = entry.map_err(|e| GitError::Content {
            message: format!("tree {tree_id} has an unreadable entry: {e}"),
        })?;
        let name: BString = entry.filename.to_owned();
        if EntryKind::from(entry.mode) == EntryKind::Tree {
            let mut children = BTreeMap::new();
            load_tree(repo, &entry.oid, &mut children)?;
            map.insert(name, Node::Dir(children));
        } else {
            map.insert(
                name,
                Node::File {
                    mode: entry.mode,
                    oid: entry.oid.to_owned(),
                },
            );
        }
    }
    Ok(())
}

/// Inserts one payload at its slash-separated path, creating intermediate
/// directories and replacing whatever the base carried there (§11.1
/// "apply EditSet in memory": the payload is the file's full content).
fn insert_payload(
    root: &mut BTreeMap<BString, Node>,
    path: &str,
    content: &[u8],
) -> Result<(), GitError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.ends_with('/')
        || path
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(GitError::Content {
            message: format!(
                "payload path {path:?} is not a repo-relative file path (no empty, `.` or \
                 `..` components)"
            ),
        });
    }
    let mut cursor = root;
    let components: Vec<&str> = path.split('/').collect();
    for (index, component) in components.iter().enumerate() {
        let name = BString::from(component.as_bytes());
        let last = index == components.len() - 1;
        if last {
            cursor.insert(
                name,
                Node::File {
                    mode: EntryMode::from(EntryKind::Blob),
                    oid: blob_oid(content)?,
                },
            );
        } else {
            cursor = match cursor
                .entry(name)
                .or_insert_with(|| Node::Dir(BTreeMap::new()))
            {
                Node::Dir(children) => children,
                Node::File { .. } => {
                    return Err(GitError::Content {
                        message: format!(
                            "payload path {path:?} walks through a file entry the base tree \
                             carries — the plan's edit and the checkout disagree; re-plan"
                        ),
                    });
                }
            };
        }
    }
    Ok(())
}

/// Builds the git tree OID for `files` applied over `root`'s HEAD (§11.1
/// step 3 "apply EditSet in memory → build tree"): every payload path is
/// replaced with its content; every other path stays exactly as HEAD
/// carries it. **Pure**: objects are hashed, never written — the content
/// tree exists only as its id until a committer materializes it, and the
/// caller's working tree is untouched.
///
/// An unborn HEAD (an empty repository) builds the tree from the
/// payloads alone. The id is deterministic: identical base + identical
/// payloads ⇒ an identical tree OID (the machine's idempotence key, I1).
///
/// # Errors
///
/// [`GitError::Open`] when the repository cannot be opened,
/// [`GitError::Object`] when HEAD's tree cannot be read, and
/// [`GitError::Content`] for malformed payload paths.
pub fn content_tree_oid(root: &Path, files: &[(String, Vec<u8>)]) -> Result<Oid, GitError> {
    let repo = open_for_content(root)?;
    let tree = assemble_tree(&repo, files)?;
    let oid = serialize_tree(&tree)?;
    core_oid(&oid)
}

/// Opens the repository for the content machinery (the shared open +
/// empty-payload gate).
fn open_for_content(root: &Path) -> Result<gix::Repository, GitError> {
    let repo = gix::open(root).map_err(|e| GitError::Open {
        path: root.display().to_string(),
        message: e.to_string(),
    })?;
    Ok(repo)
}

/// Assembles the in-memory tree the content step operates on: HEAD's
/// base tree (or the empty tree on an unborn HEAD — a fresh scaffold has
/// no commits yet, the payloads alone form the content tree) with every
/// payload path replaced by its content.
fn assemble_tree(
    repo: &gix::Repository,
    files: &[(String, Vec<u8>)],
) -> Result<BTreeMap<BString, Node>, GitError> {
    if files.is_empty() {
        return Err(GitError::Content {
            message: "no payloads to build a tree from — the caller passes no content step \
                      for an empty plan, never an empty payload list"
                .to_owned(),
        });
    }
    let mut tree = BTreeMap::new();
    let head = repo.head().map_err(|e| GitError::Open {
        path: repo.path().display().to_string(),
        message: e.to_string(),
    })?;
    match head.try_into_peeled_id() {
        Err(error) => {
            return Err(GitError::Object {
                message: format!("cannot peel HEAD: {error}"),
            });
        }
        // Unborn HEAD: the documented empty-base case, not an error.
        Ok(None) => {}
        Ok(Some(commit_id)) => {
            let commit = repo.find_commit(commit_id).map_err(|e| GitError::Object {
                message: format!("cannot read HEAD commit {commit_id}: {e}"),
            })?;
            let base_tree = commit
                .tree_id()
                .map_err(|e| GitError::Object {
                    message: format!("cannot resolve HEAD's tree: {e}"),
                })?
                .detach();
            load_tree(repo, &base_tree, &mut tree)?;
        }
    }
    // Apply the payloads over the base (sorted for a deterministic
    // traversal, I3).
    let mut payloads: Vec<&(String, Vec<u8>)> = files.iter().collect();
    payloads.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, content) in payloads {
        insert_payload(&mut tree, path, content)?;
    }
    Ok(tree)
}

/// Probes the remote release branch tip (§11.1 step 3 "compare tree OID
/// with remote release branch tip (`ls-remote`, 0 REST"): one
/// `git ls-remote <remote> refs/heads/<branch>` for the tip commit, one
/// `git fetch <remote> <branch>` when the branch exists (git-side, so
/// the dry paths never spend it), then the tip commit's tree is read
/// in-process through `gix`.
///
/// An unconfigured remote yields `Ok(None)` (the honest no-op — the same
/// convention the push effect applies: a repository without a remote has
/// nothing to compare against; the caller decides what that means). A
/// branch that does not exist remotely yields `Ok(None)` — the release
/// branch the first prepare pass would create.
///
/// # Errors
///
/// [`GitError::Content`] when a probe subprocess fails or the tip
/// commit cannot be read, [`GitError::Open`] when the repository cannot
/// be opened.
pub fn remote_release_tip(
    root: &Path,
    remote: &str,
    branch: &str,
) -> Result<Option<RemoteTip>, GitError> {
    let repo = gix::open(root).map_err(|e| GitError::Open {
        path: root.display().to_string(),
        message: e.to_string(),
    })?;
    let configured = repo
        .config_snapshot()
        .string(format!("remote.{remote}.url").as_str());
    if configured.is_none() {
        return Ok(None);
    }
    // 1. The tip commit (one git-side round trip, 0 REST).
    let probe = run_git(
        root,
        &["ls-remote", remote, &format!("refs/heads/{branch}")],
    )?;
    if !probe.status.success() {
        return Err(GitError::Content {
            message: format!(
                "cannot probe the release branch on remote {remote:?} (git ls-remote \
                 failed): {}",
                stderr_text(&probe)
            ),
        });
    }
    let out = String::from_utf8_lossy(&probe.stdout);
    let tip_hex = out.lines().find_map(|line| {
        let (oid, reference) = line.split_once('\t')?;
        (reference == format!("refs/heads/{branch}")).then(|| oid.to_owned())
    });
    let Some(tip_hex) = tip_hex else {
        // No row ⇒ the branch does not exist remotely yet: a value, never
        // an error (§11.4's not-found-is-a-value convention).
        return Ok(None);
    };
    let tip_oid = ObjectId::from_hex(tip_hex.as_bytes()).map_err(|_| GitError::Content {
        message: format!("the release branch probe returned a non-object sha: {tip_hex:.200}"),
    })?;
    // 2. The tip's objects, when not already local (one git-side fetch;
    //    idempotent — an up-to-date fetch is a cheap no-op).
    let fetch = run_git(root, &["fetch", remote, branch])?;
    if !fetch.status.success() {
        return Err(GitError::Content {
            message: format!(
                "cannot fetch the release branch tip from remote {remote:?} (git fetch \
                 failed): {}",
                stderr_text(&fetch)
            ),
        });
    }
    // 3. The tip commit's tree, read in-process (no subprocess).
    let commit = repo.find_commit(tip_oid).map_err(|e| GitError::Object {
        message: format!(
            "cannot read the release branch tip {tip_oid} (the fetch above should have \
                 delivered it): {e}"
        ),
    })?;
    let tree = commit
        .tree_id()
        .map_err(|e| GitError::Object {
            message: format!("cannot resolve the tip commit's tree: {e}"),
        })?
        .detach();
    Ok(Some(RemoteTip {
        commit: core_oid(&tip_oid)?,
        tree: core_oid(&tree)?,
    }))
}

// ---------------------------------------------------------------------------
// The §11.1 step-3 git-side half (the M4.2 remainder, §11.2): the content
// commit materialized in the local odb and the release branch pushed with
// the CAS lease. `commit_mode = "git"`: the plan's edits commit locally
// on top of the base snapshot and push with
// `--force-with-lease=<expected-oid>` — two racers cannot corrupt the
// release branch (§11.2), and the branch is halyard-owned scratch (the
// push replaces its history deliberately).
//
// These run only in the live `git` mode: a dry forecast forecasts the
// machine's content step over the preview forge (I5) and never reaches
// this half.
// ---------------------------------------------------------------------------

/// Writes the in-memory tree's blobs and subtrees into the repository's
/// object database, bottom-up, returning the root tree's id. The ids are
/// the pure hashes' ids (same serialization) — the materialization of
/// what [`content_tree_oid`] previews.
fn write_tree_objects(
    repo: &gix::Repository,
    node: &BTreeMap<BString, Node>,
) -> Result<ObjectId, GitError> {
    let mut entries: Vec<gix::objs::tree::Entry> = Vec::new();
    for (name, child) in node {
        let (mode, oid) = match child {
            Node::Dir(children) => {
                let child_oid = write_tree_objects(repo, children)?;
                (EntryMode::from(EntryKind::Tree), child_oid)
            }
            Node::File { mode, oid } => (*mode, *oid),
        };
        entries.push(gix::objs::tree::Entry {
            mode,
            filename: name.clone(),
            oid,
        });
    }
    // git's canonical tree order is the serialization's order too; the
    // in-memory map inserts payloads sorted, but the entry list here is
    // the map's own (byte-name) order — re-sort with git's rule before
    // writing.
    entries.sort_by(|a, b| {
        name_order(
            a.filename.as_slice(),
            EntryKind::from(a.mode) == EntryKind::Tree,
            b.filename.as_slice(),
            EntryKind::from(b.mode) == EntryKind::Tree,
        )
    });
    repo.write_object(gix::objs::Tree { entries })
        .map_err(|e| GitError::Content {
            message: format!("cannot write the content tree objects: {e}"),
        })
        .map(gix::Id::detach)
}

/// Writes the content commit into the local object database (§11.1 step
/// 3 "else commit", the git side): the payloads' blobs and trees land
/// first, then one commit on top of the base snapshot — the parent is
/// HEAD (the snapshot the plan was computed from, so the Release PR's
/// diff is exactly the version edits) or none on an unborn HEAD. The
/// identity is the shell-owned git config + clock pair (§11.1 step 2's
/// rule), carried by [`TaggerIdentity`].
///
/// Nothing moves a ref: the commit exists locally only until
/// [`push_release_branch`] moves the release branch under the CAS lease.
///
/// # Errors
///
/// [`GitError::Content`] for malformed payloads or failed object
/// writes, [`GitError::Object`] for unreadable HEAD, [`GitError::Open`]
/// when the repository cannot be opened.
pub fn stage_release_commit(
    root: &Path,
    message: &str,
    files: &[(String, Vec<u8>)],
    identity: &crate::tag::TaggerIdentity,
) -> Result<Oid, GitError> {
    if identity.name.trim().is_empty() || identity.email.trim().is_empty() {
        return Err(GitError::Content {
            message: "committer identity is incomplete; configure user.name/user.email \
                      or pass an explicit identity"
                .to_owned(),
        });
    }
    let repo = open_for_content(root)?;
    let tree_map = assemble_tree(&repo, files)?;
    // The payloads' blobs: the in-memory tree carries only their hashes,
    // so every payload blob is written before the trees reference it.
    for (path, content) in files {
        let oid = blob_oid(content)?;
        if !repo.has_object(oid) {
            repo.write_object(gix::objs::Blob {
                data: content.clone(),
            })
            .map_err(|e| GitError::Content {
                message: format!("cannot write the blob for {path:?}: {e}"),
            })?;
        }
    }
    let tree = write_tree_objects(&repo, &tree_map)?;
    // The parent: HEAD (the base snapshot), absent on an unborn HEAD.
    let head = repo.head().map_err(|e| GitError::Open {
        path: repo.path().display().to_string(),
        message: e.to_string(),
    })?;
    let parent = head
        .try_into_peeled_id()
        .map_err(|e| GitError::Object {
            message: format!("cannot peel HEAD: {e}"),
        })?
        .map(gix::Id::detach);
    let signature = identity.signature();
    let commit = repo
        .write_object(gix::objs::Commit {
            tree,
            parents: parent.into_iter().collect(),
            author: signature.clone(),
            committer: signature,
            encoding: None,
            message: message.to_owned().into(),
            extra_headers: Vec::new(),
        })
        .map_err(|e| GitError::Content {
            message: format!("cannot write the content commit: {e}"),
        })?
        .detach();
    core_oid(&commit)
}

/// One branch-push outcome (the porcelain row's classification). One
/// push, one ref — the report carries the single row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchPushRow {
    /// The remote accepted the ref (a new branch or an advanced tip).
    Pushed {
        /// The branch's short name.
        branch: String,
    },
    /// The remote already carried this exact commit (`=` row) — nothing
    /// was sent (I1).
    AlreadyOnRemote {
        /// The branch's short name.
        branch: String,
    },
    /// The CAS lease rejected the push (`!` row): the release branch
    /// moved under us (§11.2). Fail-closed — the caller re-probes and
    /// re-runs; the push is never forced past the lease.
    Rejected {
        /// The branch's short name.
        branch: String,
        /// The rejection text git reported.
        summary: String,
    },
}

/// The branch-push report (§11.2): the remote the push targeted (the
/// honest no-op when unconfigured) and the one outcome row.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct BranchPushReport {
    /// The remote the push targeted. `None` when no
    /// `remote.<name>.url` is configured.
    pub remote: Option<String>,
    /// The push outcome, when a push was attempted.
    pub row: Option<BranchPushRow>,
}

/// Pushes `commit_oid` onto `remote`'s `branch` under the §11.2 CAS: one
/// `git push --porcelain --force-with-lease=refs/heads/<branch>:<expected>
/// <remote> <commit>:refs/heads/<branch>` — the expected oid is the
/// probed tip, and an absent branch pushes with the create-only lease
/// (the empty expectation asserts the branch does not exist). Never
/// forced past the lease: a rejection surfaces as the
/// [`BranchPushRow::Rejected`] row the caller treats fail-closed.
///
/// An unconfigured remote yields the honest no-op report
/// ([`BranchPushReport::remote`] `None`) — the same convention the tag
/// push applies.
///
/// # Errors
///
/// [`GitError::Content`] when the push subprocess cannot run or produces
/// no parseable status, [`GitError::Open`] when the repository cannot be
/// opened.
pub fn push_release_branch(
    root: &Path,
    remote: &str,
    branch: &str,
    commit_oid: &Oid,
    expected_tip: Option<&Oid>,
) -> Result<BranchPushReport, GitError> {
    let repo = gix::open(root).map_err(|e| GitError::Open {
        path: root.display().to_string(),
        message: e.to_string(),
    })?;
    if repo
        .config_snapshot()
        .string(format!("remote.{remote}.url").as_str())
        .is_none()
    {
        return Ok(BranchPushReport::default());
    }
    let expected = expected_tip.map_or_else(String::new, |tip| tip.as_str().to_owned());
    let refspec = format!("{commit_oid}:refs/heads/{branch}");
    let lease = format!("--force-with-lease=refs/heads/{branch}:{expected}");
    let pushed = run_git(root, &["push", "--porcelain", &lease, remote, &refspec])?;
    let statuses = crate::push::parse_porcelain_ref_push(
        &String::from_utf8_lossy(&pushed.stdout),
        "refs/heads/",
    );
    if !pushed.status.success() && statuses.is_empty() {
        return Err(GitError::Content {
            message: format!(
                "cannot push the release branch to {remote:?} (git push failed with no \
                 parseable status): {}",
                stderr_text(&pushed)
            ),
        });
    }
    let row = match statuses.first() {
        Some(status) => match status.flag {
            '=' => BranchPushRow::AlreadyOnRemote {
                branch: branch.to_owned(),
            },
            '!' => BranchPushRow::Rejected {
                branch: branch.to_owned(),
                summary: status.summary.clone(),
            },
            _ => BranchPushRow::Pushed {
                branch: branch.to_owned(),
            },
        },
        None => {
            return Err(GitError::Content {
                message: format!(
                    "git push produced no status for the release branch: {}",
                    stderr_text(&pushed)
                ),
            });
        }
    };
    Ok(BranchPushReport {
        remote: Some(remote.to_owned()),
        row: Some(row),
    })
}
