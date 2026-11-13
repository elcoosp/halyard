//! The merge policy (§8.5, M2.4): how merge commits, reverts and
//! autosquash commits flow into the [`HistoryDigest`].
//!
//! Default (`first-parent`, §16.4): every commit is diffed against its
//! **first parent** and a merge commit itself is *not* counted as a
//! change — its constituent commits are walked individually. With
//! `merge-message` the merge commit's own message is parsed as a change
//! entry when it is conventional (`count_merge_messages`, §8.5). `off`
//! disables the special handling entirely (every painted commit counts).
//!
//! Reverts (§8.5): a commit whose body carries git's
//! `This reverts commit <oid>` trailer cancels the referenced commit when
//! it is **within the same walk window** — both entries leave the digest
//! and no reason chain mentions them. A revert whose target is outside
//! the window (or has no trailer at all) is *treated as a `fix`* so the
//! planner's bump mapping stays correct.
//!
//! Autosquash (§8.5): `fixup!`/`squash!` subjects never emit a
//! [`RelevantCommit`] and never contribute a reason (the parser returns
//! `None` for them; the walk still visits them).
//!
//! The `(#NNN)` trailing PR-number extractor is a byte scan from the end
//! of the subject (`halyard-conventional`, no `regex` in the hot path
//! per §14.2 — enforced by a grep test in `tests/merge.rs`).

use bstr::{BString, ByteSlice as _};
use halyard_conventional::ConventionalCommit;

/// How merge commits are treated (§8.5; resolved from
/// `[commits].merge_policy`, §16.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MergePolicy {
    /// Diff each commit against its first parent; merge commits
    /// themselves are never counted as changes (default, §16.4
    /// `first-parent`).
    #[default]
    FirstParentDiff,
    /// Additionally parse merge-commit messages as change entries when
    /// they are conventional (§8.5 `count_merge_messages`).
    FirstParentMergeMessage,
    /// No special handling of merges: every painted commit counts.
    Off,
}

/// Counters surfaced to the PR body and the profile trace (§14.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MergeStats {
    /// Merge commits dropped under the first-parent policy.
    pub merges_dropped: usize,
    /// Merge commits kept as change entries.
    pub merges_kept: usize,
    /// (commit, revert) pairs canceled inside the window.
    pub reverts_canceled: usize,
    /// Reverts kept as `fix` because their target is outside the window
    /// (or carries no trailer).
    pub reverts_as_fix: usize,
}

/// A parsed, policy-gated candidate commit (§5.7 shape before the
/// per-package attribution).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// Position in the caller's candidate list (lets the digest assembly
    /// link a kept candidate back to its message offsets/author after
    /// the policy drops entries).
    pub origin: usize,
    /// Commit object id (hex).
    pub oid: BString,
    /// Parsed conventional commit.
    pub parsed: ConventionalCommit,
    /// Raw message bytes (subject/body offsets are computed against it).
    pub message: BString,
    /// True when the commit has more than one parent (a merge).
    pub is_merge: bool,
}

/// Applies the merge policy over parsed candidates (§8.5): drops merge
/// commits per the policy, then cancels in-window reverts and downgrades
/// out-of-window reverts to `fix`.
///
/// Input order is the walk order; output order is preserved (I3).
#[must_use]
pub fn apply_policy(candidates: Vec<Candidate>, policy: MergePolicy) -> (Vec<Candidate>, MergeStats) {
    let mut stats = MergeStats::default();
    let gated: Vec<Candidate> = candidates
        .into_iter()
        .filter(|c| {
            if !c.is_merge || policy != MergePolicy::FirstParentDiff {
                return true;
            }
            stats.merges_dropped += 1;
            false
        })
        .collect();
    let merges_kept = gated.iter().filter(|c| c.is_merge).count();
    stats.merges_kept = merges_kept;

    let canceled = cancel_reverts(gated, &mut stats);
    (canceled, stats)
}

/// The per-candidate revert decision (computed before any mutation so
/// the borrow split is clean).
enum RevertDecision {
    /// Not a revert-shaped commit.
    NotARevert,
    /// Revert whose target sits at index `j` of the kept window.
    Cancel(usize),
    /// Revert whose target is outside the window (or absent): keep as a
    /// `fix` (§8.5).
    KeepAsFix,
}

