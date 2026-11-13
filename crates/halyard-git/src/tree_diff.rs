//! The trie-guided tree diff (§8.4, M2.3): for a commit and its first
//! parent, walk both trees **in lockstep, restricted to the package trie**
//! ([`halyard_core::Trie`]).
//!
//! Rules (§8.4):
//!
//! 1. equal entry OIDs ⇒ prune (the subtree is identical and never
//!    opened);
//! 2. entry names outside the trie ⇒ prune, **unless** the current
//!    position is inside a package root — there, differing subtrees are
//!    attributed to the deepest package root reached, and changed paths
//!    are collected (for include/exclude filtering) only when a touched
//!    package carries a filter;
//! 3. shared global paths (root `Cargo.toml`, `Cargo.lock`,
//!    `pnpm-workspace.yaml`) attribute to **all** packages (workspace
//!    state; documented decision — the §8.4 text leaves the shared-path
//!    policy open);
//! 4. the commit-graph's changed-path Bloom filter ([`crate::bloom`]) is
//!    consulted first: a negative for **every** package-root prefix and
//!    shared path skips the whole comparison (git stores a key for every
//!    leading directory of each changed path, so prefix negatives are
//!    sound);
//! 5. subtree results are memoized in an explicit `Vec`-backed LRU keyed
//!    on `(tree_oid_c, tree_oid_p, trie_node_id)` — deterministic lookup
//!    and iteration order (I3), not a hash-map sweep;
//! 6. per-package include/exclude globs are evaluated only on the
//!    already-collected changed paths inside that package.
//!
//! Blobs are **never read** (§8.4): the diff works on tree entries only,
//! so `--filter=blob:none` clones plan fine. Every tree and entry read is
//! counted ([`TreeDiffStats`]) — the §17 acceptance metric (≥ 10× fewer
//! tree entries than a naive diff) is read from these counters.

use std::collections::BTreeMap;

use bstr::{BString, ByteSlice as _};
use gix::hash::ObjectId;
use halyard_core::{PkgId, SmallBitset, Trie};
use tracing::trace;

use crate::bloom::ChangedPathBloom;
use crate::{GitError, map_git_err};

/// Counters for the §17 acceptance ("≥ 10× fewer tree entries read than
/// naive diff — counted"): read by the differential tests and the perf
/// gate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TreeDiffStats {
    /// Tree objects opened by the guided walk.
    pub trees_read: u64,
    /// Tree entries iterated by the guided walk.
    pub entries_read: u64,
    /// Subtree comparisons answered from the memo LRU.
    pub memo_hits: u64,
    /// Commits whose whole tree comparison was skipped by a Bloom
    /// negative.
    pub bloom_skips: u64,
    /// True when no changed-path filter was available (consultation never
    /// runs — a documented ceiling, never a wrong answer).
    pub bloom_unavailable: bool,
}

/// Memo key (§8.4 step 3): the two subtree OIDs (`None` = the absent
/// side) and the trie node the comparison happens at.
#[derive(Clone, Debug, PartialEq, Eq)]
struct MemoKey {
    c: Option<ObjectId>,
    p: Option<ObjectId>,
    node: usize,
}

/// Explicit `Vec`-backed LRU (§8.4 step 3): move-to-front on hit, oldest
/// entry evicted at capacity. The Vec order is the fixed iteration order
/// (I3) — no hash-map order anywhere near an output path.
#[derive(Debug)]
struct MemoLru {
    entries: Vec<(MemoKey, Walk)>,
    capacity: usize,
}

impl MemoLru {
    fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::new(),
            capacity: capacity.max(1),
        }
    }

    fn get(&mut self, key: &MemoKey) -> Option<Walk> {
        let pos = self.entries.iter().position(|(k, _)| k == key)?;
        let entry = self.entries.remove(pos);
        self.entries.insert(0, entry.clone());
        Some(entry.1)
    }

    fn put(&mut self, key: MemoKey, value: Walk) {
        if self.entries.len() >= self.capacity {
            self.entries.pop();
        }
        self.entries.insert(0, (key, value));
    }
}

/// One sorted tree entry with the git directory-suffix sort key.
#[derive(Clone, Debug)]
struct Entry {
    name: BString,
    oid: ObjectId,
    is_tree: bool,
}

impl Entry {
    /// Sort key per git's tree order: directories compare as `name/`.
    fn sort_key(&self) -> Vec<u8> {
        let mut key = Vec::with_capacity(self.name.len() + 1);
        key.extend_from_slice(&self.name);
        if self.is_tree {
            key.push(b'/');
        }
        key
    }
}

