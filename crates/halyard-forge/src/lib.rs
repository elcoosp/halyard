//! The Forge seam (§11.4, M4.1a): the forge-agnostic contract behind the
//! Release-PR state machine (§11.1) — GitHub is an adapter, not an
//! assumption (invariant I9).
//!
//! The seam is **pure vocabulary**: the [`Forge`] trait, its
//! request/response types ([`spec`]) and the failure taxonomy
//! ([`error::ForgeError`], §15.4 `E-FORGE-*`). No HTTP, no clock, no
//! environment reads — the effectful REST adapters land with the rest of
//! M4.1, and the §7.2 layer rule keeps this crate on `halyard-core` only.
//!
//! The [`MemoryForge`] reference adapter models the semantics the §11.1
//! state machine relies on (find-by-branch upserts, GET-by-tag-first
//! release creation, synthetic API commits, open/closed PR state) and
//! doubles as the §7.1 mock forge: its call log backs the §11.5 "≤ 2 calls
//! on the update path" budget assertion, and its failpoint harness is the
//! §15.2 failure injector — first consumed by the machine itself
//! ([`state`], M4.2a).
//!
//! ```no_run
//! use halyard_forge::{BranchName, Forge, MemoryForge, PrSpec};
//!
//! let forge = MemoryForge::new();
//! let branch = BranchName::new("halyard/release/main").unwrap();
//! let pr = forge.upsert_pr(&PrSpec {
//!     branch: branch.clone(),
//!     title: "chore: release v0.1.0".to_owned(),
//!     body: "## 🚀 Release plan".to_owned(),
//! }).unwrap();
//! // Idempotent (I1): the re-run re-finds and rewrites in place.
//! let again = forge.upsert_pr(&PrSpec {
//!     branch,
//!     title: "chore: release v0.1.0".to_owned(),
//!     body: "## 🚀 Release plan\n\n| core | 0.1.0 | 0.1.1 |".to_owned(),
//! }).unwrap();
//! assert_eq!(pr.number, again.number);
//! ```
//!
//! # Status
//!
//! M4.1a ships the seam + reference adapter; M4.2a ships the §11.1
//! machine over it ([`state::run_release_machine`]) with the
//! [`Forge::close_pr`] capability; M4.2b step 1 ships the dry-run
//! adapter ([`PreviewForge`]) the shell's `pr --dry` forecast runs on.
//! The remaining M4.1b slice is the GitHub REST adapter (`ETag`
//! conditionals, backoff, App-JWT auth) — the nightly `forge-smoke` job
//! stays latent until it exists.

#![forbid(unsafe_code)]

pub mod error;
pub mod memory;
pub mod preview;
pub mod spec;
pub mod state;

pub use error::ForgeError;
pub use memory::{CallRecord, MemoryForge};
pub use preview::PreviewForge;
pub use spec::{ApiCommit, ApiFile, BranchName, Pr, PrSpec, RateInfo, Release, ReleaseSpec};
pub use state::{MachineEffect, MachineInput, MachineReport, run_release_machine};

/// The forge contract (§11.4): every operation the Release-PR state
/// machine (§11.1) performs against a forge. Object-safe — adapters are
/// handed around as `&dyn Forge` behind the shell's layering (§7.2).
///
/// Not-found outcomes are values, never errors: a missing Release PR is
/// [`None`], a missing release is `false`. The [`ForgeError`] set is
/// reserved for transport, auth,
/// budget and conflict failures.
///
/// Idempotence (I1) is the adapter's obligation: `upsert_pr` re-finds by
/// branch and rewrites in place; `create_release` checks by tag first and
/// reports the existing release (`Release::created == false`).
pub trait Forge: Send + Sync {
    /// Finds the open Release PR on `branch` (ETag-conditional in real
    /// adapters; ≤ 1 call, §11.4).
    ///
    /// # Errors
    ///
    /// Transport/auth/budget failures as [`ForgeError`] (`E-FORGE-*`).
    fn find_release_pr(&self, branch: &BranchName) -> Result<Option<Pr>, ForgeError>;

