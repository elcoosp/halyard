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
//! This crate is an effectful shell component (it touches the
//! filesystem); the pure core never depends on it.

#![forbid(unsafe_code)]

mod apply;
mod atomic;
mod error;

pub use apply::{EditSet, LineEndings, Patch, apply, detect_line_endings, verify_outputs};
pub use atomic::{apply_to_file, apply_to_file_verified, atomic_write};
pub use error::EditError;
