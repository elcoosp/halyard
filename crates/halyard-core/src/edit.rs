//! The adapter edit seam (§7.3 edit subset, M3): the outcome and failure
//! types shared by every `EcosystemAdapter::edit_versions` implementation.
//!
//! The planner (M1.9) reduces a release to byte patches (§5.8 `edits`,
//! §9.1) over the manifests as they exist at [`crate::plan::Plan::head`].
//! The adapters re-apply those patches **semantically**: they parse the
//! manifest, locate the row each patch targets, classify it exactly like
//! the requirement rewriter (`crate::rewrite`) does, and rewrite the value
//! in place with format-preserving tools (`toml_edit` / the JSON span
//! scanner). Counts returned here drive the deterministic `apply` summary
//! (M3.6).
//!
//! Layer note (§7.2): this module is plain data — the effectful
//! filesystem work (reads, atomic writes) stays in `halyard-edit`.

use crate::package::Span;
use crate::rewrite::VersionError;

/// Result of one adapter edit pass over one manifest (§7.3 edit subset).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditOutcome {
    /// The patched manifest bytes. Byte-identical to the input when no
    /// patch applied (invariant I4: untouched bytes survive verbatim).
    pub content: Vec<u8>,
    /// Rows whose requirement/version text was rewritten.
    pub applied: usize,
    /// Rows matched but intentionally not rewritten (protocols,
    /// workspace-inherited rows, rows already carrying the target text).
    pub skipped: usize,
}

/// Why an adapter edit pass failed (§15.4 `E-EDIT-*`; every variant has a
/// page under `docs/errors/`). A failure aborts the whole manifest edit —
/// nothing is mutated and nothing is written (§9.1 apply protocol).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditFailure {
    /// The manifest is not valid UTF-8, does not parse, or the read-back
    /// check after the rewrite failed (`E-EDIT-VERIFY`); nothing was
    /// mutated.
    Parse {
        /// Manifest path the edit targeted.
        file: String,
        /// What exactly went wrong.
        message: String,
    },
    /// A patch byte span does not match any editable row of the manifest
    /// (`E-EDIT-PATCHES`) — the working tree drifted from the file the
    /// plan was computed against; re-plan instead of forcing the patch.
    SpanMismatch {
        /// Manifest path the edit targeted.
        file: String,
        /// Inclusive start byte offset of the unmatched patch.
        start: u32,
        /// Exclusive end byte offset of the unmatched patch.
        end: u32,
    },
    /// A patch targets a row whose requirement cannot be rewritten so it
    /// admits the new version (`E-EDIT-UNREWRITABLE`). Plan patches are
    /// required-to-edit by construction (the planner emits only rewrites
    /// it classified through [`crate::rewrite::rewrite_requirement`]), so
    /// an unrewritable row at a patch span is a hard error, not a skip.
    Unrewritable {
        /// Manifest path the edit targeted.
        file: String,
        /// The underlying rewriter failure (stable code
        /// `E-EDIT-UNREWRITABLE`).
        detail: VersionError,
    },
}

impl EditFailure {
    /// Stable §15.4 code of this error (rendered as the miette report
    /// code; each variant maps onto an existing `docs/errors/` page).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Parse { .. } => "E-EDIT-VERIFY",
            Self::SpanMismatch { .. } => "E-EDIT-PATCHES",
            Self::Unrewritable { .. } => "E-EDIT-UNREWRITABLE",
        }
    }
}

impl std::fmt::Display for EditFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse { file, message } => {
                write!(f, "[{}] cannot edit {file}: {message}", Self::PARSE_CODE)
            }
            Self::SpanMismatch { file, start, end } => write!(
                f,
                "[{}] patch span {start}..{end} matches no editable row of {file}; the manifest \
                 changed since the plan was computed — re-plan",
                Self::PATCHES_CODE
            ),
            Self::Unrewritable { file, detail } => {
                write!(f, "[{}] {file}: {detail}", Self::UNREWRITABLE_CODE)
            }
        }
    }
}

impl EditFailure {
    /// Stable code of [`EditFailure::Parse`].
    pub const PARSE_CODE: &'static str = "E-EDIT-VERIFY";
    /// Stable code of [`EditFailure::SpanMismatch`].
    pub const PATCHES_CODE: &'static str = "E-EDIT-PATCHES";
    /// Stable code of [`EditFailure::Unrewritable`].
    pub const UNREWRITABLE_CODE: &'static str = "E-EDIT-UNREWRITABLE";
}

impl std::error::Error for EditFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unrewritable { detail, .. } => Some(detail),
            _ => None,
        }
    }
}

/// Byte span accessor shared by adapter implementations: slices `bytes`
/// at `span`, returning `None` when the span is out of bounds (defensive:
/// span-produced patches always round-trip).
pub fn slice_span(bytes: &[u8], span: Span) -> Option<&[u8]> {
    let (start, end) = span_range(span);
    bytes.get(start..end)
}

/// Widens a [`Span`] to `usize` range bounds (`u32` offsets never exceed
/// `usize` on the platforms this tool targets; an oversized span reads as
/// "past the end").
pub fn span_range(span: Span) -> (usize, usize) {
    (
        usize::try_from(span.start).unwrap_or(usize::MAX),
        usize::try_from(span.end).unwrap_or(usize::MAX),
    )
}
