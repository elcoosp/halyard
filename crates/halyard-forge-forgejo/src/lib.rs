//! The Forgejo/Gitea REST adapter (M7 step 2, §17 "more forges"): a
//! full [`Forge`](halyard_forge::Forge) implementation over the shared
//! Forgejo/Gitea REST v1 surface (the pull-request flow: find by the
//! `head` filter, create/rewrite/close; the releases API's
//! GET-by-tag-first; the issue comments' marker scan — a pull request
//! **is** an issue on this API), riding the shared forge conformance
//! suite the seam crate ships.
//!
//! Layer rule (§7.2 as widened by ADR-0050): this crate depends on
//! `halyard-core` + `halyard-forge` and carries its own blocking-`ureq`
//! transport — the forge adapters are the HTTP clients of the runtime
//! graph; the seam crate stays on core only, and the CLI (unrestricted)
//! is the consumer.
//!
//! # Composition
//!
//! ```no_run
//! use halyard_forge::{BranchName, Forge};
//! use halyard_forge_forgejo::{ForgejoEndpoint, ForgejoRestForge, UreqTransport};
//!
//! let endpoint = ForgejoEndpoint::new("octo/halyard-demo").unwrap();
//! let forge = ForgejoRestForge::with_transport(endpoint, "forgejo-token", UreqTransport::new());
//! let branch = BranchName::new("halyard/release/main").unwrap();
//! // The §11.4 not-found value: no open Release PR on the branch.
//! assert!(forge.find_release_pr(&branch).unwrap().is_none());
//! ```
//!
//! # Failure semantics
//!
//! * `401`/`403` → `E-FORGE-AUTH`; `429` → `E-FORGE-RATE-LIMIT` with
//!   the standard `retry-after` (RFC 9110 §10.2.3, else the 60 s floor
//!   the other lanes use); the documented `409` on the pull PATCH →
//!   `E-FORGE-CONFLICT`; a rejected request (`400`/`422`) →
//!   `E-FORGE-REQUEST`; connection failures and 5xx →
//!   `E-FORGE-TRANSPORT`. No hidden retries: the run fails closed and
//!   the re-run completes (I2).
//! * Without a usable token every method fails closed with
//!   `E-FORGE-AUTH` before any call.
//! * `commit_via_api` is the documented honest decline
//!   (`E-FORGE-UNSUPPORTED` naming the adapter): the contents API
//!   writes one file per commit and a release commit carries the
//!   plan's whole `EditSet` as one `chore: release` commit (research
//!   §5) — the Forgejo lane rides `commit_mode = "git"`.
//! * The quota snapshot stays an honest zero until a deployment sends
//!   quota headers (research §6) — none are defined on this surface,
//!   and a fabricated quota is worse than an observed absence.
//!
//! # Wire reference
//!
//! Every endpoint, header and mapping in this crate is pinned against
//! `docs/research/forgejo-rest-forge.md` (the §0.4 `VERIFY` artifact
//! for M7 step 2, verified against the live `/api/v1` specification)
//! and asserted by the scripted-contract tests plus the shared
//! conformance matrix (`tests/conformance.rs`).

#![forbid(unsafe_code)]

pub mod forge;
pub mod ureq_transport;
pub mod wire;

pub use forge::ForgejoRestForge;
pub use ureq_transport::UreqTransport;
pub use wire::{ForgejoEndpoint, RELEASE_BRANCH_PREFIX};
