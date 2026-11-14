//! The registry existence-probe seam (M5.1, §13): "before each publish,
//! `published(pkg, version)` via registry" — the pure read half of the
//! publish engine, shipped ahead of the M5.2 publisher so the resume
//! contract is testable offline.
//!
//! Two adapters over the forge's [`HttpTransport`] (ADR-0013's
//! vocabulary — the registry owns no HTTP client; the runtime injects
//! the blocking `UreqTransport` from `halyard-forge-github`, tests
//! inject the scripted `MemoryTransport`):
//!
//! * [`CratesIo`] — the sparse index (`index.crates.io`, §13): one
//!   newline-delimited JSON file per package, one line per version, the
//!   version in `vers` (Cargo Book "Index files", verified live). The
//!   directory-prefix scheme (`1/a`, `2/ab`, `3/l/log`, `se/rd/serde`)
//!   is pinned by [`CratesIo::index_path`] tests.
//! * [`Npm`] — the packument document (`registry.npmjs.org/<name>`, §13
//!   "conditional GET"): a JSON object whose `versions` maps version
//!   strings to objects (verified live, including unencoded scoped
//!   names like `@angular/common`).
//!
//! Status semantics, fail-closed (§15.4 "errors teach"): a `404` means
//! *not published* (both registries 404 a missing package — verified
//! live); a `200` with the version present means *published*; a foreign
//! body or an unexpected status is `E-REG-PROTOCOL` (never silently
//! "not published" — that would double-publish); a transport failure is
//! `E-REG-IO`. An invalid package name never reaches the wire
//! (`E-REG-NAME`).
//!
//! The §13 step-3 visibility wait ([`wait_until_visible`]) polls
//! `published` with deterministic backoff — base doubling, capped, no
//! jitter (the I3 deviation ADR-0015 documents for the governor; the
//! same reasoning applies: identical inputs ⇒ identical wait behavior) —
//! bounded by the [`VisibilityPolicy`], the sleep sequence observable
//! through the injected [`RegistrySleeper`].
//!
//! Layer note: `registry → forge` (the transport vocabulary) and no
//! `halyard-core` dependency — the probe speaks plain names and
//! versions; the workspace vocabulary joins with the M5.2 publisher.

use std::sync::Arc;
use std::time::Duration;

use halyard_forge::transport::{HttpRequest, HttpTransport};

/// Registry probe failures (§15.4 `E-REG-*`; each variant has a page
/// under `docs/errors/`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryError {
    /// The package name is not valid for the registry (`E-REG-NAME`):
    /// empty, carries a separator in the wrong place, whitespace, or a
    /// character the registry's grammar rejects. Fail-closed: an
    /// invalid name never reaches the wire.
    Name {
        /// What is wrong and what the grammar expects.
        message: String,
    },
    /// The probe could not reach the registry (`E-REG-IO`): the
    /// transport reported a connection-level failure (DNS, TLS,
    /// refused, timeout) or an unusable server outcome.
    Io {
        /// What the transport reported.
        message: String,
    },
    /// The registry answered something this client cannot interpret
    /// (`E-REG-PROTOCOL`): an unexpected status, a non-JSON body, a
    /// body missing the expected shape. Fail-closed by design — an
    /// unparseable answer is never read as "not published" (that would
    /// double-publish, §13).
    Protocol {
        /// What arrived instead of the expected shape.
        message: String,
    },
}

impl RegistryError {
    /// Stable §15.4 code of this error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Name { .. } => "E-REG-NAME",
            Self::Io { .. } => "E-REG-IO",
            Self::Protocol { .. } => "E-REG-PROTOCOL",
        }
    }
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Name { message } => write!(f, "[E-REG-NAME] {message}"),
            Self::Io { message } => write!(f, "[E-REG-IO] {message}"),
            Self::Protocol { message } => write!(f, "[E-REG-PROTOCOL] {message}"),
        }
    }
}

impl std::error::Error for RegistryError {}

impl From<halyard_forge::ForgeError> for RegistryError {
    fn from(error: halyard_forge::ForgeError) -> Self {
        Self::Io {
            message: error.to_string(),
        }
    }
}

/// The existence-probe seam (§13 step 2): one pure read per call —
/// "is this exact version visible on the registry right now?". The
/// M5.2 publisher consumes it for idempotent resume (I2) and for the
/// §13 step-3 visibility wait ([`wait_until_visible`]).
pub trait Registry {
    /// Whether `version` of `name` is visible on the registry.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Name`] for an invalid package name (never sent
    /// to the wire), [`RegistryError::Io`] when the transport fails,
    /// [`RegistryError::Protocol`] for an uninterpretable response.
    fn published(&self, name: &str, version: &str) -> Result<bool, RegistryError>;
}

