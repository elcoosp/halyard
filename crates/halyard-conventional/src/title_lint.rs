//! The PR-title lint seam (§11.6, M4.3 `check`): map a squash-merge PR
//! title to the bump it would convey once merged.
//!
//! In squash-merge repositories the bump comes from the **PR title**
//! (§11.6): the merge collapses every commit into one whose subject is
//! the title, so the planner will later map exactly this one string. The
//! seam mirrors that mapping byte-for-byte — the kind is passed to the
//! rule *exactly as written* (`planner.rs` looks kinds up as written),
//! and `!`/breaking titles are always major (§10.1's fixed rule, not a
//! policy knob).
//!
//! This crate is the workspace's standalone parser leaf (ADR-0002), so
//! the *policy* does not live here: the caller injects the type→bump
//! mapping through [`TitleBumpRule`] (the shell implements it over
//! `PlanConfig::commit_types`). The leaf stays dependency-free while the
//! prediction stays planner-exact.
//!
//! ```no_run
//! use halyard_conventional::{Level, TitleBumpRule, lint_title};
//!
//! struct Defaults;
//!
//! impl TitleBumpRule for Defaults {
//!     fn bump_for_kind(&self, kind: &str) -> Level {
//!         match kind {
//!             "feat" => Level::Minor,
//!             "fix" | "perf" => Level::Patch,
//!             _ => Level::None,
//!         }
//!     }
//!     fn known_kinds(&self) -> Vec<String> {
//!         vec!["feat".to_owned(), "fix".to_owned(), "perf".to_owned()]
//!     }
//! }
//!
//! let verdict = lint_title("feat(parser): accept spans", &Defaults);
//! assert!(matches!(verdict, halyard_conventional::TitleVerdict::Accepted(_)));
//! ```

use crate::parser::{is_fixup_or_squash, parse};

/// The bump level a PR title conveys (§5.5, totally ordered). The leaf
/// mirrors `halyard_core::plan::Bump` — it cannot depend on core
/// (ADR-0002: the parser is standalone), so the shell converts via its
/// policy layer instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// No version change (unmapped type, §10.1: absent types contribute
    /// no bump).
    None,
    /// `z` component bump.
    Patch,
    /// `y` component bump.
    Minor,
    /// `x` component bump.
    Major,
}

impl Level {
    /// The lowercase wire name used by the `check` JSON document
    /// (`none` | `patch` | `minor` | `major`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Patch => "patch",
            Self::Minor => "minor",
            Self::Major => "major",
        }
    }
}

impl std::fmt::Display for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The type→bump policy behind [`lint_title`] (§10.1's configurable half:
/// `feat`→minor, `fix`→patch, …; unmapped kinds contribute no bump).
///
/// Implemented by the shell over `PlanConfig::commit_types` — the engine
/// stays policy-free so the prediction can never drift from the planner
/// mapping it mirrors.
pub trait TitleBumpRule {
    /// The bump the commit type `kind` conveys, **exactly as written** in
    /// the title (the planner performs the same as-written lookup;
    /// `Level::None` for unmapped types).
    fn bump_for_kind(&self, kind: &str) -> Level;

    /// The rule's known kinds, used only for the did-you-mean hint on
    /// unmapped kinds ([`TitleLint::closest_known`]). Case-insensitive
    /// comparisons are a hint-only concern; semantics stay as-written.
    fn known_kinds(&self) -> Vec<String>;
}

/// One accepted title, fully resolved: what was parsed and what bump the
/// injected rule conveys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TitleLint {
    /// Commit type exactly as written (`feat`, `FEAT`, …).
    pub kind: String,
    /// Scope exactly as written (`None` when absent).
    pub scope: Option<String>,
    /// Breaking via `!` before the colon (a title has no footers; the
    /// lint judges the subject line only).
    pub breaking: bool,
    /// Description after the colon.
    pub description: String,
    /// The conveyed bump: [`Level::Major`] when breaking (§10.1 fixed
    /// rule), otherwise the rule's mapping for the kind.
    pub level: Level,
    /// When the kind is unmapped and a known kind is within edit distance
    /// 2: the likely intended kind (did-you-mean, §11.6's precise
    /// diagnosis). `None` when mapped or nothing is close.
    pub closest_known: Option<String>,
}

/// The outcome of linting one PR title. A rejected title is a *verdict*,
/// not an error — the caller (the `check` command) decides the exit
/// behavior (§11.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TitleVerdict {
    /// The title is a conventional commit; see [`TitleLint`].
    Accepted(TitleLint),
    /// The parser declined the title: not a conventional commit. The
    /// reason states the expected shape (§11.6: errors teach).
    NotConventional {
        /// What was expected instead of the input.
        reason: String,
    },
    /// `fixup!`/`squash!` subject — such commits are ignored by release
    /// logic (§8.5) and must never gate a merge.
    FixupOrSquash,
}

