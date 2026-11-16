//! Config errors — stable codes `E-CFG-UNKNOWN-KEY`, `E-CFG-TAG-STYLE`,
//! `E-CFG-TOML`, `E-CFG-ENV`, `E-CFG-MIGRATE`, `E-CFG-CHANNEL`,
//! `E-CFG-INIT` (§15.4; pages under `docs/errors/`).

/// An error while loading or resolving `halyard.toml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// A key that is not part of the frozen v1 schema; carries a
    /// did-you-mean suggestion when one is close enough.
    UnknownKey {
        /// Dotted key path context (`repo`, `commits.types`, `package[0]`).
        path: String,
        /// The unknown key.
        key: String,
        /// Closest valid key (edit distance ≤ 2), if any.
        suggestion: Option<String>,
        /// The valid keys at that location, sorted.
        valid: Vec<String>,
    },
    /// The tag style is neither `auto` nor a valid template (appendix
    /// A.3; `TagTemplate` grammar).
    TagStyle {
        /// The offending value.
        value: String,
        /// Why the template was rejected.
        reason: String,
    },
    /// Reading or parsing a configuration file failed.
    Read {
        /// Path as given by the caller (never canonicalized).
        path: String,
        /// Human-readable cause (I/O message or TOML diagnostics).
        message: String,
    },
    /// A `HALYARD_*` environment variable carried an invalid value.
    Env {
        /// The variable name.
        var: String,
        /// What was wrong.
        message: String,
    },
    /// A foreign release-tool configuration (M4.7 `migrate`) could not
    /// be parsed or does not fit its documented grammar. Fail-closed: a
    /// half-understood foreign config is never mapped.
    Migrate {
        /// The foreign file's repo-relative path, as discovered.
        path: String,
        /// What is wrong (parse error or grammar violation) and what to
        /// do instead.
        message: String,
    },
    /// The `[versioning].channel` value (§10.3) is not a valid channel
    /// name — one semver identifier: alphanumerics and hyphens, non-empty,
    /// no leading or trailing hyphen.
    Channel {
        /// The offending value.
        value: String,
        /// What is wrong with the name.
        reason: String,
    },
    /// A `[versioning].maintenance_branches` entry (§10.3, ADR-0027) is
    /// not a `MAJOR.x` / `MAJOR.MINOR.x` line — the entry names the
    /// branch and pins its line, so both roles must parse.
    MaintenanceLine {
        /// The offending entry.
        value: String,
        /// What is wrong with the entry.
        reason: String,
    },
    /// The `halyard init` scaffold (§16.1/§16.2, ADR-0029) refused:
    /// create-only, fail-closed. Raised when the survey found nothing
    /// to scaffold for, when a target already exists (an existing
    /// config or workflow is never clobbered), or when the atomic
    /// write itself fails.
    Init {
        /// The target path (or `.` for a survey-level refusal).
        path: String,
        /// What is wrong, what was looked at and what to do instead.
        message: String,
    },
}

impl ConfigError {
    /// Stable code for [`ConfigError::UnknownKey`].
    pub const UNKNOWN_KEY_CODE: &'static str = "E-CFG-UNKNOWN-KEY";
    /// Stable code for [`ConfigError::TagStyle`].
    pub const TAG_STYLE_CODE: &'static str = "E-CFG-TAG-STYLE";
    /// Stable code for [`ConfigError::Read`].
    pub const READ_CODE: &'static str = "E-CFG-TOML";
    /// Stable code for [`ConfigError::Env`].
    pub const ENV_CODE: &'static str = "E-CFG-ENV";
    /// Stable code for [`ConfigError::Migrate`].
    pub const MIGRATE_CODE: &'static str = "E-CFG-MIGRATE";
    /// Stable code for [`ConfigError::Channel`].
    pub const CHANNEL_CODE: &'static str = "E-CFG-CHANNEL";
    /// Stable code for [`ConfigError::MaintenanceLine`].
    pub const MAINTENANCE_CODE: &'static str = "E-CFG-MAINTENANCE";
    /// Stable code for [`ConfigError::Init`].
    pub const INIT_CODE: &'static str = "E-CFG-INIT";

    /// Stable error code for this variant.
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownKey { .. } => Self::UNKNOWN_KEY_CODE,
            Self::TagStyle { .. } => Self::TAG_STYLE_CODE,
            Self::Read { .. } => Self::READ_CODE,
            Self::Env { .. } => Self::ENV_CODE,
            Self::Migrate { .. } => Self::MIGRATE_CODE,
            Self::Channel { .. } => Self::CHANNEL_CODE,
            Self::MaintenanceLine { .. } => Self::MAINTENANCE_CODE,
            Self::Init { .. } => Self::INIT_CODE,
        }
    }
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownKey {
                path,
                key,
                suggestion,
                valid,
            } => {
                write!(
                    f,
                    "[{}] unknown key `{key}` in {path}",
                    Self::UNKNOWN_KEY_CODE
                )?;
                if let Some(suggestion) = suggestion {
                    write!(f, " — did you mean `{suggestion}`?")?;
                } else {
                    write!(f, " — no close match")?;
                }
                write!(f, "; valid keys: {}", valid.join(", "))?;
                Ok(())
            }
            Self::TagStyle { value, reason } => write!(
                f,
                "[{}] invalid tag style {value:?}: {reason} (use `auto` or a template like \
                 `{{name}}-v{{version}}`)",
                Self::TAG_STYLE_CODE
            ),
            Self::Read { path, message } => {
                write!(f, "[{}] cannot load {path}: {message}", Self::READ_CODE)
            }
            Self::Env { var, message } => {
                write!(f, "[{}] invalid value for {var}: {message}", Self::ENV_CODE)
            }
            Self::Migrate { path, message } => write!(
                f,
                "[{}] cannot migrate {path}: {message} — fix or remove the foreign \
                 configuration; the migration never guesses",
                Self::MIGRATE_CODE
            ),
            Self::Channel { value, reason } => write!(
                f,
                "[{}] invalid prerelease channel {value:?}: {reason} — a channel is one \
                 semver identifier: alphanumerics and hyphens, no leading or trailing hyphen",
                Self::CHANNEL_CODE
            ),
            Self::MaintenanceLine { value, reason } => write!(
                f,
                "[{}] invalid maintenance branch {value:?}: {reason} — an entry names the \
                 branch and pins its line, e.g. `1.x` or `1.2.x`",
                Self::MAINTENANCE_CODE
            ),
            Self::Init { path, message } => {
                write!(f, "[{}] cannot scaffold {path}: {message}", Self::INIT_CODE)
            }
        }
    }
}

impl std::error::Error for ConfigError {}
