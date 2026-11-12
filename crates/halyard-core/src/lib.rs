//! HALYARD pure core: types, the `halyard-plan/v1` contract and the planner.
//!
//! This crate is the functional core of the pipeline (§4 of
//! `docs/HALYARD_ARCHITECTURE.md`): **no I/O, no clocks, no environment
//! access**. Everything here is a pure function of its inputs so planning is
//! reproducible and testable in isolation.
//!
//! Layer rules (§7.2): this crate may only depend on `semver`, `smallvec`,
//! `bstr`, `blake3` and `serde` (plus `serde_json` for the canonical-JSON
//! plan hash per ADR-0005, and the feature-gated schema-generation
//! dependencies documented in `docs/adr/0001-schema-gen-deps-in-core.md`).
//!
//! Module map (M1.1):
//!
//! * [`id`] — `PkgId`, `Sym` and the interner (§5.1).
//! * [`bitset`] — `SmallBitset` (§5.7).
//! * [`package`] — `Workspace`, `Package`, `DepEdge`, groups, tag
//!   templates (§5.3/§6.6/A.3).
//! * [`reason`] — `Oid`, `Reason`, `ReasonChain`, `VersionResolution`
//!   (§5.5/§5.6).
//! * [`history`] — `RelevantCommit`, `HistoryDigest` (§5.7).
//! * [`canonical`] — canonical JSON + `plan_hash` (§5.8, I3).
//! * [`plan`] — the `halyard-plan/v1` wire format (§5.8).
#![forbid(unsafe_code)]

pub mod bitset;
pub mod canonical;
pub mod history;
pub mod id;
pub mod package;
pub mod plan;
pub mod reason;

pub use bitset::SmallBitset;
pub use canonical::{blake3_hex, canonical_json, plan_hash};
pub use history::{
    AuthorRef, BaseRef, CommitIdx, HistoryDigest, ParsedCommit, RelevantCommit, Trailer,
};
pub use id::{Interner, PkgId, Sym};
pub use package::{
    DepEdge, DepKind, Ecosystem, Group, GroupId, GroupPolicy, ManifestSite, Package, PathFilter,
    RelPath, ReqSpec, Span, TagTemplate, TagTemplateError, VersionSource, Workspace,
    WorkspaceProtocol,
};
pub use plan::{
    Bump, PLAN_SCHEMA_V1, Plan, PlanChangelog, PlanEdit, PlanPatch, PlanPublish, PlanReason,
    PlanRelease, PlanSkip,
};
pub use reason::{CascadeWhy, Oid, Reason, ReasonChain, VersionResolution};
