//! The Bitbucket Cloud REST adapter (M7 step 3, §17 "more forges"): a
//! full [`Forge`](halyard_forge::Forge) implementation over the shared
//! Bitbucket REST 2.0 surface (the pull-request flow: find by the
//! documented `q` filter, create/rewrite/decline; the tag-as-release
//! probe — the API group index documents no Releases group, so the git
//! tag is the release marker and halyard invents no release object;
//! the PR comments' marker scan riding the documented
//! `page`/`pagelen` pagination), riding the shared forge conformance
//! suite the seam crate ships.
//!
//! Layer rule (§7.2 as widened by ADR-0051): this crate depends on
//! `halyard-core` + `halyard-forge` and carries its own blocking-`ureq`
//! transport — the forge adapters are the HTTP clients of the runtime
//! graph; the seam crate stays on core only, and the CLI (unrestricted)
//! is the consumer.
//!
//! # Composition
//!
//! ```no_run
//! use halyard_forge::{BranchName, Forge};
//! use halyard_forge_bitbucket::{BitbucketEndpoint, BitbucketRestForge, UreqTransport};
//!
//! let endpoint = BitbucketEndpoint::new("octo/halyard-demo").unwrap();
//! let forge = BitbucketRestForge::with_transport(endpoint, "bb-token", UreqTransport::new());
//! let branch = BranchName::new("halyard/release/main").unwrap();
//! // The §11.4 not-found value: no open Release PR on the branch.
//! assert!(forge.find_release_pr(&branch).unwrap().is_none());
//! ```
//!
//! # Failure semantics
//!
//! * `401`/`403` → `E-FORGE-AUTH`; `429` → `E-FORGE-RATE-LIMIT` with
//!   the standard `retry-after` (RFC 9110 §10.2.3, else the 60 s floor
//!   the other lanes use); a rejected request (`400`/`422`) →
//!   `E-FORGE-REQUEST`; `409` → `E-FORGE-CONFLICT`; connection failures
//!   and 5xx → `E-FORGE-TRANSPORT`. No hidden retries: the run fails
//!   closed and the re-run completes (I2).
//! * Without a usable token every method fails closed with
//!   `E-FORGE-AUTH` before any call.
//! * `commit_via_api` is the documented honest decline
//!   (`E-FORGE-UNSUPPORTED` naming the adapter): the commits resource
//!   "does NOT support new commit creation" (research §5) — the
//!   Bitbucket lane rides `commit_mode = "git"`.
//! * `create_release` probes the tag (the release marker on this
//!   surface, research §3): present ⇒ the existed shape with the tag's
//!   web URL; absent ⇒ the honest decline (no release object exists to
//!   create, and the git-side tag phase has not landed the marker yet).
//! * The quota snapshot records only the documented
//!   `X-RateLimit-Limit` header — no remaining-requests header exists
//!   on this surface (research §6), so `remaining` stays the honest
//!   zero; a fabricated quota is worse than an observed absence.
//!
//! # Wire reference
//!
//! Every endpoint, header and mapping in this crate is pinned against
//! `docs/research/bitbucket-rest-forge.md` (the §0.4 `VERIFY` artifact
//! for M7 step 3, verified against Atlassian's documented REST 2.0
//! reference and the request-limits page) and asserted by the
//! scripted-contract tests plus the shared conformance matrix
//! (`tests/conformance.rs`).

#![forbid(unsafe_code)]

pub mod forge;
pub mod ureq_transport;
pub mod wire;

pub use forge::BitbucketRestForge;
pub use ureq_transport::UreqTransport;
pub use wire::{BitbucketEndpoint, RELEASE_BRANCH_PREFIX};
