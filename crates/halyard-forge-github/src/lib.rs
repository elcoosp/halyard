//! The GitHub REST adapter (M4.1b, §11.4): a full
//! [`Forge`](halyard_forge::Forge) implementation over GitHub REST v3,
//! plus the blocking-`ureq` [`HttpTransport`] the shell injects and the
//! GitHub App credentials (M4.1c: JWT → installation token, ADR-0014).
//!
//! Layer rule (§7.2 as widened by ADR-0013): this crate depends on
//! `halyard-core` + `halyard-forge` and owns the only HTTP client in
//! the runtime graph — the seam crate stays on core only, and the CLI
//! (unrestricted) is the consumer.
//!
//! # Composition
//!
//! Token credentials (the M4.1b shape):
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
//! App credentials (M4.1c): the adapter mints the installation token
//! itself (one POST per [`TTL`](app_auth::INSTALLATION_TOKEN_TTL_SECS),
//! the JWT rides the mint's bearer):
//!
//! ```no_run
//! use halyard_forge::{BranchName, Forge};
//! use halyard_forge_github::{AppCredentials, ForgeEndpoint, GitHubRestForge, UreqTransport};
//!
//! let pem = std::fs::read_to_string("halyard-app.private-key.pem")?;
//! let app = AppCredentials::from_pem("12345", &pem, 42)?;
//! let endpoint = ForgeEndpoint::new("octo", "halyard-demo")?;
//! let forge = GitHubRestForge::with_app_transport(endpoint, app, UreqTransport::new());
//! let branch = BranchName::new("halyard/release/main").unwrap();
//! # assert!(forge.find_release_pr(&branch).is_err() || true);
//! # Ok::<(), Box<dyn std::error::Error>>(())
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
//! * Without usable credentials every method fails closed with
//!   `E-FORGE-AUTH` before any call — an empty token, or an App key
//!   that did not parse at construction.
//!
//! # Wire reference
//!
//! Every endpoint, header and mapping in this crate is pinned against
//! `docs/research/github-rest-forge.md` (the §0.4 `VERIFY` artifact) and
//! asserted by the scripted-contract tests plus the loopback lane.

#![forbid(unsafe_code)]

pub mod app_auth;
pub mod forge;
pub mod ureq_transport;
pub mod wire;

pub use app_auth::{
    AppCredentials, EpochClock, ForgeCredentials, INSTALLATION_TOKEN_TTL_SECS, system_epoch,
};
pub use forge::GitHubRestForge;
pub use ureq_transport::UreqTransport;
pub use wire::{ForgeEndpoint, RELEASE_BRANCH_PREFIX};
