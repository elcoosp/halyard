//! Golden vectors for the version crate (M1.3).
//!
//! The requirement-rewrite table is appendix A.2 verbatim, plus operator,
//! precision and policy variants; the bump algebra covers §10.1/§10.2
//! (Cargo-compatible 0.x defaults) and §10.3 (prerelease channels).

use halyard_core::Bump;
use halyard_version::{
    CommitBumpMapping, PreMajorPolicy, PrereleaseChannel, RewriteOutcome, UnchangedReason,
    UpperBoundPolicy, VersionError, advance_prerelease, apply_bump, promote_to_stable,
    rewrite_requirement,
};
use semver::Version;

fn v(text: &str) -> Version {
    // Fixture strings are static; a bad fixture surfaces as a failed
    // assertion below, so the fallback keeps this helper panic-free
    // (restriction lints cover non-`#[test]` helpers).
    Version::parse(text).unwrap_or_else(|_| Version::new(0, 0, 0))
}

fn outcome_text(outcome: RewriteOutcome) -> String {
    match outcome {
        RewriteOutcome::Unchanged { because } => format!("unchanged({because:?})"),
        RewriteOutcome::Rewritten(text) => format!("rewritten({text})"),
    }
}

#[test]
fn appendix_a2_rows() {
    let cases: &[(&str, &str, UpperBoundPolicy, &str)] = &[
        ("0.4", "0.5.0", UpperBoundPolicy::Widen, "rewritten(0.5)"),
        (
            "^0.4.1",
            "0.5.0",
            UpperBoundPolicy::Widen,
            "rewritten(^0.5.0)",
        ),
        (
            "=0.4.1",
            "0.4.2",
            UpperBoundPolicy::Widen,
            "rewritten(=0.4.2)",
        ),
        (
            "~1.2.3",
            "1.3.0",
            UpperBoundPolicy::Widen,
            "rewritten(~1.3.0)",
        ),
        (
            ">=1.0, <2",
            "1.4.0",
            UpperBoundPolicy::Widen,
            "unchanged(Satisfied)",
        ),
        (
            ">=1.0, <2",
            "2.0.0",
            UpperBoundPolicy::Widen,
            "rewritten(>=2.0, <3)",
        ),
        (
            ">=1.0, <2",
            "2.0.0",
            UpperBoundPolicy::Error,
            "E-EDIT-UNREWRITABLE",
        ),
        (
            "workspace:^",
            "9.9.9",
            UpperBoundPolicy::Widen,
            "unchanged(Protocol)",
        ),
    ];
    for (req, target, policy, expected) in cases {
        let result = rewrite_requirement(req, &v(target), *policy);
        let text = match &result {
            Ok(outcome) => outcome_text(outcome.clone()),
            Err(err) => {
                assert_eq!(
                    *expected, "E-EDIT-UNREWRITABLE",
                    "unexpected error for {req}"
                );
                assert_eq!(err.code(), VersionError::CODE);
                continue;
            }
        };
        assert_eq!(text, *expected, "req {req:?} → {target}");
    }
}

#[test]
fn protocol_and_star_requirements_are_never_rewritten() {
    let target = v("9.9.9");
    for req in [
        "workspace:*",
        "workspace:^",
        "workspace:~",
        "workspace:1.2.3",
        "catalog:",
        "catalog:react",
        "link:./vendor",
        "file:../sibling",
        "*",
    ] {
        let outcome = rewrite_requirement(req, &target, UpperBoundPolicy::Widen)
            .unwrap_or_else(|e| unreachable!("{req} must not error: {e}"));
        let because = match outcome {
            RewriteOutcome::Unchanged { because } => because,
            other @ RewriteOutcome::Rewritten(_) => {
                unreachable!("{req} must be unchanged, got {other:?}")
            }
        };
        assert_eq!(
            because,
            if req == "*" {
                UnchangedReason::Any
            } else {
                UnchangedReason::Protocol
            }
        );
    }
}

#[test]
fn single_comparator_rewrites_preserve_operator_and_precision() {
    let cases: &[(&str, &str, &str)] = &[
        // (req, new version, expected rewritten text)
        ("^1.2.3", "2.0.0", "^2.0.0"),
        ("^1.2", "2.0.0", "^2.0"),
        ("^0.9", "0.10.0", "^0.10"),
        ("~1.2", "1.4.0", "~1.4"),
        ("1.2.3", "2.0.0", "2.0.0"),
        ("1.2", "2.0.0", "2.0"),
        ("=1.2", "1.4.0", "=1.4"),
        ("1.2.*", "1.4.0", "1.4.*"),
        ("1.*", "2.0.0", "2.*"),
        (">=1.0", "0.5.0", ">=0.5"),
        ("<2", "2.0.0", "<3"),
        ("<2.0", "2.0.0", "<3.0"),
        (">=1.0, <1.5", "1.6.0", ">=1.6, <2"),
        // prerelease requirements keep the new prerelease when present
        ("^0.9.0", "1.0.0-rc.1", "^1.0.0-rc.1"),
    ];
    for (req, target, expected) in cases {
        let outcome = rewrite_requirement(req, &v(target), UpperBoundPolicy::Widen)
            .unwrap_or_else(|e| unreachable!("{req} must rewrite: {e}"));
        match outcome {
            RewriteOutcome::Rewritten(text) => assert_eq!(&text, expected, "req {req:?}"),
            other @ RewriteOutcome::Unchanged { .. } => {
                unreachable!("{req} expected rewritten, got {other:?}")
            }
        }
    }
}

