//! Fixture error type — stable code `E-TEST-FIXTURE`.

/// Classification of a fixture failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixtureErrorKind {
    /// A `git` invocation exited non-zero.
    GitInvocation,
    /// A `git` invocation succeeded but its output could not be interpreted.
    GitOutput,
    /// A filesystem operation failed.
    Io,
    /// The scenario DSL was used in an order that cannot produce a valid
    /// repository (e.g. octopus merge with fewer than two heads).
    Scenario,
}

/// Error raised while building or inspecting a [`crate::GitFixture`].
///
/// Not part of the end-user taxonomy (§15.4): it never escapes test
/// infrastructure, but it still carries the stable `E-TEST-FIXTURE` code so
/// CI logs are greppable.
#[derive(Debug)]
pub struct FixtureError {
    kind: FixtureErrorKind,
    message: String,
}

impl FixtureError {
    /// Stable error code (`docs/errors/E-TEST-FIXTURE.md`).
    pub const CODE: &'static str = "E-TEST-FIXTURE";

    pub(crate) fn git_invocation(message: String) -> Self {
        Self {
            kind: FixtureErrorKind::GitInvocation,
            message,
        }
    }

    pub(crate) fn git_output(message: String) -> Self {
        Self {
            kind: FixtureErrorKind::GitOutput,
            message,
        }
    }

    pub(crate) fn io(message: String) -> Self {
        Self {
            kind: FixtureErrorKind::Io,
            message,
        }
    }

    pub(crate) fn scenario(message: String) -> Self {
        Self {
            kind: FixtureErrorKind::Scenario,
            message,
        }
    }

    /// Failure classification.
    pub fn kind(&self) -> FixtureErrorKind {
        self.kind
    }

    /// Human-readable detail: what ran and what git said.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Stable error code, equal to [`FixtureError::CODE`].
    pub fn code(&self) -> &'static str {
        Self::CODE
    }
}

impl std::fmt::Display for FixtureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {:?}", Self::CODE, self.kind)?;
        write!(f, ": {}", self.message)
    }
}

impl std::error::Error for FixtureError {}
