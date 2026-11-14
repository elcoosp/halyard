//! The forge failure taxonomy (§15.4 `E-FORGE-*`): every variant has a
//! stable code and a page under `docs/errors/`, and the [`MemoryForge`](crate::MemoryForge)
//! failpoint harness gives each code a producer the M4.2
//! failure-injection matrix (§15.2) reuses.
//!
//! Layer note (§7.2): plain data, like the rest of the seam — no HTTP, no
//! clock, no environment reads.

/// Why a forge operation failed (§15.4 `E-FORGE-*`; every variant maps
/// onto a `docs/errors/` page). Not-found outcomes are *not* errors:
/// [`Forge::find_release_pr`] returns `None` and
/// [`Forge::release_exists`] returns `false` (§11.4) — the error set is
/// reserved for transport, auth, budget and conflict failures.
///
/// [`Forge::find_release_pr`]: crate::Forge::find_release_pr
/// [`Forge::release_exists`]: crate::Forge::release_exists
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForgeError {
    /// Credentials are missing or lack the required scopes
    /// (`E-FORGE-AUTH`).
    Auth {
        /// What the forge rejected (never the token itself).
        reason: String,
    },
    /// The forge throttled the run (`E-FORGE-RATE-LIMIT`); retryable —
    /// §15.4 maps this onto exit code 5.
    RateLimited {
        /// Suggested wait in seconds (from `Retry-After` in real
        /// adapters; the mock's injected value passes through).
        retry_after_secs: u64,
    },
    /// A compare-and-swap precondition failed (`E-FORGE-CONFLICT`): the
    /// release branch/tag moved under us (§11.2 force-with-lease). The
    /// run must re-plan, never force.
    Conflict {
        /// What moved.
        reason: String,
    },
    /// The caller sent a malformed spec (`E-FORGE-REQUEST`); a fail-closed
    /// caller bug, never retried.
    Request {
        /// What is malformed.
        reason: String,
    },
    /// The exchange never produced a usable HTTP result: a
    /// connection-level failure (DNS, TLS, refused, timeout), a
    /// server-side 5xx, or a response the adapter cannot read
    /// (`E-FORGE-TRANSPORT`; ADR-0013). Retryable at the run level —
    /// never a caller bug, never a credentials problem.
    Transport {
        /// What failed at the transport layer (never credentials, never
        /// a status the adapter interpreted).
        reason: String,
    },
    /// The adapter does not implement this capability
    /// (`E-FORGE-UNSUPPORTED`); the trait's default
    /// [`Forge::commit_via_api`](crate::Forge::commit_via_api) produces it, mirroring the lockfile
    /// seam's default-decline convention (§7.3).
    Unsupported {
        /// The adapter type that was asked.
        adapter: String,
    },
    /// The run's own call budget was exhausted (`E-FORGE-BUDGET`, the
    /// §11.5 governor): the operation was refused *before* it reached
    /// the forge — a fail-closed local policy, never a forge complaint,
    /// never retried here. The re-run resumes where the pass stopped
    /// (I2) without duplicate artifacts.
    Budget {
        /// The refused operation (`"upsert_pr"`, …).
        op: String,
        /// Forwarded attempts already spent this run (§11.5: injected
        /// failures and retries are observable spend).
        spent: u32,
        /// The configured per-run budget.
        budget: u32,
    },
}

impl ForgeError {
    /// Stable §15.4 code of this error (rendered as the report code;
    /// each variant maps onto a `docs/errors/` page).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Auth { .. } => Self::AUTH_CODE,
            Self::RateLimited { .. } => Self::RATE_LIMIT_CODE,
            Self::Conflict { .. } => Self::CONFLICT_CODE,
            Self::Request { .. } => Self::REQUEST_CODE,
            Self::Transport { .. } => Self::TRANSPORT_CODE,
            Self::Unsupported { .. } => Self::UNSUPPORTED_CODE,
            Self::Budget { .. } => Self::BUDGET_CODE,
        }
    }

    /// Builds an [`ForgeError::Auth`] (test/adapter constructor).
    pub fn auth(reason: impl Into<String>) -> Self {
        Self::Auth {
            reason: reason.into(),
        }
    }

    /// Builds an [`ForgeError::RateLimited`].
    pub fn rate_limited(retry_after_secs: u64) -> Self {
        Self::RateLimited { retry_after_secs }
    }

    /// Builds an [`ForgeError::Conflict`].
    pub fn conflict(reason: impl Into<String>) -> Self {
        Self::Conflict {
            reason: reason.into(),
        }
    }

    /// Builds an [`ForgeError::Request`].
    pub fn request(reason: impl Into<String>) -> Self {
        Self::Request {
            reason: reason.into(),
        }
    }

    /// Builds an [`ForgeError::Transport`].
    pub fn transport(reason: impl Into<String>) -> Self {
        Self::Transport {
            reason: reason.into(),
        }
    }

    /// Builds an [`ForgeError::Unsupported`] naming the adapter type.
    pub fn unsupported(adapter: impl Into<String>) -> Self {
        Self::Unsupported {
            adapter: adapter.into(),
        }
    }

    /// Builds an [`ForgeError::Budget`] (the §11.5 governor's refusal).
    pub fn budget(op: impl Into<String>, spent: u32, budget: u32) -> Self {
        Self::Budget {
            op: op.into(),
            spent,
            budget,
        }
    }
}

