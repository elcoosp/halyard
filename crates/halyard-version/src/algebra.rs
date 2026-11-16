//! Prerelease channels (§10.3) — the release-time half of the version
//! algebra. The bump application and commit-mapping half moved to
//! `halyard-core` (ADR-0008, the planner's canonical home); the channel
//! half joined it there (ADR-0025, the §10.3 planner integration), and
//! this module re-exports both unchanged so every M1.3 call site keeps
//! compiling.

pub use halyard_core::bump::{
    CommitBumpMapping, PreMajorPolicy, PrereleaseChannel, advance_prerelease, apply_bump,
    promote_to_stable,
};

#[cfg(test)]
mod tests {
    use super::*;
    use semver::Version;

    #[test]
    fn channel_validation() {
        assert!(PrereleaseChannel::new("beta").is_some());
        assert!(PrereleaseChannel::new("alpha-2").is_some());
        assert!(PrereleaseChannel::new("rc.").is_none());
        assert!(PrereleaseChannel::new("1").is_some());
    }

    #[test]
    fn advance_counts_existing_tags_through_the_reexport() {
        // The ADR-0025 re-export path: the §10.3 behavior contract must
        // hold through `halyard_version`'s public surface.
        let channel = PrereleaseChannel::new("rc").unwrap_or_else(|| unreachable!("rc"));
        let v = Version::parse("1.3.0-rc.1")
            .map_err(|_| ())
            .unwrap_or(Version::new(1, 3, 0));
        let next = advance_prerelease(&Version::new(1, 3, 0), &channel, std::slice::from_ref(&v));
        assert_eq!(next.to_string(), "1.3.0-rc.2");
        let promoted = promote_to_stable(&v);
        assert_eq!(promoted.to_string(), "1.3.0");
    }

    #[test]
    fn reexported_algebra_matches_documented_defaults() {
        // The ADR-0008 re-export path: the M1.3 behavior contract must
        // hold through `halyard_version`'s public surface.
        let policy = PreMajorPolicy::CARGO_COMPATIBLE;
        assert_eq!(
            policy.remap(halyard_core::Bump::Major),
            halyard_core::Bump::Minor
        );
        let mapping = CommitBumpMapping::default();
        assert_eq!(mapping.bump_for("feat", false), halyard_core::Bump::Minor);
        let v = Version::parse("0.4.1")
            .map_err(|_| ())
            .unwrap_or(Version::new(0, 4, 1));
        // Cargo-compatible pre-major remap: Minor → Patch on 0.y.z.
        assert_eq!(
            apply_bump(&v, halyard_core::Bump::Minor, &policy).to_string(),
            "0.4.2"
        );
    }
}
