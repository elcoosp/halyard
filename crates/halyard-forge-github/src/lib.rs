//! The GitHub REST adapter (M4.1b, §11.4, ADR-0013): a full
//! [`Forge`](halyard_forge::Forge) implementation over GitHub REST v3,
//! plus the blocking-`ureq` [`HttpTransport`] the shell injects.
//!
//! Layer rule (§7.2 as widened by ADR-0013): this crate depends on
//! `halyard-core` + `halyard-forge` and owns the only HTTP client in
//! the runtime graph — the seam crate stays on core only, and the CLI
//! (unrestricted) is the consumer.
//!
//! # Composition
//!
//! ```no_run
//! use halyard_forge::{BranchName, Forge};
//! use halyard_forge_github::{ForgeEndpoint, GitHubRestForge, UreqTransport};
//!
//! let endpoint = ForgeEndpoint::new("octo", "halyard-demo").unwrap();
//! let forge = GitHubRestForge::with_transport(endpoint, "token", UreqTransport::new());
//! let branch = BranchName::new("halyard/release/main").unwrap();
//! // The §11.4 not-found value: no open Release PR on the branch.
//! assert!(forge.find_release_pr(&branch).unwrap().is_none());
//! ```
//!
//! # Failure semantics
//!
//! * `401`/plain `403` → `E-FORGE-AUTH`; rate-limited `403`/`429` →
//!   `E-FORGE-RATE-LIMIT` with the server's wait (`retry-after`, else
//!   `x-ratelimit-reset - now`, else the documented 60 s floor);
//!   `409`/ref-update `422` → `E-FORGE-CONFLICT`; `already_exists` 422
//!   races → `E-FORGE-CONFLICT` (the I1 re-run finds the winner);
//!   connection failures and 5xx → `E-FORGE-TRANSPORT`. No hidden
//!   retries: the run fails closed and the re-run completes (I2).
//! * Without a token every method fails closed with `E-FORGE-AUTH`
//!   before any call.
//!
//! # Wire reference
//!
//! Every endpoint, header and mapping in this crate is pinned against
//! `docs/research/github-rest-forge.md` (the §0.4 `VERIFY` artifact) and
//! asserted by the scripted-contract tests plus the loopback lane.

#![forbid(unsafe_code)]

pub mod forge;
pub mod ureq_transport;
pub mod wire;

pub use forge::GitHubRestForge;
pub use ureq_transport::UreqTransport;
pub use wire::{ForgeEndpoint, RELEASE_BRANCH_PREFIX};