/// Revert cancellation (§8.5): a revert whose target is inside the kept
/// window removes both commits; otherwise the revert is kept with its
/// kind downgraded to `fix` so the §10.1 bump mapping treats it as one.
fn cancel_reverts(mut candidates: Vec<Candidate>, stats: &mut MergeStats) -> Vec<Candidate> {
    // OIDs of the kept window (hex, as parsed from the walk) and the
    // per-candidate revert shape/targets, computed before any mutation
    // (borrow split: decisions first, mutation second).
    let kept: Vec<BString> = candidates.iter().map(|c| c.oid.clone()).collect();
    let decisions: Vec<RevertDecision> = candidates
        .iter()
        .map(|c| {
            let shaped = halyard_conventional::revert_target(&c.message).is_some()
                || c.message.starts_with(b"Revert \"")
                || c.message.starts_with(b"revert:");
            if !shaped {
                return RevertDecision::NotARevert;
            }
            match halyard_conventional::revert_target(&c.message)
                .and_then(|t| kept.iter().position(|oid| oid == &t))
            {
                Some(j) => RevertDecision::Cancel(j),
                None => RevertDecision::KeepAsFix,
            }
        })
        .collect();
    let mut drop: Vec<bool> = vec![false; candidates.len()];
    for (i, decision) in decisions.iter().enumerate() {
        match decision {
            RevertDecision::NotARevert => {}
            RevertDecision::Cancel(j) => {
                // Cancel only when neither side is already dropped.
                if !drop[i] && !drop[*j] && i != *j {
                    drop[i] = true;
                    drop[*j] = true;
                    stats.reverts_canceled += 1;
                }
            }
            RevertDecision::KeepAsFix => {
                if let Some(candidate) = candidates.get_mut(i) {
                    downgrade_to_fix(candidate);
                    stats.reverts_as_fix += 1;
                }
            }
        }
    }
    let mut result: Vec<Candidate> = Vec::with_capacity(candidates.len());
    for (i, candidate) in candidates.drain(..).enumerate() {
        if !drop[i] {
            result.push(candidate);
        }
    }
    result
}

/// Downgrades a revert-shaped commit's kind to `fix` (§8.5: "otherwise
/// treated as a fix") while keeping the subject verbatim.
fn downgrade_to_fix(candidate: &mut Candidate) {
    if candidate.parsed.kind.as_bytes() != b"fix" {
        candidate.parsed.kind = BString::from("fix");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(origin: usize, subject: &str, body: &str, is_merge: bool) -> Candidate {
        let mut message = BString::from(subject);
        if !body.is_empty() {
            message.push(b'\n');
            message.push(b'\n');
            message.extend_from_slice(body.as_bytes());
        }
        Candidate {
            origin,
            oid: BString::from("a".repeat(40)),
            parsed: halyard_conventional::parse(message.as_bytes())
                .unwrap_or_else(|| halyard_conventional::ConventionalCommit {
                    kind: BString::from("revert"),
                    scope: None,
                    breaking: false,
                    description: BString::from(subject),
                    body: None,
                    footers: Vec::new(),
                    pr: None,
                }),
            message,
            is_merge,
        }
    }

    #[test]
    fn merge_commits_are_dropped_under_first_parent_diff() {
        let candidates = vec![
            candidate(0, "feat: one", "", false),
            candidate(1, "Merge branch 'x'", "", true),
            candidate(2, "fix: two", "", false),
        ];
        let (kept, stats) = apply_policy(candidates, MergePolicy::FirstParentDiff);
        assert_eq!(stats.merges_dropped, 1);
        assert_eq!(kept.len(), 2);
        // merge-message keeps them.
        let (kept, stats) = apply_policy(
            vec![candidate(0, "feat: real merge", "", true)],
            MergePolicy::FirstParentMergeMessage,
        );
        assert_eq!(stats.merges_kept, 1);
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn reverts_cancel_within_the_window() {
        let target = Candidate {
            oid: BString::from("b".repeat(40)),
            origin: 0,
            ..candidate(0, "feat: add widget", "", false)
        };
        let revert = Candidate {
            oid: BString::from("c".repeat(40)),
            origin: 1,
            ..candidate(
                1,
                "Revert \"feat: add widget\"",
                &format!("This reverts commit {}.", "b".repeat(40)),
                false,
            )
        };
        // Revert after the commit (forward order).
        let (kept, stats) = apply_policy(vec![target.clone(), revert.clone()], MergePolicy::FirstParentDiff);
        assert_eq!(stats.reverts_canceled, 1);
        assert!(kept.is_empty(), "commit + revert = zero net touch");
        // Same window, reverse order.
        let (kept, stats) = apply_policy(vec![revert, target], MergePolicy::FirstParentDiff);
        assert_eq!(stats.reverts_canceled, 1);
        assert!(kept.is_empty());
    }

    #[test]
    fn out_of_window_reverts_become_fixes() {
        let revert = candidate(
            0,
            "Revert \"feat: old\"",
            &format!("This reverts commit {}.", "d".repeat(40)),
            false,
        );
        let (kept, stats) = apply_policy(vec![revert], MergePolicy::FirstParentDiff);
        assert_eq!(stats.reverts_as_fix, 1);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].parsed.kind.as_bytes(), b"fix");
    }

    #[test]
    fn revert_without_trailer_is_a_fix() {
        let revert = candidate(0, "revert: undo the parser change", "", false);
        let (kept, stats) = apply_policy(vec![revert], MergePolicy::FirstParentDiff);
        assert_eq!(stats.reverts_as_fix, 1);
        assert_eq!(kept[0].parsed.kind.as_bytes(), b"fix");
    }

    #[test]
    fn ordinary_commits_are_untouched() {
        let (kept, stats) = apply_policy(
            vec![candidate(0, "feat: one", "", false), candidate(2, "fix: two", "", false)],
            MergePolicy::FirstParentDiff,
        );
        assert_eq!(kept.len(), 2);
        assert_eq!(stats.reverts_canceled, 0);
        assert_eq!(stats.merges_dropped, 0);
    }
}
