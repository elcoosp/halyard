//! Title-lint seam tests (§11.6, M4.3 `check`): the [`lint_title`]
//! verdict matrix — acceptance mapping through an injected
//! [`TitleBumpRule`], the fixed breaking rule (§10.1), unmapped kinds
//! with did-you-mean hints, rejections and autosquash detection. The
//! engine mirrors the planner's mapping (`planner.rs`: kind looked up
//! **exactly as written**, breaking always major).

use halyard_conventional::{Level, TitleBumpRule, TitleVerdict, lint_title};

/// A static table rule standing in for the shell's `PlanConfig` mapping
/// (§10.1 defaults: `feat`→minor, `fix`/`perf`→patch).
struct TableRule {
    map: &'static [(&'static str, Level)],
    known: &'static [&'static str],
}

impl TitleBumpRule for TableRule {
    fn bump_for_kind(&self, kind: &str) -> Level {
        self.map
            .iter()
            .find(|(k, _)| *k == kind)
            .map_or(Level::None, |&(_, level)| level)
    }

    fn known_kinds(&self) -> Vec<String> {
        self.known.iter().map(|k| (*k).to_owned()).collect()
    }
}

const DEFAULTS: TableRule = TableRule {
    map: &[
        ("feat", Level::Minor),
        ("fix", Level::Patch),
        ("perf", Level::Patch),
    ],
    known: &["feat", "fix", "perf"],
};

/// Unwraps an [`TitleVerdict::Accepted`] with a message that names the
/// input title (tests are the one allowed site).
fn accepted(title: &str, verdict: TitleVerdict) -> halyard_conventional::TitleLint {
    match verdict {
        TitleVerdict::Accepted(lint) => lint,
        other => unreachable!("title {title:?} should be accepted, got {other:?}"),
    }
}

#[test]
fn accepted_feat_conveys_minor() {
    let lint = accepted(
        "feat(parser): accept spans",
        lint_title("feat(parser): accept spans", &DEFAULTS),
    );
    assert_eq!(lint.kind, "feat");
    assert_eq!(lint.scope.as_deref(), Some("parser"));
    assert!(!lint.breaking);
    assert_eq!(lint.description, "accept spans");
    assert_eq!(lint.level, Level::Minor);
    assert_eq!(lint.closest_known, None);
}

#[test]
fn breaking_title_conveys_major_regardless_of_the_rule() {
    // `!` before the colon is major even though `fix` maps to patch.
    for title in [
        "feat!: drop the v1 wire format",
        "fix(api)!: handle empty refs",
    ] {
        let lint = accepted(title, lint_title(title, &DEFAULTS));
        assert!(lint.breaking, "{title}");
        assert_eq!(lint.level, Level::Major, "{title}");
    }
}

#[test]
fn unmapped_kind_contributes_no_bump_without_a_hint() {
    let lint = accepted(
        "chore: tidy the bench fixtures",
        lint_title("chore: tidy the bench fixtures", &DEFAULTS),
    );
    assert_eq!(lint.kind, "chore");
    assert_eq!(lint.level, Level::None);
    assert_eq!(lint.closest_known, None);
}

#[test]
fn known_but_unmapped_kind_does_not_hint_at_itself() {
    // A rule may know a kind while deliberately mapping it to no bump —
    // that is policy, not a typo, so the did-you-mean hint must stay off.
    let rule = TableRule {
        map: &[("feat", Level::Minor), ("chore", Level::None)],
        known: &["feat", "chore"],
    };
    let lint = accepted(
        "chore: tidy the bench fixtures",
        lint_title("chore: tidy the bench fixtures", &rule),
    );
    assert_eq!(lint.level, Level::None);
    assert_eq!(lint.closest_known, None);
}

#[test]
fn typo_kind_carries_a_did_you_mean_hint() {
    let lint = accepted(
        "feet: adjust the parser fixtures",
        lint_title("feet: adjust the parser fixtures", &DEFAULTS),
    );
    assert_eq!(lint.level, Level::None);
    assert_eq!(lint.closest_known.as_deref(), Some("feat"));
}

#[test]
fn not_conventional_title_is_rejected_with_a_reason() {
    match lint_title("updated the parser", &DEFAULTS) {
        TitleVerdict::NotConventional { reason } => {
            assert!(reason.contains("type"), "{reason}");
        }
        other => panic!("expected NotConventional, got {other:?}"),
    }
}

#[test]
fn empty_title_is_not_conventional() {
    match lint_title("", &DEFAULTS) {
        TitleVerdict::NotConventional { .. } => {}
        other => panic!("expected NotConventional, got {other:?}"),
    }
}

#[test]
fn fixup_and_squash_subjects_are_ignored() {
    for title in [
        "fixup! feat: the real change",
        "squash! fix: the folded change",
    ] {
        assert_eq!(
            lint_title(title, &DEFAULTS),
            TitleVerdict::FixupOrSquash,
            "{title}"
        );
    }
}

#[test]
fn kind_is_passed_to_the_rule_exactly_as_written() {
    // The planner looks kinds up as written (`planner.rs`); the seam must
    // not silently normalize case or the lint would predict bumps the
    // planner never produces.
    let rule = TableRule {
        map: &[("FEAT", Level::Minor)],
        known: &["FEAT"],
    };
    let lint = accepted(
        "FEAT: upper-case type",
        lint_title("FEAT: upper-case type", &rule),
    );
    assert_eq!(lint.kind, "FEAT");
    assert_eq!(lint.level, Level::Minor);
}

#[test]
fn only_the_subject_line_is_judged() {
    // A PR title is single-line; anything past the first LF is not part
    // of the lint (a trailing "BREAKING CHANGE" line must not leak into
    // the verdict).
    let lint = accepted(
        "feat: subject",
        lint_title("feat: subject\n\nBREAKING CHANGE: leaked", &DEFAULTS),
    );
    assert!(!lint.breaking);
    assert_eq!(lint.level, Level::Minor);
    assert_eq!(lint.description, "subject");
}

#[test]
fn description_bytes_are_preserved() {
    let title = "feat: ἀδειοδότησε τους χρήστες";
    let lint = accepted(title, lint_title(title, &DEFAULTS));
    assert_eq!(lint.description, "ἀδειοδότησε τους χρήστες");
}