/// Attribution result of one subtree comparison.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Walk {
    touched: SmallBitset,
    /// Changed repo-relative paths per touched package (collected only
    /// when a package carries include/exclude filters, §8.4 step 5).
    paths: BTreeMap<PkgId, Vec<BString>>,
}

impl Walk {
    fn merge(&mut self, sub: Walk) {
        self.touched.union_with(&sub.touched);
        for (pkg, paths) in sub.paths {
            self.paths.entry(pkg).or_default().extend(paths);
        }
    }
}

/// The stateful tree differ: owns the memo LRU and the statistics for a
/// whole repository walk (one per [`crate::GitHistory::build`]).
pub struct TreeDiffer<'r> {
    repo: &'r gix::Repository,
    trie: Trie,
    bloom: Option<ChangedPathBloom>,
    /// Package root paths in package-id order (empty string = repo root).
    roots: Vec<BString>,
    /// Per-package include/exclude filters (package-id order).
    filters: Vec<halyard_core::PathFilter>,
    memo: MemoLru,
    stats: TreeDiffStats,
}

impl<'r> TreeDiffer<'r> {
    /// Builds a differ over `trie`. `filters` and `roots` must be
    /// package-id aligned. The Bloom consultation is enabled when the
    /// repository's commit-graph file carries changed-path filters
    /// (§8.4 step 4); without one the differ still answers exactly
    /// (consultation is only ever a skip of provably-unchanged work).
    #[must_use]
    pub fn new(
        repo: &'r gix::Repository,
        trie: Trie,
        roots: Vec<BString>,
        filters: Vec<halyard_core::PathFilter>,
    ) -> Self {
        let bloom = ChangedPathBloom::open(repo.git_dir());
        let stats = TreeDiffStats {
            bloom_unavailable: bloom.is_none(),
            ..TreeDiffStats::default()
        };
        Self {
            repo,
            trie,
            bloom,
            roots,
            filters,
            memo: MemoLru::new(128),
            stats,
        }
    }

    /// The counters so far (the §17 acceptance metric).
    #[must_use]
    pub fn stats(&self) -> &TreeDiffStats {
        &self.stats
    }

    /// Computes the touched-package bitset for one commit vs its first
    /// parent (`None` = root commit, diffed against the empty tree).
    /// `commit_position` is the commit's position in the commit-graph
    /// (`None` when there is no graph — disables the Bloom pre-check for
    /// that commit, never a wrong answer).
    ///
    /// # Errors
    ///
    /// [`GitError::Object`] when trees cannot be read.
    pub fn diff_commit_at(
        &mut self,
        tree_c: ObjectId,
        tree_p: Option<ObjectId>,
        commit_position: Option<usize>,
    ) -> Result<SmallBitset, GitError> {
        if let (Some(bloom), Some(position)) = (&self.bloom, commit_position) {
            let all_negative = self
                .probes()
                .iter()
                .all(|path| !bloom.possibly_changed(position, path.as_bytes()));
            if all_negative {
                self.stats.bloom_skips += 1;
                trace!("tree_diff: bloom negative for all package roots — skipping");
                return Ok(SmallBitset::new());
            }
        }
        let active = self
            .trie
            .roots_here(Trie::ROOT)
            .cloned()
            .unwrap_or_default();
        let result = self.walk_keyed(Some(tree_c), tree_p, Trie::ROOT, &active, &mut Vec::new())?;
        Ok(self.apply_filters(&result))
    }

    /// Probe paths for the Bloom pre-check: every package root plus the
    /// shared global paths (§8.4 step 4).
    fn probes(&self) -> Vec<BString> {
        let mut probes: Vec<BString> = self.roots.clone();
        for shared in ["Cargo.toml", "Cargo.lock", "pnpm-workspace.yaml"] {
            probes.push(BString::from(shared));
        }
        probes
    }

    /// Applies per-package include/exclude filters (§8.4 step 5): a
    /// package with a filter keeps its touched bit only when at least one
    /// collected changed path matches. The returned bitset is the
    /// filtered attribution (the raw one stays visible in
    /// [`Walk::touched`]-carrying internals and in the cache payload,
    /// which stores the raw tree-diff result).
    fn apply_filters(&self, result: &Walk) -> SmallBitset {
        let mut filtered = SmallBitset::new();
        for pkg in result.touched.iter() {
            let Some(filter) = self.filters.get(pkg.to_usize()) else {
                filtered.set(pkg);
                continue;
            };
            if filter.is_empty() {
                filtered.set(pkg);
                continue;
            }
            let passes = result.paths.get(&pkg).is_some_and(|paths| {
                paths
                    .iter()
                    .any(|path| path_matches(filter, path, pkg, &self.roots))
            });
            if passes {
                filtered.set(pkg);
            }
        }
        filtered
    }

