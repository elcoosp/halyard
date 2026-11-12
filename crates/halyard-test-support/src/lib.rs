//! Test-support toolkit for HALYARD (§7.1): the [`GitFixture`] declarative
//! DSL for building deterministic git repositories, named fixture scenarios,
//! and (later milestones) mock forges and golden-test helpers.
//!
//! Everything here is **test infrastructure**: it shells out to the `git`
//! binary via argv arrays (never a shell string, invariant I7) with a fully
//! isolated configuration so fixture builds are reproducible byte-for-byte —
//! identical scenario + content produce identical commit OIDs on every
//! platform.
//!
//! Fixture errors carry the stable code `E-TEST-FIXTURE`
//! (see `docs/errors/E-TEST-FIXTURE.md`).

pub mod error;
pub mod fixture;
pub mod scenarios;

pub use error::{FixtureError, FixtureErrorKind};
pub use fixture::{CommitSpec, GitFixture};
