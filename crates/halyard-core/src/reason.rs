//! Semantic bump reasons (§5.5, §5.6).
//!
//! These types power `halyard explain` and feed the wire format when a plan
//! is emitted (M1.9): each [`Reason`] maps 1:1 onto a
//! [`crate::plan::PlanReason`] variant.

use bstr::BString;
use semver::Version;

use crate::id::PkgId;
use crate::package::DepKind;

/// Git object id stored as lowercase hex bytes (40 hex for SHA-1, 64 for
/// SHA-256). The git engine (M2) maps `gix` object ids onto this type.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Oid(BString);

impl Oid {
    /// Parses a hex object id (upper- or lowercase accepted; stored
    /// lowercase). `None` when the bytes are not exactly 40 or 64 hex
    /// digits.
    pub fn from_hex(raw: &[u8]) -> Option<Self> {
        let valid_len = raw.len() == 40 || raw.len() == 64;
        if valid_len && raw.iter().all(u8::is_ascii_hexdigit) {
            Some(Self(BString::from(raw.to_ascii_lowercase())))
        } else {
            None
        }
    }

    /// Hex bytes (lowercase).
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Hex as `&str`. Infallible in practice: `from_hex` validated the
    /// bytes as ASCII hex, so a non-UTF-8 payload is unreachable.
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or_default()
    }
}

impl std::fmt::Display for Oid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a cascade fired (§10.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CascadeWhy {
    /// The dependent's requirement no longer admits the new version.
    RequirementUnsatisfied,
    /// `cascade = "always"` for normal deps of publishable crates.
    PolicyAlways,
    /// The packages share a lockstep group (§5.4).
    LockstepGroup,
}

impl CascadeWhy {
    /// Wire keyword used by `halyard-plan/v1` cascade reasons.
    pub const fn wire_key(self) -> &'static str {
        match self {
            Self::RequirementUnsatisfied => "requirement_unsatisfied",
            Self::PolicyAlways => "policy_always",
            Self::LockstepGroup => "lockstep_group",
        }
    }
}

/// One entry of a reason chain (§5.6). Wire counterpart:
/// [`crate::plan::PlanReason`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    /// A parsed conventional commit contributed to the bump.
    Commit {
        /// Commit object id.
        oid: Oid,
        /// Commit type as parsed (`feat`, `fix`, …; open set).
        kind: BString,
        /// Whether the commit was breaking (`!` or `BREAKING CHANGE`).
        breaking: bool,
    },
    /// A `Release-As: <version>` footer forced this version (honoured only
    /// from commits on the protected base branch, invariant I7).
    ReleaseAs {
        /// Commit carrying the footer.
        oid: Oid,
        /// Forced version.
        version: Version,
    },
    /// A change file (`.halyard/changes/*.md`) contributed the bump (§9.4).
    Changeset {
        /// Repo-relative path of the change file.
        file: BString,
    },
    /// The bump cascaded from a dependency (§10.4).
    Cascade {
        /// The upstream package that was bumped.
        via: PkgId,
        /// The dependency edge kind connecting the packages.
        edge: DepKind,
        /// Why the cascade fired.
        why: CascadeWhy,
    },
    /// The package has no release base yet; this is its initial release.
    InitialRelease,
}

/// Ordered list of reasons behind one version decision (§5.6).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReasonChain(Vec<Reason>);

impl ReasonChain {
    /// Empty chain.
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Appends a reason (order is the planner's emit order; never sorted).
    pub fn push(&mut self, reason: Reason) {
        self.0.push(reason);
    }

    /// Number of reasons.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True when no reason was recorded.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Reasons in recorded order.
    pub fn iter(&self) -> std::slice::Iter<'_, Reason> {
        self.0.iter()
    }
}

impl<'chain> IntoIterator for &'chain ReasonChain {
    type Item = &'chain Reason;
    type IntoIter = std::slice::Iter<'chain, Reason>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// Result of resolving one package's next version (§5.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionResolution {
    /// Current version in the manifest.
    pub from: Version,
    /// Next version after applying the bump.
    pub to: Version,
    /// Bump level derived from commits, policy and cascade.
    pub bump: crate::plan::Bump,
    /// Why this resolution happened (powers `explain`).
    pub reason: ReasonChain,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oid_accepts_both_hash_lengths_and_normalizes_case() {
        let sha1 = Oid::from_hex(b"ABCDEF0123456789ABCDEF0123456789ABCDEF01");
        assert!(sha1.is_some());
        let sha1 = sha1.unwrap();
        assert_eq!(sha1.as_str(), "abcdef0123456789abcdef0123456789abcdef01");
        assert!(Oid::from_hex(&[b'0'; 39]).is_none());
        assert!(Oid::from_hex(&[b'0'; 65]).is_none());
        assert!(Oid::from_hex(&[b'g'; 40]).is_none());
    }
}