impl ForgeError {
    /// Stable code of [`ForgeError::Auth`].
    pub const AUTH_CODE: &'static str = "E-FORGE-AUTH";
    /// Stable code of [`ForgeError::RateLimited`].
    pub const RATE_LIMIT_CODE: &'static str = "E-FORGE-RATE-LIMIT";
    /// Stable code of [`ForgeError::Conflict`].
    pub const CONFLICT_CODE: &'static str = "E-FORGE-CONFLICT";
    /// Stable code of [`ForgeError::Request`].
    pub const REQUEST_CODE: &'static str = "E-FORGE-REQUEST";
    /// Stable code of [`ForgeError::Transport`].
    pub const TRANSPORT_CODE: &'static str = "E-FORGE-TRANSPORT";
    /// Stable code of [`ForgeError::Unsupported`].
    pub const UNSUPPORTED_CODE: &'static str = "E-FORGE-UNSUPPORTED";
    /// Stable code of [`ForgeError::Budget`].
    pub const BUDGET_CODE: &'static str = "E-FORGE-BUDGET";
}

impl std::fmt::Display for ForgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auth { reason } => write!(
                f,
                "[{}] forge rejected the credentials: {reason}",
                Self::AUTH_CODE
            ),
            Self::RateLimited { retry_after_secs } => write!(
                f,
                "[{}] forge rate limit hit; retry after {retry_after_secs}s",
                Self::RATE_LIMIT_CODE
            ),
            Self::Conflict { reason } => write!(
                f,
                "[{}] forge state moved under us, re-plan required: {reason}",
                Self::CONFLICT_CODE
            ),
            Self::Request { reason } => {
                write!(
                    f,
                    "[{}] invalid forge request: {reason}",
                    Self::REQUEST_CODE
                )
            }
            Self::Transport { reason } => {
                write!(
                    f,
                    "[{}] forge transport failed: {reason}",
                    Self::TRANSPORT_CODE
                )
            }
            Self::Unsupported { adapter } => write!(
                f,
                "[{}] adapter {adapter} does not implement this capability",
                Self::UNSUPPORTED_CODE
            ),
            Self::Budget { op, spent, budget } => write!(
                f,
                "[{}] forge call budget exhausted: {op} refused after \
                 {spent}/{budget} calls this run",
                Self::BUDGET_CODE
            ),
        }
    }
}

impl std::error::Error for ForgeError {}

#[cfg(test)]
mod tests {
    use super::ForgeError;

    #[test]
    fn codes_and_display_are_stable() {
        let cases = [
            (ForgeError::auth("no token"), ForgeError::AUTH_CODE),
            (ForgeError::rate_limited(30), ForgeError::RATE_LIMIT_CODE),
            (
                ForgeError::conflict("branch moved"),
                ForgeError::CONFLICT_CODE,
            ),
            (
                ForgeError::request("empty branch"),
                ForgeError::REQUEST_CODE,
            ),
            (
                ForgeError::transport("connection refused"),
                ForgeError::TRANSPORT_CODE,
            ),
            (
                ForgeError::unsupported("GitHubRest"),
                ForgeError::UNSUPPORTED_CODE,
            ),
            (
                ForgeError::budget("upsert_pr", 40, 40),
                ForgeError::BUDGET_CODE,
            ),
        ];
        for (err, code) in cases {
            assert_eq!(err.code(), code, "{err}");
            assert!(
                err.to_string().starts_with(&format!("[{code}]")),
                "display leads with the code: {err}"
            );
        }
    }
}
