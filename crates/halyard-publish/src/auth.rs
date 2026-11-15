//! The publish-auth plumbing (M5.3 step 1, ADR-0019): the [`PublishAuth`]
//! seam that resolves one publish target's registry credentials into an
//! injectable environment pair, and the [`scrub_secrets`] helper that
//! keeps resolved secrets out of surfaced tool output.
//!
//! The grammar is the tools' own, VERIFIED per §0.4 (see
//! `docs/research/publish-auth-env.md` for the fetched quotes):
//!
//! - **cargo** reads tokens directly from the environment:
//!   `CARGO_REGISTRY_TOKEN` for crates.io (the Cargo Book's singular
//!   variable), `CARGO_REGISTRIES_<NAME>_TOKEN` for every other
//!   registry — NAME is the registry name uppercased with dots and
//!   dashes converted to underscores (the config reference's
//!   env-variable rule). The `registries.crates-io.token` general
//!   mapping (`CARGO_REGISTRIES_CRATES_IO_TOKEN`) is the documented
//!   fallback for crates.io.
//! - **npm** (the pnpm spawn, §13.4) reads tokens through the
//!   user-level auth file's `${NAME}` interpolation: `NPM_TOKEN` is the
//!   canonical candidate, `NODE_AUTH_TOKEN` the actions/setup-node
//!   convention. pnpm deliberately does **not** expand `${…}` for
//!   credentials in the project `.npmrc` (its anti-exfiltration rule),
//!   so halyard never writes auth files — the env var rides the
//!   user-level file contract the tool already documents.
//!
//! Absent credentials are a **value, not an error** ([`AuthOutcome::
//! Absent`]): the tools' own credential stores (`cargo login`, the
//! user-level auth file) govern, and halyard cannot see them. A token
//! that is *present but empty* is a misconfiguration — with a `${VAR}`
//! interpolation it becomes an empty bearer token and a confusing 401 —
//! and fails closed with `E-PUB-AUTH` naming the variable, before any
//! spawn.
//!
//! The spawn injects the resolved pair and scrubs every resolved value
//! from tool output before it reaches a report row: a token a
//! misconfigured tool echoes must never leak into the human/JSON/GHA
//! renderers. Layer note: std-only, same rule as the engine
//! (`publish → [core, registry]`).

use std::collections::BTreeMap;
use std::fmt;

use crate::{PublishError, PublishTarget};

// ---------------------------------------------------------------------------
// The seam
// ---------------------------------------------------------------------------

/// The publish-auth seam (M5.3 step 1, ADR-0019): resolves one publish
/// target's registry credentials into the environment pair the spawn
/// effect injects, and exposes the resolved secret values so surfaced
/// tool output can be scrubbed. The shell owns the resolution sources
/// (the production [`EnvPublishAuth`] reads the process environment
/// once); tests inject scripted ones.
///
/// The trait is object-safe (`&dyn`) and `Send + Sync` — the engine's
/// bounded within-layer parallelism shares the effect, and the effect
/// shares the provider, across scoped threads.
pub trait PublishAuth: Send + Sync {
    /// Resolves the credentials for `target`.
    ///
    /// # Errors
    ///
    /// [`PublishError::Auth`] (`E-PUB-AUTH`) for a present-but-empty
    /// token — a misconfiguration refused before any spawn, naming the
    /// variable. Absence is [`AuthOutcome::Absent`], never an error.
    fn credentials(&self, target: &PublishTarget) -> Result<AuthOutcome, PublishError>;

    /// Every secret value this provider resolved (sorted by variable
    /// name — deterministic, I3). The spawn effect scrubs each of them
    /// from tool output before it reaches a report row; the list is
    /// the scrubbing contract: a value `credentials` can resolve is a
    /// value `secrets` lists.
    fn secrets(&self) -> Vec<String>;
}

/// One target's resolved credentials: inject the pair, or inject
/// nothing (the tool's own credential store governs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthOutcome {
    /// The resolved credential — the spawn effect injects
    /// `credential.key = credential.value` into the tool's environment.
    Token(CredentialEnv),
    /// No credential the documented grammar covers is present. Nothing
    /// is injected; the tool's own credential store (cargo login, the
    /// user-level auth file) governs the run.
    Absent,
}

/// One injectable environment pair: the documented variable name and
/// its secret value. The value is plain data for the spawn's
/// `Command::env` — but [`fmt::Debug`] and [`fmt::Display`] always
/// redact it, so a credential can never leak through a log line, a
/// report render or a panic message.
#[derive(Clone, PartialEq, Eq)]
pub struct CredentialEnv {
    /// The documented environment-variable name (`CARGO_REGISTRY_TOKEN`,
    /// `NPM_TOKEN`, …).
    pub key: String,
    /// The secret value. Never rendered: the `Debug`/`Display` impls
    /// redact it, and it is scrubbed from tool output by the spawn
    /// effect ([`scrub_secrets`]).
    pub value: String,
}

