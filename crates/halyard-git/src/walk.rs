//! The painter walk (§8.3, M2.2): `git rev-list HEAD ^tagA ^tagB …`
//! computed **once for all packages** with per-commit bitsets.
//!
//! Correctness (the seed tool's bug): termination is *not* "each base was
//! encountered" — on non-linear history a merge can reach one base while
//! another branch is still unreleased. The walk stops only when **every**
//! commit left in the heap is stale for every based package (`flags ⊇
//! ALL`): such commits can only propagate stale bits downwards, so no
//! fresh emission is possible. Pops run in non-increasing generation
//! order, so with a commit-graph each commit is popped once after all its
//! children and its flags are final; without one, re-propagation on flag
//! growth keeps the result exact (a second pass over the affected
//! frontier), and the result carries the `degraded` marker (§8.3,
//! surfaced by `halyard doctor`).
//!
//! Determinism (I3): the heap orders by `(generation desc, committer
//! time desc, oid asc)`; ties never depend on heap layout, and re-running
//! any walk byte-compares identical (tested).
//!
//! Allocation audit (§14.3): per popped commit the walk allocates one
//! flags entry (amortized), one heap node (amortized) and one
//! `EmittedCommit` — measured with `dhat` in `tests/dhat_walk.rs` and
//! reported in the PR body.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use gix::hash::ObjectId;
use halyard_core::{PkgId, SmallBitset};

use crate::{map_git_err, refs::Generations, GitError};

/// One emitted commit: it is NEW for every package in `new_for` (not
/// reachable from that package's base). The tree-diff stage (M2.3)
/// refines `new_for` with per-package include filters — until then
/// attribution is path-agnostic by design (the M1.9 naive semantics,
/// documented in `PROGRESS.md`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmittedCommit {
    /// Commit object id.
    pub oid: ObjectId,
    /// Packages for which this commit is unreleased history (bitset over
    /// package ids; initial-release packages receive every commit).
    pub new_for: SmallBitset,
    /// Commit generation (max parents + 1; exact, graph or DFS).
    pub generation: u32,
    /// Committer timestamp (walk-order evidence, §8.3 heap key).
    pub time: i64,
}

/// The painter walk's result (§8.3): emitted commits in pop order plus
/// the degraded/truncation markers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaintResult {
    /// Emitted commits, pop order (deterministic).
    pub commits: Vec<EmittedCommit>,
    /// True when no commit-graph was available and generations came from
    /// the memoized DFS (correct but slower; `doctor` suggests writing
    /// one: `git commit-graph write --reachable --changed-paths`).
    pub degraded: bool,
    /// Set when the walk stopped early because an initial-release package
    /// exceeded `initial_release.max_commits` (§8.3 bound).
    pub truncated: bool,
}

/// Heap entry: max-ordered by `(generation, time)`, oid ascending as the
/// deterministic tie-break (the "largest" element carries the smallest
/// oid).
#[derive(Clone, Debug, Eq)]
struct HeapEntry {
    generation: u32,
    time: i64,
    oid: ObjectId,
}

impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.generation, self.time)
            .cmp(&(other.generation, other.time))
            .then_with(|| other.oid.cmp(&self.oid))
    }
}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Walk state shared by the pop loop and the push helper.
struct WalkState<'r, 'g> {
    repo: &'r gix::Repository,
    generations: &'g mut Generations<'r>,
    flags: HashMap<ObjectId, SmallBitset>,
    heap: BinaryHeap<HeapEntry>,
    all_based: SmallBitset,
    /// Every package's bit (based **and** initial). A commit is "exhausted"
    /// (can never be emitted) iff its flags ⊇ `all_packages`; with an
    /// initial-release package in play no commit can ever be exhausted
    /// (initial bits are never painted), so the walk correctly runs to the
    /// root (§8.3).
    all_packages: SmallBitset,
    /// Number of heap entries currently exhausted (fresh for no package).
    stale_count: usize,
}

impl<'r> WalkState<'r, '_> {
    /// Heap key for one commit: generation + timestamp, graph-first.
    ///
    /// # Errors
    ///
    /// [`GitError::Object`] when the commit cannot be read.
    fn entry(&mut self, oid: ObjectId) -> Result<(u32, i64), GitError> {
        let (generation, time) = self.generations.of_with_time(oid)?;
        let time = if time == 0 && !self.generations.has_graph() {
            commit_time(self.repo, oid)?
        } else {
            time
        };
        Ok((generation, time))
    }

    /// Pushes (or re-pushes, §8.3 re-propagation) one commit with the
    /// given bits unioned onto its flags. Returns the exact delta applied
    /// to `stale_count`.
    ///
    /// # Errors
    ///
    /// [`GitError::Object`] when the commit's generation or time cannot
    /// be read.
    fn push(&mut self, oid: ObjectId, bits: &SmallBitset) -> Result<u8, GitError> {
        match self.flags.get_mut(&oid) {
            Some(existing) => {
                let was_stale = is_stale(existing, &self.all_based);
                existing.union_with(bits);
                let is_now_stale = is_stale(existing, &self.all_based);
                if is_now_stale == was_stale {
                    // Flags may have grown without crossing the
                    // staleness threshold; the entry is already queued
                    // (or was popped with its final flags).
                    return Ok(0);
                }
                let (generation, time) = self.entry(oid)?;
                self.heap.push(HeapEntry {
                    generation,
                    time,
                    oid,
                });
                Ok(u8::from(is_now_stale) - u8::from(was_stale))
            }
            None => {
                self.flags.insert(oid, bits.clone());
                let (generation, time) = self.entry(oid)?;
                self.heap.push(HeapEntry {
                    generation,
                    time,
                    oid,
                });
                Ok(u8::from(is_stale(bits, &self.all_based)))
            }
        }
    }
}

