//! Ref enumeration, tag-template matching and base resolution (§8.2,
//! M2.1).
//!
//! The ground-truth rule (§8.2): for each package, the **highest-version**
//! tag whose (peeled) commit is reachable from HEAD wins. Generation
//! numbers make the ancestor test `O(1)`-rejectable when a commit-graph is
//! present; without one, generations are computed by a memoized DFS
//! (exact, cached, shared with the walk).
//!
//! Determinism (I3): tag enumeration sorts by name, candidate selection
//! sorts by `(version desc, tag name asc)`, so neither packed-refs layout
//! nor scan order can change a resolved base.

use std::collections::{HashMap, HashSet};

use gix::hash::ObjectId;
use gix::reference::Category;
use halyard_core::BString;
use halyard_core::Oid;
use halyard_core::history::BaseRef;
use halyard_core::package::Workspace;

use crate::{GitError, map_git_err};

/// One tag candidate: name, peeled commit and (per package) the parsed
/// version (§8.2 step 3: invalid versions are ignored, not fatal).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagCandidate {
    /// Short tag name (`core-v0.4.1`, scope-stripped of `refs/tags/`).
    pub name: BString,
    /// The peeled commit the tag points at (annotated tags peel once,
    /// §8.2 step 5).
    pub peeled: ObjectId,
    /// The version extracted through a package's template, when the tag
    /// matches it (`None` during raw enumeration).
    pub version: Option<semver::Version>,
}

/// Maps a [`gix::hash::ObjectId`] onto the core [`Oid`] (hex-carrier).
/// gix oids are always valid hex, so the conversion is total.
fn core_oid(oid: ObjectId) -> Oid {
    let hex = oid.to_string();
    Oid::from_hex(hex.as_bytes()).unwrap_or_else(|| {
        // Unreachable for gix-produced oids; the zero-oid fallback keeps
        // the function total without panicking (§15.3). The static hex
        // literals are 40-char valid hex — validated by this module's
        // tests — so the final arm cannot be reached either.
        Oid::from_hex(b"0000000000000000000000000000000000000000")
            .or_else(|| Oid::from_hex(b"1111111111111111111111111111111111111111"))
            .unwrap_or_else(|| unreachable!("static fallback hex is valid"))
    })
}

/// Enumerates `refs/tags/*` once (packed + loose), peeling annotated tags
/// (§8.2 steps 1/5). Invalid or duplicate names are skipped/rejected by
/// the dedup below; unmatched tags simply carry no version per package.
///
/// # Errors
///
/// [`GitError::RefScan`] when the reference database cannot be read,
/// [`GitError::Object`] when a tag object cannot be peeled.
pub fn enumerate(repo: &gix::Repository) -> Result<Vec<TagCandidate>, GitError> {
    let mut out: Vec<TagCandidate> = Vec::new();
    let platform = repo
        .references()
        .map_err(|e| map_git_err("E-GIT-REF", format!("cannot enumerate references: {e}")))?;
    let iter = platform
        .tags()
        .map_err(|e| map_git_err("E-GIT-REF", format!("cannot scan tags: {e}")))?;
    for reference in iter {
        let reference = reference
            .map_err(|e| map_git_err("E-GIT-REF", format!("unreadable reference: {e}")))?;
        let Some((Category::Tag, short)) = reference.name().category_and_short_name() else {
            continue;
        };
        let name = BString::from(short);
        let peeled = reference
            .into_fully_peeled_id()
            .map_err(|e| map_git_err("E-GIT-OBJECT", format!("cannot peel tag {name:?}: {e}")))?;
        out.push(TagCandidate {
            name,
            peeled: peeled.detach(),
            version: None,
        });
    }
    // Deterministic order regardless of packed-refs/loose layout (I3).
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.dedup_by(|a, b| a.name == b.name && a.peeled == b.peeled);
    Ok(out)
}

