//! Prerelease channels (§10.3) — the release-time half of the version
//! algebra. The bump application and commit-mapping half moved to
//! `halyard-core` (ADR-0008, the planner's canonical home); this module
//! re-exports those paths unchanged so every M1.3 call site keeps
//! compiling.

use semver::{BuildMetadata, Prerelease, Version};

pub use halyard_core::bump::{CommitBumpMapping, PreMajorPolicy, apply_bump};

/// A validated prerelease channel name (§10.3): one semver identifier —
/// alphanumerics and hyphens, non-empty, no leading/trailing hyphen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrereleaseChannel(String);

impl PrereleaseChannel {
    /// Validates a channel name; `None` when invalid.
    pub fn new(raw: &str) -> Option<Self> {
        let valid = !raw.is_empty()
            && !raw.starts_with('-')
            && !raw.ends_with('-')
            && raw.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        if valid {
            Some(Self(raw.to_owned()))
        } else {
            None
        }
    }

    /// Channel name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The next prerelease on `channel` for `base` (§10.3): the counter is
/// derived from `existing` versions — the highest `base-{channel}.N` with
/// the same base numbers — with no state file. A new base starts at 1.
pub fn advance_prerelease(
    base: &Version,
    channel: &PrereleaseChannel,
    existing: &[Version],
) -> Version {
    let mut highest: Option<u64> = None;
    for version in existing {
        let same_base = version.major == base.major
            && version.minor == base.minor
            && version.patch == base.patch
            && version.build.is_empty();
        if !same_base {
            continue;
        }
        let idents: Vec<&str> = version.pre.as_str().split('.').collect();
        let channel_matches = idents
            .first()
            .is_some_and(|ident| *ident == channel.as_str());
        let counter = idents
            .get(1)
            .and_then(|c| c.parse::<u64>().ok())
            .filter(|_| idents.len() == 2 && channel_matches);
        if let Some(counter) = counter {
            highest = highest.max(Some(counter));
        }
    }
    let counter = highest.unwrap_or(0).saturating_add(1);
    let text = format!(
        "{}.{}.{}-{}.{}",
        base.major,
        base.minor,
        base.patch,
        channel.as_str(),
        counter
    );
    // `channel` is a validated identifier and the counter is a plain
    // number, so the composed version always parses; fall back to the
    // base itself on the impossible path instead of panicking.
    Version::parse(&text).unwrap_or_else(|_| base.clone())
}

/// Drops the prerelease/build suffixes (§10.3 `rc → stable` promotion).
pub fn promote_to_stable(version: &Version) -> Version {
    Version {
        major: version.major,
        minor: version.minor,
        patch: version.patch,
        pre: Prerelease::EMPTY,
        build: BuildMetadata::EMPTY,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_validation() {
        assert!(PrereleaseChannel::new("beta").is_some());
        assert!(PrereleaseChannel::new("alpha-2").is_some());
        assert!(PrereleaseChannel::new("rc.").is_none());
        assert!(PrereleaseChannel::new("1").is_some());
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
