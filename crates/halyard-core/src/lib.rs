//! HALYARD pure core: types, the `halyard-plan/v1` contract and the planner.
//!
//! This crate is the functional core of the pipeline (§4 of
//! `docs/HALYARD_ARCHITECTURE.md`): **no I/O, no clocks, no environment
//! access**. Everything here is a pure function of its inputs so planning is
//! reproducible and testable in isolation.
//!
//! Layer rules (§7.2): this crate may only depend on `semver`, `smallvec`,
//! `bstr`, `blake3` and `serde` (plus the feature-gated schema-generation
//! dependencies documented in `docs/adr/0001-schema-gen-deps-in-core.md`).

pub mod plan;

pub use plan::Plan;
