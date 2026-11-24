//! The GitLab REST adapter (M7 step 1, §17 "more forges"): a full
//! [`Forge`](halyard_forge::Forge) implementation over GitLab REST v4
//! (the MR flow: find-by-`source_branch`, create/rewrite/close; the
//! releases API's GET-by-tag-first; the notes API's marker scan; the
//! commits API's base64 actions), riding the shared forge conformance
//! suite the seam crate ships.
//!
//! Layer rule (§7.2 as widened by ADR-0049): this crate depends on
//! `halyard-core` + `halyard-forge` and carries its own blocking-`ureq`
//! transport — the forge adapters are the HTTP clients of the runtime
//! graph; the seam crate stays on core only, and the CLI (unrestricted)
//! is the consumer.
//!
//! # Composition
//!
//! ```no_run
//! use halyard_forge::{BranchName, Forge};
//! use halyard_forge_gitlab::{GitLabEndpoint, GitLabRestForge, UreqTransport};
//!
//! let endpoint = GitLabEndpoint::new("group/halyard-demo").unwrap();
//! let forge = GitLabRestForge::with_transport(endpoint, "glpat-token", UreqTransport::new());
//! let branch = BranchName::new("halyard/release/main").unwrap();
//! // The §11.4 not-found value: no open Release MR on the branch.
//! assert!(forge.find_release_pr(&branch).unwrap().is_none());
//! ```
//!
//! # Failure semantics
//!
//! * `401`/plain `403` → `E-FORGE-AUTH`; throttled `403`/`429` →
//!   `E-FORGE-RATE-LIMIT` with the server's own `retry-after` (else the
//!   60 s floor); `409` → `E-FORGE-CONFLICT`; a rejected request →
//!   `E-FORGE-REQUEST`; connection failures and 5xx →
//!   `E-FORGE-TRANSPORT`. No hidden retries: the run fails closed and
//!   the re-run completes (I2).
//! * Without a usable token every method fails closed with
//!   `E-FORGE-AUTH` before any call.
//! * The quota snapshot stays an honest zero until a deployment sends
//!   quota headers (research §7) — GitLab does not universally send
//!   them, and a fabricated quota is worse than an observed absence.
//!
//! # Wire reference
//!
//! Every endpoint, header and mapping in this crate is pinned against
//! `docs/research/gitlab-rest-forge.md` (the §0.4 `VERIFY` artifact for
//! M7 step 1) and asserted by the scripted-contract tests plus the
//! shared conformance matrix (`tests/conformance.rs`).

#![forbid(unsafe_code)]

pub mod forge;
pub mod ureq_transport;
pub mod wire;

pub use forge::GitLabRestForge;
pub use ureq_transport::UreqTransport;
pub use wire::{GitLabEndpoint, RELEASE_BRANCH_PREFIX};