    /// Reads and sorts one tree's entries (git order: directories compare
    /// with a trailing slash).
    fn read_tree_entries(&mut self, oid: ObjectId) -> Result<Vec<Entry>, GitError> {
        self.stats.trees_read += 1;
        let object = self
            .repo
            .find_object(oid)
            .map_err(|e| map_git_err("E-GIT-OBJECT", format!("cannot read tree {oid}: {e}")))?;
        if object.kind != gix::objs::Kind::Tree {
            return Err(map_git_err(
                "E-GIT-OBJECT",
                format!("object {oid} is a {}, expected a tree", object.kind),
            ));
        }
        let mut entries: Vec<Entry> = Vec::new();
        let mut iter = gix::objs::TreeRefIter::from_bytes(object.data.as_ref(), oid.kind());
        while let Some(entry) = iter
            .next()
            .transpose()
            .map_err(|e| map_git_err("E-GIT-OBJECT", format!("malformed tree {oid}: {e}")))?
        {
            self.stats.entries_read += 1;
            entries.push(Entry {
                name: BString::from(entry.filename),
                oid: entry.oid.into(),
                is_tree: entry.mode.is_tree(),
            });
        }
        entries.sort_by_key(Entry::sort_key);
        Ok(entries)
    }

    /// The lockstep walk (§8.4 steps 1–3), memoized per subtree.
    /// `c`/`p` are the two tree OIDs (`None` = the absent side, i.e. the
    /// empty tree); `active` carries the deepest package roots matched so
    /// far; `prefix` is the repo-relative directory path of the current
    /// position.
    fn walk_keyed(
        &mut self,
        c: Option<ObjectId>,
        p: Option<ObjectId>,
        node: usize,
        active: &SmallBitset,
        prefix: &mut Vec<u8>,
    ) -> Result<Walk, GitError> {
        // §8.4 step 1: equal OIDs ⇒ identical subtree, prune.
        if matches!((&c, &p), (Some(c_oid), Some(p_oid)) if c_oid == p_oid) {
            return Ok(Walk::default());
        }
        // §8.4 step 3: memoized subtree result.
        let key = MemoKey { c, p, node };
        if let Some(hit) = self.memo.get(&key) {
            self.stats.memo_hits += 1;
            return Ok(hit);
        }
        let c_list = match c {
            Some(oid) => Some(self.read_tree_entries(oid)?),
            None => None,
        };
        let p_list = match p {
            Some(oid) => Some(self.read_tree_entries(oid)?),
            None => None,
        };
        let mut out = Walk::default();
        let mut ci = 0usize;
        let mut pi = 0usize;
        let c_entries = c_list.unwrap_or_default();
        let p_entries = p_list.unwrap_or_default();
        loop {
            let ce = c_entries.get(ci);
            let pe = p_entries.get(pi);
            match (ce, pe) {
                (None, None) => break,
                (Some(e), None) => {
                    self.descend(Some(e), None, node, active, prefix, &mut out)?;
                    ci += 1;
                }
                (None, Some(e)) => {
                    self.descend(None, Some(e), node, active, prefix, &mut out)?;
                    pi += 1;
                }
                (Some(ce), Some(pe)) => match ce.sort_key().cmp(&pe.sort_key()) {
                    std::cmp::Ordering::Less => {
                        self.descend(Some(ce), None, node, active, prefix, &mut out)?;
                        ci += 1;
                    }
                    std::cmp::Ordering::Greater => {
                        self.descend(None, Some(pe), node, active, prefix, &mut out)?;
                        pi += 1;
                    }
                    std::cmp::Ordering::Equal => {
                        if ce.oid != pe.oid {
                            self.descend(Some(ce), Some(pe), node, active, prefix, &mut out)?;
                        }
                        ci += 1;
                        pi += 1;
                    }
                },
            }
        }
        self.memo.put(key, out.clone());
        Ok(out)
    }

