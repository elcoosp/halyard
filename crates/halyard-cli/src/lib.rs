//! The `halyard` CLI's library surface: the man-page renderer (§16.2
//! shell integration) — a pure function over a [`clap::Command`] tree,
//! kept on a lib target so the bench harness and the bin share one
//! renderer. The command tree itself is built by the bin's derive
//! (`crates/halyard-cli/src/main.rs`).
//!
//! Pages are roff, deterministic (I3: the same tree renders
//! byte-identical text) and escape every dash (`\-`), since a literal
//! `-` is roff's option-marker/control-line character.

#![forbid(unsafe_code)]

pub mod man;