/// Resolves the release base of every package (§8.2): the highest-version
/// matching tag whose peeled commit is an ancestor of HEAD. Returns the
/// refs aligned with `ws.packages()` (`None` = initial release) plus the
/// enumerated tags (for diagnostics).
///
/// # Errors
///
/// [`GitError::Head`] when HEAD cannot be resolved, [`GitError::Object`]
/// when commits or generations cannot be read.
pub fn resolve_bases(
    repo: &gix::Repository,
    head: ObjectId,
    ws: &Workspace,
    generations: &mut Generations<'_>,
) -> Result<(Vec<Option<BaseRef>>, Vec<TagCandidate>), GitError> {
    let tags = enumerate(repo)?;
    let head_gen = generations.of(head)?;
    let mut bases: Vec<Option<BaseRef>> = Vec::with_capacity(ws.packages().len());
    for pkg in ws.packages() {
        let name = ws.interner().get(pkg.name).unwrap_or_default().to_owned();
        let eco = pkg.ecosystem.resolve(ws.interner());
        // One pass over the tags for this package's template (§8.2 step 2).
        let mut candidates: Vec<(semver::Version, &TagCandidate)> = Vec::new();
        for tag in &tags {
            let tag_name = std::str::from_utf8(&tag.name).unwrap_or_default();
            if let Some(version) = pkg.tag_template.extract_version(tag_name, &name, eco) {
                candidates.push((version, tag));
            }
        }
        // Highest version first; equal versions tie-break by tag name so
        // the outcome never depends on enumeration order (I3).
        candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
        let mut chosen: Option<&TagCandidate> = None;
        for (_, tag) in &candidates {
            let tag_gen = generations.of(tag.peeled)?;
            // O(1) rejection: a commit strictly above HEAD cannot be its
            // ancestor (§8.2 step 4). Equal generations can still be
            // unrelated, so they fall through to the real test.
            if tag_gen > head_gen {
                continue;
            }
            if is_ancestor(repo, tag.peeled, head, head_gen, generations)? {
                chosen = Some(tag);
                break;
            }
        }
        bases.push(chosen.map(|tag| BaseRef {
            tag: tag.name.clone(),
            oid: core_oid(tag.peeled),
        }));
    }
    Ok((bases, tags))
}

/// True when `candidate` is an ancestor of (or equal to) `head`.
///
/// # Errors
///
/// [`GitError::Object`] when commits cannot be read.
fn is_ancestor(
    repo: &gix::Repository,
    candidate: ObjectId,
    head: ObjectId,
    head_gen: u32,
    generations: &mut Generations<'_>,
) -> Result<bool, GitError> {
    if candidate == head {
        return Ok(true);
    }
    let candidate_gen = generations.of(candidate)?;
    if candidate_gen > head_gen {
        return Ok(false);
    }
    // Bounded DFS from `head` down to `candidate_gen`: commits strictly
    // below the candidate's generation cannot reach it, so the walk stops
    // there (the same O(1)-style pruning §8.2 describes).
    let mut seen: HashSet<ObjectId> = HashSet::new();
    let mut stack: Vec<ObjectId> = vec![head];
    while let Some(oid) = stack.pop() {
        if oid == candidate {
            return Ok(true);
        }
        if generations.of(oid)? < candidate_gen || !seen.insert(oid) {
            continue;
        }
        let commit = repo
            .find_commit(oid)
            .map_err(|e| map_git_err("E-GIT-OBJECT", format!("cannot read commit {oid}: {e}")))?;
        for parent in commit.parent_ids() {
            let parent = parent.detach();
            if generations.of(parent)? >= candidate_gen {
                stack.push(parent);
            }
        }
    }
    Ok(false)
}

/// Memoized commit generations (§8.2/§8.3): commit-graph lookups (v1/v2
/// files are both handled by gix) when the repository carries one, exact
/// memoized DFS otherwise. The cache is shared with the painter walk, so
/// generations are computed at most once per commit per process.
pub struct Generations<'r> {
    repo: &'r gix::Repository,
    graph: Option<gix_commitgraph::Graph>,
    cache: HashMap<ObjectId, u32>,
    /// Committer timestamps cached alongside generations (the walk's heap
    /// key needs both; the DFS fills this during its single read per
    /// commit).
    times: HashMap<ObjectId, i64>,
}

impl<'r> Generations<'r> {
    /// Opens the commit-graph when present (`None` = the DFS path, which
    /// `doctor` and the walk's degraded marker surface).
    pub fn new(repo: &'r gix::Repository) -> Self {
        let graph = repo.commit_graph_if_enabled().ok().flatten();
        Self {
            repo,
            graph,
            cache: HashMap::new(),
            times: HashMap::new(),
        }
    }

    /// True when the repository has a usable commit-graph (§8.3).
    pub fn has_graph(&self) -> bool {
        self.graph.is_some()
    }

