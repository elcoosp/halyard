//! History digest types (§5.7): the per-package view of the commits that
//! matter between HEAD and each package's release base.
//!
//! The painter walk that produces this digest is the M2 git engine; for
//! M1.9 the planner consumes a naive `gix`-based provider through the
//! [`crate::history`] types. Parsed-commit detail lives in
//! `halyard-conventional` — [`ParsedCommit`] is the plain-data summary
//! core carries (§7.2: core takes no workspace dependencies).

use bstr::BString;
use smallvec::SmallVec;

use crate::bitset::SmallBitset;
use crate::id::PkgId;
use crate::reason::Oid;

/// Commit author identity as raw bytes (git allows arbitrary bytes in
/// names; nothing here is validated or mangled).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorRef {
    /// Author name (raw bytes).
    pub name: BString,
    /// Author email (raw bytes).
    pub email: BString,
}

/// A `Key: value` commit-message trailer (git trailers, §5.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trailer {
    /// Trailer key (`Co-authored-by`, `Refs`, …).
    pub key: BString,
    /// Trailer value (may span multiple lines joined by `\n`).
    pub value: BString,
}

/// Plain-data summary of one parsed conventional commit (§5.7 `parsed`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedCommit {
    /// Commit type (`feat`, `fix`, …; open set).
    pub kind: BString,
    /// Scope as written (`None` when absent).
    pub scope: Option<BString>,
    /// Breaking via `!` or a `BREAKING CHANGE` footer.
    pub breaking: bool,
    /// Description after the colon (non-empty for accepted commits).
    pub description: BString,
}

/// One commit relevant to at least one package (§5.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelevantCommit {
    /// Commit object id.
    pub oid: Oid,
    /// Subject line (first line of the message, raw bytes).
    pub subject: BString,
    /// Byte range of the body inside the raw message buffer (`0..0` when
    /// the message has no body).
    pub body_range: core::ops::Range<u32>,
    /// Parsed conventional-commit summary.
    pub parsed: ParsedCommit,
    /// PR number extracted from a trailing `(#NNN)` in the subject (§8.5).
    pub pr: Option<u64>,
    /// Author identity.
    pub author: AuthorRef,
    /// Footers/trailers (at most a handful per commit; inline ≤ 4).
    pub trailers: SmallVec<[Trailer; 4]>,
    /// Packages this commit touches (drives per-package attribution).
    pub touched: SmallBitset,
}

/// Index into [`HistoryDigest::commits`].
pub type CommitIdx = usize;

/// The release base of one package: the tag it was last released at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BaseRef {
    /// Tag name as rendered (e.g. `core-v0.4.1`).
    pub tag: BString,
    /// Tag commit (peeled for annotated tags, §8.2).
    pub oid: Oid,
}

/// Digest of the walk between HEAD and every package's base (§5.7).
///
/// `bases[i] == None` means package `i` has no release base yet
/// (`Base::Initial`, §8.2): its walk goes to the root, bounded by
/// `initial_release.max_commits` (§8.3) and later by the cache.
#[derive(Clone, Debug, Default)]
pub struct HistoryDigest {
    /// All relevant commits in walk order.
    pub commits: Vec<RelevantCommit>,
    /// Per-package commit indices, in walk order.
    pub per_pkg: Vec<Vec<CommitIdx>>,
    /// Release base per package (`None` = initial release).
    pub bases: Vec<Option<BaseRef>>,
}

impl HistoryDigest {
    /// Commit indices for one package (`None`-safe: an out-of-range or
    /// base-less package yields an empty slice).
    pub fn commits_for(&self, pkg: PkgId) -> &[CommitIdx] {
        self.per_pkg.get(pkg.to_usize()).map_or(&[], Vec::as_slice)
    }

    /// The base of one package (`None` = initial release).
    pub fn base_of(&self, pkg: PkgId) -> Option<&BaseRef> {
        self.bases.get(pkg.to_usize()).and_then(|b| b.as_ref())
    }
}

