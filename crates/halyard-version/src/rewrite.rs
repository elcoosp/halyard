//! The appendix-A.2 requirement rewriter (§10.4) — re-exported from
//! `halyard-core` (ADR-0008: the planner composes it directly; the
//! canonical implementation lives next to the plan types it feeds).
//! Every M1.3 public path is preserved.

pub use halyard_core::rewrite::{
    rewrite_requirement, RewriteOutcome, UnchangedReason, UnrewritableReason, UpperBoundPolicy,
    VersionError,
};