    /// The commit's position in the commit-graph file (`None` without a
    /// graph or for commits outside it) — the index the changed-path
    /// Bloom filters (§8.4 step 4) are keyed by.
    pub fn position(&self, oid: ObjectId) -> Option<usize> {
        self.graph
            .as_ref()?
            .lookup(oid)
            .map(|pos| usize::try_from(pos.0).unwrap_or(usize::MAX))
    }

    /// Generation **and** committer timestamp in one lookup: the
    /// commit-graph stores both (§8.3 heap key without any object read);
    /// the DFS path falls back to an ODB read for the time.
    ///
    /// # Errors
    ///
    /// [`GitError::Object`] when the commit cannot be read.
    pub fn of_with_time(&mut self, oid: ObjectId) -> Result<(u32, i64), GitError> {
        if let Some(graph) = &self.graph
            && let Some(position) = graph.lookup(oid)
        {
            let entry = graph.commit_at(position);
            let generation = entry.generation();
            self.cache.insert(oid, generation);
            let time = i64::try_from(entry.committer_timestamp()).unwrap_or(i64::MAX);
            return Ok((generation, time));
        }
        let generation = self.of(oid)?;
        // The DFS cached the time during its single read per commit.
        let time = self.times.get(&oid).copied().unwrap_or(i64::MAX);
        Ok((generation, time))
    }

    /// The generation of `oid` (root commits are generation 1, matching
    /// git's definition of *generation number* = max(parents)+1).
    ///
    /// # Errors
    ///
    /// [`GitError::Object`] when the commit cannot be read (commit-graph
    /// miss followed by an ODB failure).
    pub fn of(&mut self, oid: ObjectId) -> Result<u32, GitError> {
        if let Some(cached) = self.cache.get(&oid) {
            return Ok(*cached);
        }
        if let Some(graph) = &self.graph
            && let Some(position) = graph.lookup(oid)
        {
            let entry = graph.commit_at(position);
            let generation = entry.generation();
            self.cache.insert(oid, generation);
            return Ok(generation);
        }
        let generation = self.compute(oid)?;
        self.cache.insert(oid, generation);
        Ok(generation)
    }

    /// Exact generation via iterative post-order DFS over parents
    /// (generation = max(parents) + 1). Each commit is read **once**
    /// (time + parents together); history depth is unbounded, so no
    /// recursion (M1.7's rule for large monorepos).
    fn compute(&mut self, oid: ObjectId) -> Result<u32, GitError> {
        enum Frame {
            /// Visit the commit (after its parents).
            Enter(ObjectId),
            /// All parents visited: derive the generation.
            Merge(ObjectId),
        }
        // Parents staged during the visit so the object is read once.
        let mut staged: HashMap<ObjectId, Vec<ObjectId>> = HashMap::new();
        let mut stack: Vec<Frame> = vec![Frame::Enter(oid)];
        while let Some(frame) = stack.pop() {
            match frame {
                Frame::Enter(current) => {
                    if self.cache.contains_key(&current) {
                        continue;
                    }
                    let commit = self.repo.find_commit(current).map_err(|e| {
                        map_git_err("E-GIT-OBJECT", format!("cannot read commit {current}: {e}"))
                    })?;
                    let time = commit
                        .time()
                        .map_err(|e| {
                            map_git_err(
                                "E-GIT-OBJECT",
                                format!("cannot read time of {current}: {e}"),
                            )
                        })?
                        .seconds;
                    let parents: Vec<ObjectId> = commit.parent_ids().map(gix::Id::detach).collect();
                    self.times.insert(current, time);
                    stack.push(Frame::Merge(current));
                    let staged_parents = staged.entry(current).or_default();
                    for parent in parents {
                        stack.push(Frame::Enter(parent));
                        staged_parents.push(parent);
                    }
                }
                Frame::Merge(current) => {
                    if self.cache.contains_key(&current) {
                        continue;
                    }
                    let mut best = 0u32;
                    for parent in staged.remove(&current).unwrap_or_default() {
                        let generation = self.of(parent)?;
                        best = best.max(generation);
                    }
                    self.cache.insert(current, best.saturating_add(1));
                }
            }
        }
        self.cache
            .get(&oid)
            .copied()
            .ok_or_else(|| map_git_err("E-GIT-OBJECT", format!("generation of {oid} not derived")))
    }
}
