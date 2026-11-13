//! HALYARD edit crate (M1.4, §9.1): `EditSet` byte-range patches, atomic
//! apply with fsync+rename, and EOL/BOM preservation.
//!
//! Patches are computed from parser spans (`toml_edit` / the JSON span
//! scanner / changelog rendering), applied **over ascending ranges** in
//! memory, verified by re-parsing, and written with the atomic protocol
//! of §9.1: temp file in the same directory → fsync → rename (Windows:
//! retry on `ERROR_ACCESS_DENIED`). Byte-range semantics make
//! preservation of untouched bytes (comments, ordering, whitespace,
//! EOLs, BOM) automatic — invariant I4.
//!
//! The M3.6 plan-driven apply ([`apply_plan`]) extends this with the
//! local `apply` flow: a [`halyard_core::plan::Plan`]'s edits convert
//! into an [`EditSet`] ([`edit_set_from_plan`], [`dry_run_preview`] for
//! `--dry`), route through the ecosystem adapters' semantic patchers
//! (§7.3 `edit_versions`) and the M3.2 lockfile stage (`patch_lockfile`,
//! §9.2), and land with the same atomic protocol.
//!
//! This crate is an effectful shell component (it touches the
//! filesystem); the pure core never depends on it.

#![forbid(unsafe_code)]

mod apply;
mod atomic;
mod error;
mod plan_apply;

pub use apply::{EditSet, LineEndings, Patch, apply, detect_line_endings, verify_outputs};
pub use atomic::{apply_to_file, apply_to_file_verified, atomic_write};
pub use error::EditError;
pub use plan_apply::{
    ApplyReport, DryPatch, FileSummary, LockfileSummary, apply_plan, dry_run_preview,
    edit_set_from_plan,
};