#[test]
fn unrewritable_shapes_error() {
    let target = v("2.0.0");
    for req in ["<=1.9", "!=1.0", ">1.0, <=1.9", "not a requirement at all"] {
        let err = rewrite_requirement(req, &target, UpperBoundPolicy::Widen)
            .err()
            .unwrap_or_else(|| unreachable!("{req} must fail"));
        assert_eq!(err.code(), VersionError::CODE, "req {req:?}");
        assert_eq!(err.code(), "E-EDIT-UNREWRITABLE");
    }
}

#[test]
fn bump_algebra_cargo_compatible_pre_major() {
    let policy = PreMajorPolicy::CARGO_COMPATIBLE;
    // 0.x: breaking → minor, feat → patch (§10.2 Cargo-compatible default).
    assert_eq!(apply_bump(&v("0.4.1"), Bump::Major, &policy), v("0.5.0"));
    assert_eq!(apply_bump(&v("0.4.1"), Bump::Minor, &policy), v("0.4.2"));
    assert_eq!(apply_bump(&v("0.4.1"), Bump::Patch, &policy), v("0.4.2"));
    // 1.x: conventional semantics.
    assert_eq!(apply_bump(&v("1.2.3"), Bump::Major, &policy), v("2.0.0"));
    assert_eq!(apply_bump(&v("1.2.3"), Bump::Minor, &policy), v("1.3.0"));
    assert_eq!(apply_bump(&v("1.2.3"), Bump::Patch, &policy), v("1.2.4"));
    assert_eq!(apply_bump(&v("1.2.3"), Bump::None, &policy), v("1.2.3"));
    // Prerelease bumps drop the suffix (release promotion by bump).
    assert_eq!(
        apply_bump(&v("1.2.3-rc.2"), Bump::Patch, &policy),
        v("1.2.4")
    );
    // Custom pre-major mapping.
    let aggressive = PreMajorPolicy {
        breaking: Bump::Major,
        feat: Bump::Minor,
    };
    assert_eq!(
        apply_bump(&v("0.4.1"), Bump::Major, &aggressive),
        v("1.0.0")
    );
    assert_eq!(
        apply_bump(&v("0.4.1"), Bump::Minor, &aggressive),
        v("0.5.0")
    );
}

#[test]
fn bump_for_commit_kinds() {
    let mut mapping = CommitBumpMapping::new();
    assert_eq!(mapping.bump_for("feat", false), Bump::Minor);
    assert_eq!(mapping.bump_for("fix", false), Bump::Patch);
    assert_eq!(mapping.bump_for("perf", false), Bump::Patch);
    assert_eq!(mapping.bump_for("docs", false), Bump::None);
    assert_eq!(mapping.bump_for("whatever", false), Bump::None);
    assert_eq!(mapping.bump_for("chore", true), Bump::Major);
    mapping.set("docs", Bump::Patch);
    assert_eq!(mapping.bump_for("docs", false), Bump::Patch);
}

#[test]
fn prerelease_channels() {
    let channel = PrereleaseChannel::new("rc").unwrap_or_else(|| unreachable!("rc is valid"));
    assert_eq!(
        advance_prerelease(&v("1.2.0"), &channel, &[]),
        v("1.2.0-rc.1")
    );
    let existing = [v("1.2.0-rc.1"), v("1.2.0-rc.2"), v("1.1.0-rc.9")];
    assert_eq!(
        advance_prerelease(&v("1.2.0"), &channel, &existing),
        v("1.2.0-rc.3"),
        "counter derived from same-base tags only"
    );
    assert_eq!(
        advance_prerelease(&v("1.3.0"), &channel, &existing),
        v("1.3.0-rc.1"),
        "new base resets the counter"
    );
    assert_eq!(promote_to_stable(&v("1.2.0-rc.3")), v("1.2.0"));
    assert!(PrereleaseChannel::new("").is_none());
    assert!(PrereleaseChannel::new("-rc").is_none());
    assert!(PrereleaseChannel::new("rc-").is_none());
    assert!(PrereleaseChannel::new("r c").is_none());
    assert!(PrereleaseChannel::new("next").is_some());
}