/// Lints one PR title against `rule` (§11.6). Only the subject line is
/// judged: a PR title is single-line, so everything after the first LF is
/// ignored (a trailing `BREAKING CHANGE` line cannot leak into the
/// verdict).
pub fn lint_title(title: &str, rule: &dyn TitleBumpRule) -> TitleVerdict {
    // The subject line only — titles are single-line by contract (§11.6).
    let subject = title.split('\n').next().unwrap_or_default();
    if is_fixup_or_squash(subject.as_bytes()) {
        return TitleVerdict::FixupOrSquash;
    }
    let Some(parsed) = parse(subject.as_bytes()) else {
        return TitleVerdict::NotConventional {
            reason: "expected `<type>(<scope>): <description>` per Conventional \
                     Commits 1.0.0 (e.g. `feat(parser): accept spans`)"
                .to_owned(),
        };
    };
    let kind = String::from_utf8_lossy(&parsed.kind).into_owned();
    let level = if parsed.breaking {
        // §10.1's fixed rule (mirrors `planner.rs`): breaking is always
        // major — the type map is never consulted.
        Level::Major
    } else {
        rule.bump_for_kind(&kind)
    };
    // The did-you-mean hint only fires for kinds the rule does not know
    // at all. A *known* kind that maps to no bump (`chore` → none) is an
    // intentional policy choice, not a typo — hinting at itself would be
    // noise.
    let closest_known = if level == Level::None {
        let known = rule.known_kinds();
        if known.iter().any(|name| name.eq_ignore_ascii_case(&kind)) {
            None
        } else {
            closest_kind(&kind, &known)
        }
    } else {
        None
    };
    TitleVerdict::Accepted(TitleLint {
        kind,
        scope: parsed
            .scope
            .map(|s| String::from_utf8_lossy(&s).into_owned()),
        breaking: parsed.breaking,
        description: String::from_utf8_lossy(&parsed.description).into_owned(),
        level,
        closest_known,
    })
}

/// The known kind within edit distance ≤ 2 of `kind` (case-insensitive),
/// if any. Kinds are short, so a plain DP with the cutoff folded in is
/// cheap (cold path: one lint per `check` run).
fn closest_kind(kind: &str, known: &[String]) -> Option<String> {
    let mut best: Option<(usize, &String)> = None;
    for candidate in known {
        // Candidates beyond the cap are skipped, not fatal: an earlier
        // in-cap match must survive a later far-off one.
        let Some(distance) = edit_distance_cutoff(kind, candidate, 2) else {
            continue;
        };
        if best.is_none_or(|(best_distance, _)| distance < best_distance) {
            best = Some((distance, candidate));
        }
    }
    best.map(|(_, name)| name.clone())
}

/// Levenshtein distance between the lowercased byte strings, capped:
/// returns `None` once the distance provably exceeds `cap`. Two rolling
/// rows carried with `windows`/`iter` accessors — the workspace denies
/// `indexing_slicing`, so the DP never indexes — and an early abandon
/// that keeps the scan O(len) in practice for short kind names.
fn edit_distance_cutoff(a: &str, b: &str, cap: usize) -> Option<usize> {
    let a = a.to_lowercase().into_bytes();
    let b = b.to_lowercase().into_bytes();
    if a.len().abs_diff(b.len()) > cap {
        return None;
    }
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        // Row i+1: current[0] = i + 1, then
        // current[j + 1] = min(previous[j] + cost, previous[j + 1] + 1,
        //                      current[j] + 1)
        // for j over `previous.windows(2)` zipped with `b`.
        let mut current = Vec::with_capacity(b.len() + 1);
        // The left boundary column D(i + 1, 0) — without it the rolling
        // row shrinks by one per iteration and the DP loses the base
        // case for the first byte of `b`.
        current.push(i + 1);
        let mut row_min = i + 1;
        let mut last = i + 1;
        for (pair, cb) in previous.windows(2).zip(&b) {
            let diagonal = pair.first().copied().unwrap_or_default();
            let above = pair.get(1).copied().unwrap_or_default();
            let cost = usize::from(ca != cb);
            let next = (diagonal + cost).min(above + 1).min(last + 1);
            last = next;
            row_min = row_min.min(next);
            current.push(next);
        }
        if row_min > cap {
            return None;
        }
        previous = current;
    }
    let distance = previous.last().copied().unwrap_or_default();
    (distance <= cap).then_some(distance)
}

#[cfg(test)]
mod tests {
    use super::{Level, TitleVerdict, closest_kind, edit_distance_cutoff, lint_title};

    struct Identity;

    impl super::TitleBumpRule for Identity {
        fn bump_for_kind(&self, kind: &str) -> Level {
            match kind {
                "feat" => Level::Minor,
                _ => Level::None,
            }
        }

        fn known_kinds(&self) -> Vec<String> {
            vec!["feat".to_owned(), "fix".to_owned(), "perf".to_owned()]
        }
    }

    #[test]
    fn edit_distance_cutoff_bands_as_documented() {
        assert_eq!(edit_distance_cutoff("feat", "feat", 2), Some(0));
        assert_eq!(edit_distance_cutoff("feet", "feat", 2), Some(1));
        assert_eq!(edit_distance_cutoff("chore", "feat", 2), None);
        assert_eq!(edit_distance_cutoff("FEAT", "feat", 2), Some(0));
    }

    #[test]
    fn closest_kind_prefers_the_smallest_distance() {
        let known = vec!["feat".to_owned(), "fix".to_owned()];
        assert_eq!(closest_kind("feet", &known).as_deref(), Some("feat"));
        assert_eq!(closest_kind("fex", &known).as_deref(), Some("fix"));
        assert_eq!(closest_kind("docs", &known), None);
    }

    #[test]
    fn breaking_titles_skip_the_hint_path() {
        // A breaking title is major regardless of the kind, so no
        // did-you-mean hint is computed even for a typo'd kind.
        match lint_title("feet!: drop the wire format", &Identity) {
            TitleVerdict::Accepted(lint) => {
                assert_eq!(lint.level, Level::Major);
                assert_eq!(lint.closest_known, None);
            }
            other => panic!("expected Accepted, got {other:?}"),
        }
    }
}