impl fmt::Debug for CredentialEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CredentialEnv")
            .field("key", &self.key)
            .field("value", &"[secret]")
            .finish()
    }
}

impl fmt::Display for CredentialEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Provenance only — the name names the source, never the value.
        write!(f, "env:{}", self.key)
    }
}

// ---------------------------------------------------------------------------
// The grammar (VERIFIED — see the module doc + the research note)
// ---------------------------------------------------------------------------

/// The documented candidate variables for `target`, in resolution order
/// (first present wins). Empty means the grammar covers no variable for
/// this target's ecosystem — an honest [`AuthOutcome::Absent`] (no
/// invented grammar; an unknown ecosystem's tool governs its own
/// credentials).
///
/// Public because the doctor's `publish-auth` check reports the same
/// table — the grammar is data, not scattered string literals.
#[must_use]
pub fn variables_for(target: &PublishTarget) -> Vec<String> {
    match target.ecosystem.as_str() {
        "cargo" => {
            if is_crates_io(&target.registry) {
                vec![
                    "CARGO_REGISTRY_TOKEN".to_owned(),
                    "CARGO_REGISTRIES_CRATES_IO_TOKEN".to_owned(),
                ]
            } else {
                vec![format!(
                    "CARGO_REGISTRIES_{}_TOKEN",
                    registry_env_fragment(&target.registry)
                )]
            }
        }
        // The pnpm spawn reads tokens through the user-level auth
        // file's `${NAME}` interpolation (pnpm.io/npmrc); `NPM_TOKEN`
        // is the canonical candidate, `NODE_AUTH_TOKEN` the
        // actions/setup-node convention. First present wins.
        "npm" => vec!["NPM_TOKEN".to_owned(), "NODE_AUTH_TOKEN".to_owned()],
        _ => Vec::new(),
    }
}

/// Whether `registry` names crates.io (both the dotted §13 spelling and
/// Cargo's own dashed config spelling resolve to the same registry).
fn is_crates_io(registry: &str) -> bool {
    registry == "crates.io" || registry == "crates-io"
}

/// The `<NAME>` fragment of `CARGO_REGISTRIES_<NAME>_TOKEN`: the
/// registry name uppercased with dots and dashes converted to
/// underscores (the Cargo config reference's env-variable rule:
/// "Keys are converted to uppercase, dots and dashes are converted to
/// underscores").
fn registry_env_fragment(registry: &str) -> String {
    registry
        .chars()
        .map(|c| match c {
            '.' | '-' => '_',
            other => other,
        })
        .collect::<String>()
        .to_uppercase()
}

// ---------------------------------------------------------------------------
// The production provider
// ---------------------------------------------------------------------------

/// The production [`PublishAuth`]: the process environment captured
/// **once** into a table (deterministic for the whole run — I3 — and
/// shared by [`PublishAuth::credentials`] and [`PublishAuth::secrets`],
/// so the scrubbing contract cannot drift from the resolution).
///
/// [`EnvPublishAuth::capture`] scans the documented candidates plus every variable
/// matching the `CARGO_REGISTRIES_<NAME>_TOKEN` shape, so custom cargo
/// registries resolve without enumerating their names up front. A
/// token that is present but empty fails the capture (`E-PUB-AUTH`
/// naming the variable): the run would inject an empty bearer token.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct EnvPublishAuth {
    table: BTreeMap<String, String>,
}

impl fmt::Debug for EnvPublishAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The keys name the sources; the values are secrets.
        let keys: Vec<&String> = self.table.keys().collect();
        f.debug_struct("EnvPublishAuth")
            .field("variables", &keys)
            .finish()
    }
}

impl EnvPublishAuth {
    /// Captures the production table from the process environment.
    ///
    /// # Errors
    ///
    /// [`PublishError::Auth`] when a documented token variable is
    /// present but empty (`E-PUB-AUTH`, naming the variable) — refused
    /// before the run instead of injected into a doomed spawn.
    pub fn capture() -> Result<Self, PublishError> {
        let mut table = BTreeMap::new();
        let documented: [&str; 4] = [
            "CARGO_REGISTRY_TOKEN",
            "CARGO_REGISTRIES_CRATES_IO_TOKEN",
            "NPM_TOKEN",
            "NODE_AUTH_TOKEN",
        ];
        let mut empty: Vec<String> = Vec::new();
        for name in documented {
            match std::env::var(name) {
                Ok(value) if value.is_empty() => empty.push(name.to_owned()),
                Ok(value) => {
                    table.insert(name.to_owned(), value);
                }
                Err(_) => {}
            }
        }
        // Custom cargo registries: every variable of the documented
        // `CARGO_REGISTRIES_<NAME>_TOKEN` shape (non-UTF-8 variables
        // are skipped — they cannot be injected into a UTF-8 argv/env
        // pair anyway).
        for (name, value) in std::env::vars() {
            if name.starts_with("CARGO_REGISTRIES_")
                && name.ends_with("_TOKEN")
                && name != "CARGO_REGISTRIES_CRATES_IO_TOKEN"
            {
                if value.is_empty() {
                    empty.push(name);
                } else {
                    table.insert(name, value);
                }
            }
        }
        if let Some(first) = empty.first() {
            return Err(PublishError::Auth {
                registry: "env".to_owned(),
                message: format!(
                    "the publish token variable {first} is set but empty — with a `${{VAR}}` \
                     auth-file interpolation an empty value becomes an empty bearer token and a \
                     confusing 401; unset the variable or give it the real token (see \
                     E-PUB-AUTH)"
                ),
            });
        }
        Ok(Self { table })
    }

