//! Property tests (§15.1): requirement rewrites preserve the operator and
//! produce requirements that admit the new version; the bump algebra is
//! monotone and terminating.

use std::fmt::Write as _;

use halyard_core::Bump;
use halyard_version::{
    CommitBumpMapping, PreMajorPolicy, RewriteOutcome, UnchangedReason, UpperBoundPolicy,
    apply_bump, rewrite_requirement,
};
use proptest::prelude::*;
use semver::Version;

/// Deterministic version strategy: bounded components keep shrink useful.
fn version_strategy() -> impl Strategy<Value = Version> {
    (0u64..4, 0u64..10, 0u64..10, proptest::option::of(0u64..5)).prop_map(
        |(major, minor, patch, pre)| {
            let mut text = format!("{major}.{minor}.{patch}");
            if let Some(counter) = pre {
                let _ = write!(text, "-rc.{counter}");
            }
            // Static format ⇒ always valid; the `Version::new` fallback
            // keeps the strategy total without panicking (restriction
            // lints apply to non-`#[test]` helpers too).
            Version::parse(&text).unwrap_or_else(|_| Version::new(0, 0, 0))
        },
    )
}

/// Requirement shapes over a generated major/minor pair.
fn req_strategy() -> impl Strategy<Value = String> {
    (
        proptest::num::u64::ANY.prop_map(|m| m % 4),
        proptest::num::u64::ANY.prop_map(|m| m % 10),
        0usize..7,
    )
        .prop_map(|(major, minor, shape)| {
            let pair = format!("{major}.{minor}");
            let triple = format!("{major}.{minor}.0");
            match shape {
                0 => pair.clone(),
                1 => triple.clone(),
                2 => format!("^{pair}"),
                3 => format!("^{triple}"),
                4 => format!("~{triple}"),
                5 => format!("={triple}"),
                _ => format!(">={pair}, <{}", major + 1),
            }
        })
}

proptest::proptest! {
    #[test]
    fn rewritten_requirement_admits_new_version(
        req in req_strategy(),
        new in version_strategy(),
        widen in proptest::bool::ANY,
    ) {
        let policy = if widen { UpperBoundPolicy::Widen } else { UpperBoundPolicy::Error };
        match rewrite_requirement(&req, &new, policy) {
            Ok(RewriteOutcome::Rewritten(text)) => {
                let parsed = semver::VersionReq::parse(&text)
                    .unwrap_or_else(|e| panic!("rewritten {text:?} must parse: {e}"));
                prop_assert!(parsed.matches(&new), "rewritten {text:?} must admit {new}");
                prop_assert!(
                    !semver::VersionReq::parse(&req).unwrap_or_else(|e| panic!("req: {e}")).matches(&new),
                    "rewrite only happens when the original does not admit the new version"
                );
            }
            Ok(RewriteOutcome::Unchanged { because }) => {
                let parsed = semver::VersionReq::parse(&req)
                    .unwrap_or_else(|e| panic!("req {req:?}: {e}"));
                if because == UnchangedReason::Satisfied {
                    prop_assert!(parsed.matches(&new));
                }
            }
            Err(_) => {}
        }
    }

    #[test]
    fn rewritten_requirement_preserves_operator(
        req in req_strategy(),
        new in version_strategy(),
    ) {
        if let Ok(RewriteOutcome::Rewritten(text)) =
            rewrite_requirement(&req, &new, UpperBoundPolicy::Widen)
        {
            // Prerelease targets may legitimately introduce a `>=` floor
            // next to the widened bound (cargo prerelease matching), so
            // the operator check applies to stable targets only.
            prop_assume!(new.pre.is_empty());
            let original_op: String = req
                .chars()
                .take_while(|c| !c.is_ascii_digit() && *c != '*')
                .collect();
            let rewritten_op: String = text
                .chars()
                .take_while(|c| !c.is_ascii_digit() && *c != '*')
                .collect();
            prop_assert_eq!(rewritten_op, original_op, "operator style must be preserved");
        }
    }

    #[test]
    fn apply_bump_is_monotone(
        version in version_strategy(),
        bump in proptest::num::u8::ANY.prop_map(|raw| match raw % 4 {
            0 => Bump::None,
            1 => Bump::Patch,
            2 => Bump::Minor,
            _ => Bump::Major,
        }),
    ) {
        let policy = PreMajorPolicy::CARGO_COMPATIBLE;
        let next = apply_bump(&version, bump, &policy);
        prop_assert!(next >= version, "{next} must not go below {version}");
        if bump != Bump::None {
            prop_assert!(next > version, "{next} must be strictly above {version}");
            prop_assert!(next.pre.is_empty(), "bumping drops the prerelease suffix");
        }
    }

    #[test]
    fn bump_for_is_total_and_breaking_is_major(
        kind in "[a-z]{1,10}",
        breaking in prop::bool::ANY,
    ) {
        let mapping = CommitBumpMapping::new();
        let bump = mapping.bump_for(&kind, breaking);
        if breaking {
            prop_assert_eq!(bump, Bump::Major);
        } else {
            prop_assert!(matches!(bump, Bump::None | Bump::Patch | Bump::Minor | Bump::Major));
        }
    }
}
