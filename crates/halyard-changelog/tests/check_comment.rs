//! The §11.6 check-comment renderer (M4.5 step 1): the stable, parseable
//! PR comment the `check --pr-title --comment` mode posts to the PR
//! under review. The body is deterministic (I3) — a given verdict
//! renders byte-for-byte identically on every run, so the marker-based
//! comment upsert (the forge seam's `upsert_pr_comment`) rewrites the
//! same comment in place instead of piling up duplicates.

use halyard_changelog::check_comment::{
    CHECK_MARKER, CheckComment, CheckPrediction, CheckVerdict, render_check_comment,
};
use halyard_core::Bump;

fn prediction(package: &str, from: &str, bump: Bump, remapped: bool) -> CheckPrediction {
    CheckPrediction {
        package: package.to_owned(),
        from: from.to_owned(),
        bump,
        remapped,
    }
}

fn accepted() -> CheckComment {
    CheckComment {
        title: "feat(parser): accept spans".to_owned(),
        verdict: CheckVerdict::Accepted {
            kind: "feat".to_owned(),
            breaking: false,
        },
        predictions: vec![
            prediction("p1", "0.4.1", Bump::Patch, true),
            prediction("p2", "1.0.3", Bump::Minor, false),
        ],
    }
}

#[test]
fn the_marker_is_a_stable_html_comment() {
    assert_eq!(CHECK_MARKER, "<!-- halyard:check v1 -->");
}

#[test]
fn the_accepted_body_leads_with_the_marker_and_names_the_bumps() {
    let body = render_check_comment(&accepted());
    let first = body
        .lines()
        .next()
        .unwrap_or_else(|| unreachable!("the body is never empty"));
    assert_eq!(
        first, CHECK_MARKER,
        "the marker is the first line — the find key"
    );
    assert!(
        body.contains("## 🧪 halyard check"),
        "the heading identifies the bot"
    );
    assert!(
        body.contains("`feat(parser): accept spans`"),
        "the title rides verbatim (the bump input, §11.6)"
    );
    assert!(
        body.contains("| `p1` | 0.4.1 | patch (0.x policy: remapped for pre-major) |"),
        "the pre-major remap row names its cause"
    );
    assert!(body.contains("| `p2` | 1.0.3 | minor |"), "the plain row");
}

#[test]
fn the_remap_is_taught_not_silent() {
    let body = render_check_comment(&accepted());
    assert!(
        body.contains("pre-major"),
        "a remapped prediction says why its bump differs, got:\n{body}"
    );
}

#[test]
fn no_pending_releases_render_the_honest_note() {
    let input = CheckComment {
        title: "chore: tidy the readme".to_owned(),
        verdict: CheckVerdict::Accepted {
            kind: "chore".to_owned(),
            breaking: false,
        },
        predictions: Vec::new(),
    };
    let body = render_check_comment(&input);
    assert!(body.starts_with(CHECK_MARKER));
    assert!(
        body.contains("No pending releases"),
        "an accepted non-release title says so plainly, got:\n{body}"
    );
}

#[test]
fn a_rejected_verdict_carries_the_diagnosis() {
    let input = CheckComment {
        title: "updated the parser".to_owned(),
        verdict: CheckVerdict::Rejected {
            reason: "not a conventional commit: missing `type(scope): description`".to_owned(),
        },
        predictions: Vec::new(),
    };
    let body = render_check_comment(&input);
    assert!(body.starts_with(CHECK_MARKER));
    assert!(
        body.contains("rejected"),
        "the comment teaches the fix, got:\n{body}"
    );
    assert!(
        body.contains("missing `type(scope): description`"),
        "the diagnosis rides verbatim"
    );
}

#[test]
fn the_render_is_deterministic() {
    assert_eq!(
        render_check_comment(&accepted()),
        render_check_comment(&accepted())
    );
}

#[test]
fn the_breaking_title_is_marked() {
    let input = CheckComment {
        title: "feat!: drop the wire format".to_owned(),
        verdict: CheckVerdict::Accepted {
            kind: "feat".to_owned(),
            breaking: true,
        },
        predictions: vec![prediction("p1", "1.0.3", Bump::Major, false)],
    };
    let body = render_check_comment(&input);
    assert!(
        body.contains("breaking"),
        "a breaking title is called out in the comment, got:\n{body}"
    );
}
