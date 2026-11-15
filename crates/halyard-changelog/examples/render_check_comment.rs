//! The §11.6 check-comment demo (M4.5 step 1): the verdict comment the
//! `check --pr-title --comment` mode posts to the PR under review,
//! rendered over a sample prediction set — both verdict shapes (the
//! accepted release prediction and the rejected diagnosis), the marker
//! that makes the forge seam's upsert idempotent, and the byte-identical
//! re-render (I3: the same verdict rewrites the same comment).
//!
//! Run: `cargo run -p halyard-changelog --example render_check_comment`

use halyard_changelog::{
    CHECK_MARKER, CheckComment, CheckPrediction, CheckVerdict, render_check_comment,
};
use halyard_core::Bump;

/// One planned package's predicted bump under the merged title (the
/// §10.2 pre-major remap rides the `remapped` flag).
fn prediction(package: &str, from: &str, bump: Bump, remapped: bool) -> CheckPrediction {
    CheckPrediction {
        package: package.to_owned(),
        from: from.to_owned(),
        bump,
        remapped,
    }
}

fn main() {
    // The accepted verdict: a `feat` title over a mixed workspace — the
    // pre-major package's conveyed minor remaps to patch (§10.2), and
    // the comment teaches that cause per row.
    let accepted = CheckComment {
        title: "feat(parser): accept spans".to_owned(),
        verdict: CheckVerdict::Accepted {
            kind: "feat".to_owned(),
            breaking: false,
        },
        predictions: vec![
            prediction("halyard-core", "0.4.1", Bump::Patch, true),
            prediction("halyard-cli", "1.0.3", Bump::Minor, false),
        ],
    };
    let body = render_check_comment(&accepted);
    println!("— accepted verdict (the PR comment) —\n{body}");
    assert!(
        body.starts_with(CHECK_MARKER),
        "the marker is the upsert's find key"
    );
    // I3: the same verdict re-renders byte-identically, so the marker
    // upsert rewrites the same comment in place on every push.
    assert_eq!(body, render_check_comment(&accepted));

    // The rejected verdict: the diagnosis rides the comment too (the
    // check teaches), then the command still fails with `E-CLI-TITLE`.
    let rejected = CheckComment {
        title: "updated the parser".to_owned(),
        verdict: CheckVerdict::Rejected {
            reason: "not a conventional commit: missing `type(scope): description`".to_owned(),
        },
        predictions: Vec::new(),
    };
    let rejected_body = render_check_comment(&rejected);
    println!("— rejected verdict (the PR comment) —\n{rejected_body}");
    assert!(rejected_body.starts_with(CHECK_MARKER));
}