/// True when `bits` mark the commit stale for every based package. With
/// no based package at all nothing is ever stale (the walk runs to the
/// root, §8.3).
fn is_stale(bits: &SmallBitset, all_packages: &SmallBitset) -> bool {
    !all_packages.is_empty() && bits.is_superset_of(all_packages)
}

/// Committer timestamp of one commit (heap key, §8.3).
///
/// # Errors
///
/// [`GitError::Object`] when the commit cannot be read.
fn commit_time(repo: &gix::Repository, oid: ObjectId) -> Result<i64, GitError> {
    let commit = repo
        .find_commit(oid)
        .map_err(|e| map_git_err("E-GIT-OBJECT", format!("cannot read commit {oid}: {e}")))?;
    Ok(commit
        .time()
        .map_err(|e| map_git_err("E-GIT-OBJECT", format!("cannot read time of {oid}: {e}")))?
        .seconds)
}

/// Paints the history between `head` and every package's base (§8.3).
///
/// `bases` is package-id aligned; `None` = initial release (its window
/// extends to the root, bounded by `initial_max_commits`).
///
/// # Errors
///
/// [`GitError::Object`] when commits cannot be read or generations
/// cannot be derived.
pub fn paint<'r>(
    repo: &'r gix::Repository,
    head: ObjectId,
    bases: &[Option<ObjectId>],
    initial_max_commits: u32,
    generations: &mut Generations<'r>,
) -> Result<PaintResult, GitError> {
    let degraded = !generations.has_graph();
    let package_count = bases.len();
    // Group base commits so packages sharing a release point paint their
    // bits together (each bit's propagation is per package, §8.3).
    let mut base_bits: HashMap<ObjectId, SmallBitset> = HashMap::new();
    let mut all_based = SmallBitset::new();
    for (i, base) in bases.iter().enumerate() {
        if let Some(oid) = base {
            all_based.set(PkgId::new(i as u32));
            base_bits
                .entry(*oid)
                .or_default()
                .set(PkgId::new(i as u32));
        }
    }

    let mut state = WalkState {
        repo,
        generations,
        flags: HashMap::new(),
        heap: BinaryHeap::new(),
        all_based,
        all_packages: {
            let mut all = SmallBitset::new();
            for i in 0..package_count {
                all.set(PkgId::new(i as u32));
            }
            all
        },
        stale_count: 0,
    };

    // Seed: HEAD is unstale for everyone; each base is stale for its own
    // packages (§8.3).
    state.stale_count += usize::from(state.push(head, &SmallBitset::new())?);
    for (oid, bits) in &base_bits {
        state.stale_count += usize::from(state.push(*oid, bits)?);
    }

    let mut commits: Vec<EmittedCommit> = Vec::new();
    let mut truncated = false;
    let bound = u64::from(initial_max_commits);
    while let Some(entry) = state.heap.pop() {
        let bits = state
            .flags
            .get(&entry.oid)
            .cloned()
            .unwrap_or_default();
        if is_stale(&bits, &state.all_based) {
            state.stale_count = state.stale_count.saturating_sub(1);
        }
        // Termination (§8.3): every remaining heap commit is fresh for no
        // package (flags ⊇ all packages' bits) ⇒ no fresh emission is
        // possible downstream. Exhaustion is monotone (flags only grow),
        // so the count is exact.
        if is_stale(&bits, &state.all_packages) && state.stale_count == state.heap.len() {
            break;
        }
        // fresh_for = ALL_PACKAGES \ flags (initial packages are always
        // fresh, §8.3).
        let mut fresh = SmallBitset::new();
        for i in 0..package_count {
            let id = PkgId::new(i as u32);
            if !bits.get(id) {
                fresh.set(id);
            }
        }
        if !fresh.is_empty() {
            if u64::try_from(commits.len()).unwrap_or(u64::MAX) >= bound
                && state.all_based.count() < package_count as u32
            {
                // An initial-release package is in play: stop at the
                // configured bound (§8.3).
                truncated = true;
                break;
            }
            commits.push(EmittedCommit {
                oid: entry.oid,
                new_for: fresh,
                generation: entry.generation,
                time: entry.time,
            });
        }
        let commit = repo
            .find_commit(entry.oid)
            .map_err(|e| {
                {
                    let oid = entry.oid;
                    map_git_err("E-GIT-OBJECT", format!("cannot read commit {oid}: {e}"))
                }
            })?;
        for parent in commit.parent_ids() {
            let parent = parent.detach();
            let parent_gen = state.generations.of(parent)?;
            if parent_gen > entry.generation {
                // A commit-graph claiming a parent above its child is
                // corrupt; the walk drops the edge instead of looping
                // (documented defensive bound, §8.3).
                continue;
            }
            state.stale_count += usize::from(state.push(parent, &bits)?);
        }
    }
    Ok(PaintResult {
        commits,
        degraded,
        truncated,
    })
}