/// The history seam between the git engine (M2) and the pure planner
/// (§4, M1.9): the planner reads **pre-materialized** data through this
/// trait, which is what keeps [`crate::planner::plan`] free of I/O
/// (invariant I6). A provider implementation (the in-memory
/// [`FixedHistory`], or the `gix`-backed engine in `halyard-git`) does its
/// walking at *construction* time — inside the imperative shell — and
/// serves the finished [`HistoryDigest`] immutably afterwards.
pub trait HistoryProvider {
    /// The commit the snapshot was taken at (`None` = empty repository).
    fn head(&self) -> Option<Oid>;

    /// The materialized digest between HEAD and every package's release
    /// base (§5.7). Must stay aligned with the [`ReleaseRefs`] the
    /// snapshot carries: `bases[i]` describes package `i` in both.
    fn digest(&self) -> &HistoryDigest;

    /// Commit indices relevant to one package (convenience over
    /// [`HistoryDigest::commits_for`]).
    fn commits_for(&self, pkg: PkgId) -> &[CommitIdx] {
        self.digest().commits_for(pkg)
    }
}

/// An in-memory [`HistoryProvider`]: a pre-built digest (and head) served
/// as-is. This is the planner's test double *and* the M1.9 "naive"
/// provider: fixture-built digests (from `halyard-test-support` scenarios
/// or recorded walks) flow in unchanged, so golden plans are reproducible
/// without any git engine.
#[derive(Clone, Debug, Default)]
pub struct FixedHistory {
    head: Option<Oid>,
    digest: HistoryDigest,
}

impl FixedHistory {
    /// Wraps a materialized digest.
    pub fn new(head: Option<Oid>, digest: HistoryDigest) -> Self {
        Self { head, digest }
    }

    /// Mutable digest access for test-fixture construction (the fields of
    /// [`HistoryDigest`] are public; this keeps `FixedHistory`'s own
    /// fields private so invariants stay with the constructors).
    pub fn digest_mut(&mut self) -> &mut HistoryDigest {
        &mut self.digest
    }
}

impl HistoryProvider for FixedHistory {
    fn head(&self) -> Option<Oid> {
        self.head.clone()
    }

    fn digest(&self) -> &HistoryDigest {
        &self.digest
    }
}

/// Per-package release bases (§4 `refs`, §8.2): entry `i` is the base of
/// package `i` (aligned with [`crate::package::Workspace::packages`]);
/// `None` = the package has no release tag yet (`Base::Initial`).
///
/// Produced by `halyard-git`'s ref enumeration (M2.1) and consumed by the
/// planner through [`crate::planner::Snapshot::refs`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReleaseRefs(Vec<Option<BaseRef>>);

impl ReleaseRefs {
    /// Builds the refs from per-package bases (must be aligned with the
    /// workspace's package list).
    pub fn from_bases(bases: Vec<Option<BaseRef>>) -> Self {
        Self(bases)
    }

    /// The bases, package-id aligned.
    pub fn bases(&self) -> &[Option<BaseRef>] {
        &self.0
    }

    /// The base of one package (`None` = initial release or foreign id).
    pub fn base_of(&self, pkg: PkgId) -> Option<&BaseRef> {
        self.0.get(pkg.to_usize()).and_then(|b| b.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_indexes_packages() {
        let digest = HistoryDigest {
            commits: Vec::new(),
            per_pkg: vec![vec![0], vec![0, 1]],
            bases: vec![
                Some(BaseRef {
                    tag: BString::from("core-v0.4.1"),
                    oid: Oid::from_hex(&[b'a'; 40]).unwrap(),
                }),
                None,
            ],
        };
        assert_eq!(digest.commits_for(PkgId::new(1)).len(), 2);
        assert_eq!(digest.commits_for(PkgId::new(7)).len(), 0);
        assert!(digest.base_of(PkgId::new(0)).is_some());
        assert!(digest.base_of(PkgId::new(1)).is_none());
        assert_eq!(
            digest.base_of(PkgId::new(0)).map(|b| b.tag.to_string()),
            Some("core-v0.4.1".to_owned())
        );
    }
}
