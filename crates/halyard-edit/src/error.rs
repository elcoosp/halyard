//! Edit errors — stable codes `E-EDIT-PATCHES`, `E-EDIT-VERIFY`,
//! `E-EDIT-IO` (§15.4; pages under `docs/errors/`).

/// An error while validating, applying or writing an edit set.
#[derive(Debug)]
pub enum EditError {
    /// The patch set is invalid: unsorted ranges, an overlap, an
    /// inverted range, or a range outside the file.
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
    /// Stable code for [`EditError::Io`].
    pub const IO_CODE: &'static str = "E-EDIT-IO";

    /// Stable error code for this variant.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidPatches { .. } => Self::PATCHES_CODE,
            Self::VerifyFailed { .. } => Self::VERIFY_CODE,
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
