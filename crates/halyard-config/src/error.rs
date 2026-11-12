//! Config errors — stable codes `E-CFG-UNKNOWN-KEY`, `E-CFG-TAG-STYLE`,
//! `E-CFG-TOML`, `E-CFG-ENV` (§15.4; pages under `docs/errors/`).

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

    /// Stable error code for this variant.
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownKey { .. } => Self::UNKNOWN_KEY_CODE,
            Self::TagStyle { .. } => Self::TAG_STYLE_CODE,
            Self::Read { .. } => Self::READ_CODE,
            Self::Env { .. } => Self::ENV_CODE,
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
        }
    }
}

impl std::error::Error for ConfigError {}
