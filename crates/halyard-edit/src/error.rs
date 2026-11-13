//! Edit errors — stable codes `E-EDIT-PATCHES`, `E-EDIT-VERIFY`,
//! `E-EDIT-IO`, `E-EDIT-UNREWRITABLE`, `E-LOCK-PARSE` (§15.4; pages under
//! `docs/errors/`).

/// An error while validating, applying or writing an edit set.
#[derive(Debug)]
pub enum EditError {
    /// The patch set is invalid: unsorted ranges, an overlap, an
    /// inverted range, a range outside the file, or a patch span that no
    /// longer matches the manifest (stale plan).
    InvalidPatches {
        /// What exactly is wrong (file/patch context included).
        reason: String,
    },
    /// The re-parse verification of an applied file failed; nothing was
    /// written (§9.1 apply protocol).
    VerifyFailed {
        /// What the verifier reported.
        reason: String,
    },
    /// A required-to-edit row cannot be rewritten so it admits the new
    /// version (`E-EDIT-UNREWRITABLE`; surfaced by the ecosystem
    /// adapters' semantic patchers, M3.1/M3.3).
    Unrewritable {
        /// The manifest carrying the unrewritable row plus the rewriter
        /// detail.
        message: String,
    },
    /// A lockfile patch pass failed outright (`E-LOCK-PARSE`, M3.2): the
    /// lockfile does not parse or a planned change is unusable; nothing
    /// was written (§9.1 apply protocol).
    LockfileParse {
        /// What the lockfile patcher reported.
        reason: String,
    },
    /// A filesystem operation failed (read, temp write, fsync, rename).
    Io {
        /// Path the operation targeted.
        path: String,
        /// Underlying OS error.
        source: std::io::Error,
    },
}

impl EditError {
    /// Stable code for [`EditError::InvalidPatches`].
    pub const PATCHES_CODE: &'static str = "E-EDIT-PATCHES";
    /// Stable code for [`EditError::VerifyFailed`].
    pub const VERIFY_CODE: &'static str = "E-EDIT-VERIFY";
    /// Stable code for [`EditError::Unrewritable`].
    pub const UNREWRITABLE_CODE: &'static str = "E-EDIT-UNREWRITABLE";
    /// Stable code for [`EditError::LockfileParse`].
    pub const LOCKFILE_PARSE_CODE: &'static str = "E-LOCK-PARSE";
    /// Stable code for [`EditError::Io`].
    pub const IO_CODE: &'static str = "E-EDIT-IO";

    /// Stable error code for this variant.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidPatches { .. } => Self::PATCHES_CODE,
            Self::VerifyFailed { .. } => Self::VERIFY_CODE,
            Self::Unrewritable { .. } => Self::UNREWRITABLE_CODE,
            Self::LockfileParse { .. } => Self::LOCKFILE_PARSE_CODE,
            Self::Io { .. } => Self::IO_CODE,
        }
    }
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPatches { reason } => {
                write!(f, "[{}] invalid patch set: {reason}", Self::PATCHES_CODE)
            }
            Self::VerifyFailed { reason } => {
                write!(
                    f,
                    "[{}] verification failed, nothing written: {reason}",
                    Self::VERIFY_CODE
                )
            }
            Self::Unrewritable { message } => {
                write!(f, "[{}] {message}", Self::UNREWRITABLE_CODE)
            }
            Self::LockfileParse { reason } => {
                write!(f, "[{}] {reason}", Self::LOCKFILE_PARSE_CODE)
            }
            Self::Io { path, source } => {
                write!(
                    f,
                    "[{}] filesystem error on {path}: {source}",
                    Self::IO_CODE
                )
            }
        }
    }
}

impl std::error::Error for EditError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