    /// A provider over an injected table (tests, and shells whose
    /// credential source is not the process environment — a secret
    /// store resolved before the run). Values are validated lazily by
    /// [`PublishAuth::credentials`], exactly as the [`EnvPublishAuth::capture`] production path validates eagerly
    /// them eagerly.
    #[must_use]
    pub fn from_table(table: BTreeMap<String, String>) -> Self {
        Self { table }
    }

    /// The captured table (variable name → value), in name order. The
    /// caller owns the secrecy of the values from here on.
    #[must_use]
    pub fn table(&self) -> &BTreeMap<String, String> {
        &self.table
    }
}

impl PublishAuth for EnvPublishAuth {
    fn credentials(&self, target: &PublishTarget) -> Result<AuthOutcome, PublishError> {
        for key in variables_for(target) {
            match self.table.get(&key) {
                // Present but empty: the misconfiguration the module
                // doc describes, refused before any spawn.
                Some(value) if value.is_empty() => {
                    return Err(PublishError::Auth {
                        registry: target.registry.clone(),
                        message: format!(
                            "the publish token variable {key} is set but empty — with a \
                             `${{VAR}}` auth-file interpolation an empty value becomes an \
                             empty bearer token and a confusing 401; unset the variable or \
                             give it the real token (see E-PUB-AUTH)"
                        ),
                    });
                }
                Some(value) => {
                    return Ok(AuthOutcome::Token(CredentialEnv {
                        key,
                        value: value.clone(),
                    }));
                }
                None => {}
            }
        }
        Ok(AuthOutcome::Absent)
    }

    fn secrets(&self) -> Vec<String> {
        // BTreeMap iterates in key order — the deterministic listing.
        self.table.values().cloned().collect()
    }
}

// ---------------------------------------------------------------------------
// Provenance summary (M5.3 step 2: the renderers' input)
// ---------------------------------------------------------------------------

/// Resolves the per-registry auth provenance for a run: each target's
/// registry name mapped to `env:<VARIABLE>` when a documented candidate
/// resolved, or `none` when nothing will be injected (the tool's own
/// credential store governs). Name-sorted by the map itself — plain
/// deterministic data (I3) the renderers embed verbatim; **values never
/// appear** — the provenance names the variable, never the secret.
///
/// # Errors
///
/// [`PublishError::Auth`] when a target's resolution fails closed
/// (present-but-empty); the run refuses before any spawn, so a summary
/// never coexists with a misconfiguration.
pub fn auth_summary(
    auth: &dyn PublishAuth,
    targets: &[PublishTarget],
) -> Result<BTreeMap<String, String>, PublishError> {
    let mut summary = BTreeMap::new();
    for target in targets {
        match auth.credentials(target)? {
            AuthOutcome::Token(credential) => {
                summary.insert(target.registry.clone(), credential.to_string());
            }
            AuthOutcome::Absent => {
                summary.insert(target.registry.clone(), "none".to_owned());
            }
        }
    }
    Ok(summary)
}

// ---------------------------------------------------------------------------
// Scrubbing
// ---------------------------------------------------------------------------

/// Replaces every occurrence of every non-empty secret in `text` with
/// the `[secret]` marker. The spawn effect runs surfaced tool output
/// (the stderr tail that becomes a report row's message) through this
/// before wrapping it in [`PublishError::Effect`] — a token a
/// misconfigured tool echoes must never reach the human/JSON/GHA
/// renderers.
///
/// Deterministic: secrets are replaced in the given order, empty
/// entries are inert, and the marker itself contains no secret.
#[must_use]
pub fn scrub_secrets(text: &str, secrets: &[&str]) -> String {
    let mut scrubbed = text.to_owned();
    for secret in secrets {
        if !secret.is_empty() {
            scrubbed = scrubbed.replace(secret, "[secret]");
        }
    }
    scrubbed
}
