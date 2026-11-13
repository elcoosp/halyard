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
//! Module map (M1.1 foundation, M1.5–M1.7 discovery, M1.9 planner):
//!
//! * [`id`] — `PkgId`, `Sym` and the interner (§5.1).
//! * [`bitset`] — `SmallBitset` (§5.7).
//! * [`package`] — `Workspace`, `Package`, `DepEdge`, groups, tag
//!   templates (§5.3/§6.6/A.3).
//! * [`reason`] — `Oid`, `Reason`, `ReasonChain`, `VersionResolution`
//!   (§5.5/§5.6).
//! * [`history`] — `RelevantCommit`, `HistoryDigest`, the
//!   `HistoryProvider` seam and `ReleaseRefs` (§5.7, §8.2).
//! * [`canonical`] — canonical JSON + `plan_hash` (§5.8, I3).
//! * [`plan`] — the `halyard-plan/v1` wire format (§5.8).
//! * [`bump`] — bump application + commit mapping, canonical (ADR-0008).
//! * [`rewrite`] — the appendix-A.2 requirement rewriter (ADR-0008).
//! * [`planner`] — the pure `plan()` (M1.9, §4, A.1; ADR-0007).
//! * [`glob`] — member-pattern matcher shared by the adapters (§6.2).
//! * [`discover`] — the `EcosystemAdapter` seam, `DiscoverCtx`,
//!   `RawPackage` and `E-DISC-*` errors (§6, §7.3).
//! * [`edit`] — the adapter edit seam outcomes/failures (§7.3 edit
//!   subset, M3.1/M3.3).
//! * [`lockfile`] — the adapter lockfile seam outcomes/changes/failures
//!   (§7.3 lockfile subset, §9.2, M3.2).
//! * [`graph`] — CSR adjacency, topological order, SCCs, `E-GRAPH-CYCLE`
//!   with the dev-cycle exemption (§6.6, M1.7).
//! * [`link`] — polyglot link rules (§6.5, M1.7).
#![forbid(unsafe_code)]

pub mod bitset;
pub mod bump;
pub mod canonical;
pub mod discover;
pub mod edit;
pub mod glob;
pub mod graph;
pub mod history;
pub mod id;
pub mod link;
pub mod lockfile;
pub mod package;
pub mod plan;
pub mod planner;
pub mod reason;
pub mod rewrite;
pub mod trie;

pub use bitset::SmallBitset;
pub use bstr::BString;
pub use canonical::{blake3_hex, canonical_json, plan_hash};
pub use discover::{
    DiscoverCtx, DiscoverError, DiscoveryWarning, EcosystemAdapter, Facets, ManifestFormat, RawDep,
    RawPackage, RawVersionSource,
};
pub use edit::{EditFailure, EditOutcome};
pub use graph::{GraphError, WorkspaceGraph};
pub use history::{
    AuthorRef, BaseRef, CommitIdx, FixedHistory, HistoryDigest, HistoryProvider, ParsedCommit,
    ReleaseRefs, RelevantCommit, Trailer,
};
pub use id::{Interner, PkgId, Sym};
pub use link::{ExplicitLinkSpec, LinkEdge, LinkMode, LinkRule, detect_links};
pub use lockfile::{LockfileFailure, LockfileOutcome, LockfileVersionChange, NativeRefreshError};
pub use package::{
    DepEdge, DepKind, Ecosystem, Group, GroupId, GroupPolicy, ManifestSite, Package, PathFilter,
    RelPath, ReqSpec, Span, TagTemplate, TagTemplateError, VersionSource, Workspace,
    WorkspaceProtocol,
};
pub use plan::{
    Bump, PLAN_SCHEMA_V1, Plan, PlanChangelog, PlanEdit, PlanPatch, PlanPublish, PlanReason,
    PlanRelease, PlanSkip,
};
pub use planner::{CascadeMode, GroupMode, PlanConfig, PlanError, Snapshot, plan};
pub use reason::{CascadeWhy, Oid, Reason, ReasonChain, VersionResolution};
pub use rewrite::{
    RewriteOutcome, UnchangedReason, UnrewritableReason, UpperBoundPolicy, VersionError,
    rewrite_requirement,
};
pub use trie::{Trie, TrieNode, build_trie, layout_hash};