    /// Handles one differing entry: trie routing, package attribution,
    /// recursion into trees, blob path collection.
    fn descend(
        &mut self,
        c: Option<&Entry>,
        p: Option<&Entry>,
        node: usize,
        active: &SmallBitset,
        prefix: &mut Vec<u8>,
        out: &mut Walk,
    ) -> Result<(), GitError> {
        let entry = c.or(p);
        let Some(entry) = entry else {
            return Ok(());
        };
        let name = &entry.name;
        // Shared global path (§8.4): an exact root-level file whose change
        // is workspace-wide state — attributes to all packages.
        let trie_child = self.trie.child(node, name);
        if trie_child.is_some_and(|child| self.trie.is_shared(child)) {
            for i in 0..self.filters.len() {
                out.touched
                    .set(PkgId::new(u32::try_from(i).unwrap_or(u32::MAX)));
            }
            return Ok(());
        }
        // Deepest-root-wins attribution (§8.4 step 2): entering a nested
        // package root resets `active` to that root's packages.
        let next_active = match trie_child {
            Some(child) => match self.trie.roots_here(child) {
                Some(roots) if !roots.is_empty() => roots.clone(),
                _ => active.clone(),
            },
            None => active.clone(),
        };
        // Outside every package root and not on any package-root path ⇒
        // prune without opening (§8.4: entries outside any trie node are
        // never opened).
        if next_active.is_empty() && trie_child.is_none() {
            return Ok(());
        }

        let c_is_tree = c.is_some_and(|e| e.is_tree);
        let p_is_tree = p.is_some_and(|e| e.is_tree);
        if c_is_tree || p_is_tree {
            // Tree subtree (a tree↔blob type change counts as add+delete
            // of the two shapes).
            let c_oid = c.filter(|_| c_is_tree).map(|e| e.oid);
            let p_oid = p.filter(|_| p_is_tree).map(|e| e.oid);
            let walk_node = trie_child.unwrap_or(node);
            let was_empty = prefix.is_empty();
            if !was_empty {
                prefix.push(b'/');
            }
            prefix.extend_from_slice(name);
            let sub = self.walk_keyed(c_oid, p_oid, walk_node, &next_active, prefix)?;
            out.merge(sub);
            let cut = name.len() + usize::from(!was_empty);
            prefix.truncate(prefix.len().saturating_sub(cut));
            return Ok(());
        }
        // Blob change inside a package: attribute; collect the path only
        // when that package's filter demands path-level decisions
        // (§8.4 step 5).
        for pkg in next_active.iter() {
            out.touched.set(pkg);
            if self
                .filters
                .get(pkg.to_usize())
                .is_some_and(|f| !f.is_empty())
            {
                let mut path = prefix.clone();
                if !path.is_empty() {
                    path.push(b'/');
                }
                path.extend_from_slice(name);
                out.paths.entry(pkg).or_default().push(BString::from(path));
            }
        }
        Ok(())
    }
}

/// True when `path` (repo-relative) passes the package's include/exclude
/// filter: empty include = everything counts; otherwise at least one
/// include glob must match (evaluated against the path relative to the
/// package root) and no exclude glob may match.
fn path_matches(
    filter: &halyard_core::PathFilter,
    path: &BString,
    pkg: PkgId,
    roots: &[BString],
) -> bool {
    let root: &[u8] = roots.get(pkg.to_usize()).map_or(&[][..], |r| r.as_bytes());
    let rel: &[u8] = if root.is_empty() {
        path.as_bytes()
    } else {
        path.strip_prefix(root).unwrap_or(path.as_bytes())
    };
    let rel = rel.strip_prefix(b"/").unwrap_or(rel);
    let rel = std::str::from_utf8(rel).unwrap_or_default();
    let glob_matches =
        |g: &BString| halyard_core::glob::matches(std::str::from_utf8(g).unwrap_or_default(), rel);
    if !filter.include().is_empty() && !filter.include().iter().any(glob_matches) {
        return false;
    }
    !filter.exclude().iter().any(glob_matches)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memo_lru_is_deterministic_and_bounded() {
        let mut memo = MemoLru::new(2);
        let key = |n| MemoKey {
            c: None,
            p: None,
            node: n,
        };
        memo.put(key(1), Walk::default());
        memo.put(key(2), Walk::default());
        memo.get(&key(1)).expect("still cached");
        memo.put(key(3), Walk::default());
        // Node 2 was the least recently used: evicted.
        assert!(memo.get(&key(2)).is_none());
        assert!(memo.get(&key(3)).is_some());
        assert!(memo.get(&key(1)).is_some());
    }

    #[test]
    fn path_filter_semantics() {
        let roots = vec![BString::from("crates/core")];
        let filter =
            halyard_core::PathFilter::new(vec!["src/**".to_owned()], vec!["**/*.snap".to_owned()]);
        let pkg = PkgId::new(0);
        assert!(path_matches(
            &filter,
            &BString::from("crates/core/src/lib.rs"),
            pkg,
            &roots
        ));
        assert!(!path_matches(
            &filter,
            &BString::from("crates/core/tests/snap/x.snap"),
            pkg,
            &roots
        ));
        // Everything-counts filter.
        let empty = halyard_core::PathFilter::default();
        assert!(path_matches(
            &empty,
            &BString::from("crates/core/anything.txt"),
            pkg,
            &roots
        ));
    }
}