/// The crates.io sparse-index client (§13: "crates.io **sparse index**
/// `index.crates.io/<prefix>/<name>`"). One GET per probe; the file is
/// newline-delimited JSON with one line per published version (`vers`).
pub struct CratesIo {
    transport: Arc<dyn HttpTransport>,
    index_base: String,
}

impl CratesIo {
    /// A client over `transport` probing the real crates.io index.
    #[must_use]
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self::with_index_base(transport, "https://index.crates.io")
    }

    /// A client probing a different index base (contract tests, mirrors).
    #[must_use]
    pub fn with_index_base(transport: Arc<dyn HttpTransport>, index_base: &str) -> Self {
        Self {
            transport,
            index_base: index_base.trim_end_matches('/').to_owned(),
        }
    }

    /// The sparse-index path of `name` (Cargo Book "Index files",
    /// verified live): 1-char names under `1/`, 2-char under `2/`,
    /// 3-char under `3/<first>/`, everything else
    /// `<first-two>/<next-two>/<lowercased-name>` (`ca/rg/cargo`).
    ///
    /// # Errors
    ///
    /// [`RegistryError::Name`] when `name` is not a valid crate name
    /// (empty, separators in the wrong places, whitespace, or a
    /// character outside the crate-name grammar).
    pub fn index_path(name: &str) -> Result<String, RegistryError> {
        validate_crate_name(name)?;
        let lower = name.to_ascii_lowercase();
        let path = match lower.len() {
            1 => format!("1/{lower}"),
            2 => format!("2/{lower}"),
            3 => format!("3/{}/{}", &lower[..1], lower),
            _ => format!("{}/{}/{}", &lower[..2], &lower[2..4], lower),
        };
        Ok(path)
    }

    /// The absolute URL one probe fetches.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Name`] for an invalid name.
    pub fn probe_url(&self, name: &str) -> Result<String, RegistryError> {
        Ok(format!("{}/{}", self.index_base, Self::index_path(name)?))
    }
}

impl Registry for CratesIo {
    fn published(&self, name: &str, version: &str) -> Result<bool, RegistryError> {
        let url = self.probe_url(name)?;
        let response = self.transport.execute(HttpRequest {
            method: "GET",
            url,
            headers: Vec::new(),
            body: None,
        })?;
        // Both registries 404 a missing package (verified live): the
        // §13 resume reads it as "not published", never an error.
        if response.status == 404 {
            return Ok(false);
        }
        if response.status != 200 {
            return Err(RegistryError::Protocol {
                message: format!(
                    "the sparse index answered {} for {name} (expected 200 or 404)",
                    response.status
                ),
            });
        }
        for line in response.body.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let entry: serde_json::Value =
                serde_json::from_str(line).map_err(|e| RegistryError::Protocol {
                    message: format!("the sparse index sent a non-JSON line: {e}"),
                })?;
            if entry.get("vers").and_then(|v| v.as_str()) == Some(version) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// The npm packument client (§13: "npm `registry.npmjs.org/<name>`
/// conditional GET"). One GET per probe; the document's `versions`
/// object maps version strings to per-version objects.
pub struct Npm {
    transport: Arc<dyn HttpTransport>,
    registry_base: String,
}

impl Npm {
    /// A client over `transport` probing the real npm registry.
    #[must_use]
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self::with_registry_base(transport, "https://registry.npmjs.org")
    }

    /// A client probing a different registry base (contract tests,
    /// private mirrors).
    #[must_use]
    pub fn with_registry_base(transport: Arc<dyn HttpTransport>, registry_base: &str) -> Self {
        Self {
            transport,
            registry_base: registry_base.trim_end_matches('/').to_owned(),
        }
    }

    /// The packument URL of `name`. Scoped names ride the path
    /// **unencoded** (`registry.npmjs.org/@scope/name`) — verified live
    /// against registry.npmjs.org; stricter third-party mirrors that
    /// require the `%2f`-encoded form stay a documented gap (the
    /// research note records the VERIFY state).
    ///
    /// # Errors
    ///
    /// [`RegistryError::Name`] when `name` is not a valid npm package
    /// name (empty, more than one `/`, whitespace, a leading character
    /// outside the grammar).
    pub fn probe_url(&self, name: &str) -> Result<String, RegistryError> {
        validate_npm_name(name)?;
        let base = self.registry_base.as_str();
        Ok(format!("{base}/{name}"))
    }
}

impl Registry for Npm {
    fn published(&self, name: &str, version: &str) -> Result<bool, RegistryError> {
        let url = self.probe_url(name)?;
        let response = self.transport.execute(HttpRequest {
            method: "GET",
            url,
            headers: Vec::new(),
            body: None,
        })?;
        if response.status == 404 {
            return Ok(false);
        }
        if response.status != 200 {
            return Err(RegistryError::Protocol {
                message: format!(
                    "the registry answered {} for {name} (expected 200 or 404)",
                    response.status
                ),
            });
        }
        let document: serde_json::Value =
            serde_json::from_str(&response.body).map_err(|e| RegistryError::Protocol {
                message: format!("the packument is not JSON: {e}"),
            })?;
        let versions = document
            .get("versions")
            .ok_or_else(|| RegistryError::Protocol {
                message: "the packument carries no versions map".to_owned(),
            })?;
        Ok(versions.get(version).is_some())
    }
}

/// The §13 step-3 visibility-wait policy: bounded polling with
/// deterministic backoff (no jitter — I3, the ADR-0015 deviation
/// convention). `max_polls × delays` bounds the wall clock; the M5.2
/// publisher renders exhaustion as an honest skip, never a guess.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibilityPolicy {
    /// Maximum number of `published` probes (the first probe + this
    /// many retries... exactly: at most `max_polls` probes total).
    pub max_polls: u32,
    /// The first retry's delay; each further retry doubles it.
    pub base_delay: Duration,
    /// The backoff cap (exponential growth stops here).
    pub max_delay: Duration,
}

