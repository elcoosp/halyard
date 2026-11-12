//! Bump application (§10.1/§10.2), commit-kind mapping (§10.1) and
//! prerelease channels (§10.3).
//!
//! Every operation is a pure function with saturating component
//! arithmetic: no panics, deterministic outputs.

use std::collections::BTreeMap;

use semver::{BuildMetadata, Prerelease, Version};

use halyard_core::Bump;

/// Pre-1.0 bump remapping (§10.2): what a breaking change and a `feat`
/// become while `major == 0`. Defaults replicate release-please's
/// `bump-minor-pre-major` / `bump-patch-for-minor-pre-major`; the default
/// here is Cargo-compatible (`0.y.z`: `y` is the breaking axis).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreMajorPolicy {
    /// Bump applied to breaking commits before 1.0 (default `minor`).
    pub breaking: Bump,
    /// Bump applied to `feat` commits before 1.0 (default `patch`).
    pub feat: Bump,
}

impl PreMajorPolicy {
    /// The Cargo-compatible default: breaking → minor, feat → patch.
    pub const CARGO_COMPATIBLE: Self = Self {
        breaking: Bump::Minor,
        feat: Bump::Patch,
    };

    /// Remaps a raw bump for a `0.y.z` version. Bumps on `1.y.z` versions
    /// pass through unchanged (see [`apply_bump`]).
    pub const fn remap(&self, raw: Bump) -> Bump {
        match raw {
            Bump::Major => self.breaking,
            Bump::Minor => self.feat,
            other => other,
        }
    }
}

/// Commit-type → bump mapping (§10.1, configurable through
/// `halyard.toml [commits.types]`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitBumpMapping {
    entries: BTreeMap<String, Bump>,
    unknown: Bump,
}

impl Default for CommitBumpMapping {
    fn default() -> Self {
        Self::new()
    }
}

impl CommitBumpMapping {
    /// Built-in mapping (§10.1): `feat` → minor, `fix`/`perf` → patch,
    /// unknown types → none.
    pub fn new() -> Self {
        let mut entries = BTreeMap::new();
        entries.insert("feat".to_owned(), Bump::Minor);
        entries.insert("fix".to_owned(), Bump::Patch);
        entries.insert("perf".to_owned(), Bump::Patch);
        Self {
            entries,
            unknown: Bump::None,
        }
    }

    /// Overrides the bump for one commit type (config layering applies
    /// this before planning).
    pub fn set(&mut self, kind: &str, bump: Bump) {
        self.entries.insert(kind.to_owned(), bump);
    }

    /// The raw bump for one commit: breaking commits are always
    /// [`Bump::Major`] (the 0.x remap happens in [`apply_bump`], which
    /// knows the version), other types follow the mapping.
    pub fn bump_for(&self, kind: &str, breaking: bool) -> Bump {
        if breaking {
            Bump::Major
        } else {
            self.entries.get(kind).copied().unwrap_or(self.unknown)
        }
    }
}

/// Applies a bump to a version (§5.5/§10.2): versions with `major == 0`
/// go through the pre-major policy; any bump drops the prerelease/build
/// suffixes ([`Bump::None`] keeps the version exactly as it is). Component
/// arithmetic saturates at `u64::MAX` instead of panicking.
pub fn apply_bump(version: &Version, bump: Bump, policy: &PreMajorPolicy) -> Version {
    if bump.is_none() {
        return version.clone();
    }
    let effective = if version.major == 0 {
        policy.remap(bump)
    } else {
        bump
    };
    let major = version.major;
    let minor = version.minor;
    let patch = version.patch;
    match effective {
        Bump::None => version.clone(),
        Bump::Patch => Version {
            major,
            minor,
            patch: patch.saturating_add(1),
            pre: Prerelease::EMPTY,
            build: BuildMetadata::EMPTY,
        },
        Bump::Minor => Version {
            major,
            minor: minor.saturating_add(1),
            patch: 0,
            pre: Prerelease::EMPTY,
            build: BuildMetadata::EMPTY,
        },
        Bump::Major => Version {
            major: major.saturating_add(1),
            minor: 0,
            patch: 0,
            pre: Prerelease::EMPTY,
            build: BuildMetadata::EMPTY,
        },
    }
}

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
    fn pre_major_remap_matches_documented_defaults() {
        let policy = PreMajorPolicy::CARGO_COMPATIBLE;
        assert_eq!(policy.remap(Bump::Major), Bump::Minor);
        assert_eq!(policy.remap(Bump::Minor), Bump::Patch);
        assert_eq!(policy.remap(Bump::Patch), Bump::Patch);
    }

    #[test]
    fn channel_validation() {
        assert!(PrereleaseChannel::new("beta").is_some());
        assert!(PrereleaseChannel::new("alpha-2").is_some());
        assert!(PrereleaseChannel::new("rc.").is_none());
        assert!(PrereleaseChannel::new("1").is_some());
    }

    #[test]
    fn default_mapping_matches_section_10_1() {
        let mapping = CommitBumpMapping::default();
        assert_eq!(mapping.bump_for("feat", false), Bump::Minor);
        assert_eq!(mapping.bump_for("fix", false), Bump::Patch);
        assert_eq!(mapping.bump_for("perf", false), Bump::Patch);
        assert_eq!(mapping.bump_for("ci", false), Bump::None);
    }
}
