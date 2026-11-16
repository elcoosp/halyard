//! HALYARD configuration (`halyard.toml`, §16.4 of
//! `docs/HALYARD_ARCHITECTURE.md`).
//!
//! The frozen v1 surface (`model`, M0.7) is what
//! `schemas/halyard-config-v1.json` is generated from. On top of it the
//! M1.8 loader adds:
//!
//! * strict unknown-key detection with did-you-mean suggestions
//!   ([`loader`], `E-CFG-UNKNOWN-KEY`);
//! * layered resolution (built-in defaults < `halyard.toml` <
//!   `package.metadata.halyard` < env `HALYARD_*` < CLI flags) into a
//!   fully materialized [`ResolvedConfig`]
//!   ([`resolve`](resolve::resolve));
//! * the deterministic `config_hash` (blake3 over canonical JSON) that
//!   feeds the cache key (§8.7, invariant I3).
#![forbid(unsafe_code)]

pub mod error;
pub mod init;
pub mod loader;
pub mod migrate;
pub mod model;
pub mod resolve;

pub use error::ConfigError;
pub use loader::{load, load_from_str};
pub use model::HalyardConfig;
pub use resolve::{
    Layer, ResolvedConfig, TagStyleSetting, config_hash, env_layer, env_layer_from, resolve,
};

// M4.7 (`migrate`): the foreign release-tool migration seam, its report
// data model and the §16.2 write step (the planner + the injected
// `DraftSink` the shell wires). The registered sources cover the four
// §16.2 tools: release-please, changesets, knope and release-plz.
pub use crate::migrate::{
    AtomicDraftSink, ChangesetsSource, DRAFT_TARGET, DraftSink, DraftWriteOutcome, DraftWritePlan,
    DraftWriteReport, ForeignFiles, KnopeSource, MappingRow, MemoryDraftSink, MigrationReport,
    MigrationSource, PackageSurvey, ReleasePleaseSource, ReleasePlzSource, ToolSurvey, UnmappedRow,
    default_sources, migrate, plan_draft_write, run_draft_write,
};

// ADR-0029 (`init`, §16.1/§16.2): the zero-config scaffold engine — the
// survey-driven create-only write plan (config skeleton + optional
// §16.1 workflow) over the same injected `DraftSink` seam, with the
// directory-creating production sink.
pub use crate::init::{
    AtomicScaffoldSink, INIT_CONFIG_TARGET, INIT_WORKFLOW_TARGET, InitFileReport, InitPlan,
    InitReport, InitSurvey, ScaffoldFile, plan_init, run_init,
};

// Re-exported for the CLI (M1.10) and adapter consumption.
pub use crate::model::{
    BumpLevel, CargoPublishCfg, CascadePolicy, ChangelogCfg, CommitMode, CommitsCfg, ForgeCfg,
    ForgeKind, InitialReleaseCfg, LinkCfg, LinkMode, MergePolicy, NpmAccess, NpmPublishCfg,
    PackageOverride, PreMajorCfg, PublishCfg, RepoCfg, TagStyle, VersionPolicy,
};