/// The sleep effect of the visibility wait — injected so tests observe
/// the exact delay sequence (the [`crate::governor`]-style injected
/// clock/sleeper convention of ADR-0015).
pub trait RegistrySleeper {
    /// Sleeps (or records) one backoff delay.
    fn sleep(&self, delay: Duration);
}

/// The §13 step-3 visibility wait (pure control flow over any
/// [`Registry`]): probe → on a miss sleep the doubling-capped backoff →
/// probe again, at most `policy.max_polls` probes total. Returns
/// whether the version became visible. Errors propagate immediately (a
/// failing registry is not a "not yet visible").
///
/// # Errors
///
/// Whatever [`Registry::published`] raises.
pub fn wait_until_visible(
    registry: &dyn Registry,
    name: &str,
    version: &str,
    policy: &VisibilityPolicy,
    sleeper: &dyn RegistrySleeper,
) -> Result<bool, RegistryError> {
    let mut delay = policy.base_delay;
    for poll in 0..policy.max_polls {
        if poll > 0 {
            sleeper.sleep(delay);
            delay = delay.saturating_mul(2).min(policy.max_delay);
        }
        if registry.published(name, version)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Crate-name validation (§13, the Cargo Book grammar): ASCII letters,
/// digits, `-` and `_`; must start with a letter. The index path
/// lowercases (Cargo Book: "the index filenames are in lowercase").
fn validate_crate_name(name: &str) -> Result<(), RegistryError> {
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
    if valid {
        Ok(())
    } else {
        Err(RegistryError::Name {
            message: format!(
                "{name:?} is not a valid crate name (ASCII letters, digits, '-' and '_', \
                 starting with a letter) — the probe never sends an invalid name"
            ),
        })
    }
}

/// npm-name validation (the registry's package grammar): unscoped
/// `name` or scoped `@scope/name` — exactly one `/` when scoped, both
/// halves non-empty, no whitespace, and a bare `@name` (no slash) is
/// not a package name.
fn validate_npm_name(name: &str) -> Result<(), RegistryError> {
    let valid = !name.is_empty()
        && !name.chars().any(char::is_whitespace)
        && match name.split_once('/') {
            Some((scope, pkg)) => {
                scope.starts_with('@') && scope.len() > 1 && !pkg.is_empty() && !pkg.contains('/')
            }
            None => {
                !name.starts_with('@') && name.chars().next().is_some_and(char::is_alphanumeric)
            }
        };
    if valid {
        Ok(())
    } else {
        Err(RegistryError::Name {
            message: format!(
                "{name:?} is not a valid npm package name (name or @scope/name, no \
                 whitespace) — the probe never sends an invalid name"
            ),
        })
    }
}