    /// Creates the Release PR on `spec.branch` or rewrites the existing
    /// one in place — same branch ⇒ same PR, no duplicates (I1).
    ///
    /// # Errors
    ///
    /// [`ForgeError`] (`E-FORGE-*`); [`ForgeError::Request`] for a spec
    /// the forge would reject.
    fn upsert_pr(&self, spec: &PrSpec) -> Result<Pr, ForgeError>;

    /// Whether `tag` already has a forge release.
    ///
    /// # Errors
    ///
    /// [`ForgeError`] (`E-FORGE-*`).
    fn release_exists(&self, tag: &str) -> Result<bool, ForgeError>;

    /// Creates the forge release for `spec.tag` with `spec.notes`;
    /// idempotent via GET-by-tag-first (§11.1 step 2) — the re-run
    /// returns the existing release with
    /// [`Release::created == false`](spec::Release::created).
    ///
    /// # Errors
    ///
    /// [`ForgeError`] (`E-FORGE-*`); [`ForgeError::Request`] for an empty
    /// tag.
    fn create_release(&self, spec: &ReleaseSpec) -> Result<Release, ForgeError>;

    /// Closes the open Release PR on `branch` (§11.1 step 3: the plan is
    /// empty and a stale Release PR must go). Returns `false` when no
    /// open PR exists on the branch — a not-found outcome is a value,
    /// never an error (§11.4), and the re-close is a no-op (I1). A closed
    /// PR is never reused: a later [`Forge::upsert_pr`] on the same
    /// branch opens a fresh one, because real forges cannot reopen a
    /// closed PR under its number.
    ///
    /// The trait default is the documented honest decline
    /// ([`ForgeError::Unsupported`], `E-FORGE-UNSUPPORTED`), mirroring
    /// [`Forge::commit_via_api`]'s default and the lockfile seam's
    /// default-decline convention (§7.3): forges without a close
    /// endpoint name themselves and the §11.1 machine surfaces the
    /// decline fail-closed instead of guessing.
    ///
    /// # Errors
    ///
    /// [`ForgeError::Unsupported`] by default; adapter-specific failures
    /// otherwise.
    fn close_pr(&self, branch: &BranchName) -> Result<bool, ForgeError> {
        let _ = branch;
        Err(ForgeError::unsupported(std::any::type_name::<Self>()))
    }

    /// Creates a commit through the forge's Git Data API so the forge
    /// signs it (`commit_mode = "api"`, §11.5: "Verified" commits). The
    /// returned [`Oid`](halyard_core::reason::Oid) is the new commit.
    ///
    /// The trait default is the documented honest decline
    /// ([`ForgeError::Unsupported`], `E-FORGE-UNSUPPORTED`): forges
    /// without an API-commit endpoint name themselves and the caller
    /// falls back to `commit_mode = "git"` — mirroring the lockfile
    /// seam's default-decline convention (§7.3).
    ///
    /// # Errors
    ///
    /// [`ForgeError::Unsupported`] by default; adapter-specific failures
    /// otherwise.
    fn commit_via_api(&self, spec: &ApiCommit) -> Result<halyard_core::reason::Oid, ForgeError> {
        let _ = spec;
        Err(ForgeError::unsupported(std::any::type_name::<Self>()))
    }

    /// The adapter's API budget snapshot (§11.5 end-of-run usage
    /// summary). A read — real adapters fill from response headers.
    fn rate_limit(&self) -> RateInfo;

    /// The adapter's operation log since construction, oldest first —
    /// the observability surface behind the §11.5 "end-of-run API usage
    /// summary" and the "≤ 2 REST calls on the update path" budget
    /// assertion (§11.4/M4.1). Real adapters record one entry per forge
    /// call (method + target); injected failures are recorded too, so a
    /// dropped call is never an unobservable one.
    fn calls(&self) -> Vec<CallRecord>;

    /// Convenience over [`Forge::calls`]: how many recorded operations
    /// used the given method name.
    fn call_count(&self, method: &str) -> usize {
        self.calls()
            .iter()
            .filter(|record| record.method == method)
            .count()
    }
}
