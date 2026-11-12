//! HALYARD version algebra (M1.3, §10): bump derivation and application,
//! the Cargo-compatible 0.x policy, prerelease channels and the
//! requirement rewriter of appendix A.2.
//!
//! Pure functions only: everything here takes versions in and returns
//! versions out, so the planner (M1.9) can compose them deterministically.
//!
//! Layering note (§7.2): this crate depends on `halyard-core` for the
//! `Bump` wire type; `halyard-core` never depends on this crate. Since
//! slice C (M1.9) the bump application / commit-mapping / requirement
//! rewriting *implementations* live in `halyard-core` (`bump` / `rewrite`
//! modules) so the planner can compose them; this crate re-exports every
//! public path unchanged (ADR-0008) and owns the release-time prerelease
//! channels.

#![forbid(unsafe_code)]

mod algebra;
mod rewrite;

pub use algebra::{
    CommitBumpMapping, PreMajorPolicy, PrereleaseChannel, advance_prerelease, apply_bump,
    promote_to_stable,
};
pub use halyard_core::Bump;
pub use rewrite::{
    RewriteOutcome, UnchangedReason, UnrewritableReason, UpperBoundPolicy, VersionError,
    rewrite_requirement,
};
