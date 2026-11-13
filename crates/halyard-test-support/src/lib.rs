//! Test-support toolkit for HALYARD (§7.1): the [`GitFixture`] declarative
//! DSL for building deterministic git repositories, named fixture scenarios,
//! the mock forge, and (later milestones) golden-test helpers.
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
pub mod manifest_fixtures;
pub mod scenarios;

pub use error::{FixtureError, FixtureErrorKind};
pub use fixture::{CommitSpec, GitFixture};

// §7.1: the mock forge. The [`Forge`] trait and the deterministic
// [`MemoryForge`] reference adapter are defined in `halyard-forge`
// (ADR-0011); test-support is the fixture-facing home — E2E suites
// (M4.2's §15.1 lane) import forge and git fixtures from one crate.
pub use halyard_forge::{
    BranchName, CallRecord, Forge, ForgeError, MemoryForge, PrSpec, ReleaseSpec,
};
