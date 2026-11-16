//! The OIDC trusted-publishing exchange (M5.3 step 3, ADR-0031): the
//! GitHub Actions runtime-context detection, the id-token request and
//! the crates.io token exchange over the forge [`HttpTransport`]
//! vocabulary, composed behind the [`PublishAuth`] seam as a fallback
//! provider.
//!
//! The grammar is VERIFIED per §0.4 — the reference implementation
//! (`rust-lang/crates-io-auth-action`) and the actions toolkit's
//! `oidc-utils.ts`, fetched quotes in
//! [`docs/research/oidc-trusted-publishing.md`](../../docs/research/oidc-trusted-publishing.md):
//!
//! - **Context**: `ACTIONS_ID_TOKEN_REQUEST_URL` +
//!   `ACTIONS_ID_TOKEN_REQUEST_TOKEN` (the runner exposes both when the
//!   workflow declares `id-token: write`). Both present → a context;
//!   both absent → `None` (the normal shape outside GitHub Actions); a
//!   present-but-empty or half-present pair is a misconfiguration and
//!   fails closed `E-PUB-AUTH` naming the variable — the same rule the
//!   empty-token refusal uses, before any network.
//! - **Audience**: the registry URL minus its `https?://` scheme (the
//!   reference action's rule) — `crates.io` for the default.
//! - **Id-token**: `GET {request_url}&audience={encoded audience}` with
//!   `Authorization: Bearer <request token>`; the 2xx body is JSON with
//!   the token in the `value` field.
//! - **Exchange**: `POST {registry_url}/api/v1/trusted_publishing/tokens`
//!   (`Content-Type: application/json`, body `{"jwt": "<id-token>"}`);
//!   the 2xx body is JSON with the temporary token in the `token`
//!   field. Error bodies carry crates.io's `{"errors":[{"detail":…}]}`
//!   shape — the parsed details ride the refusal message.
//! - **Injection**: the exchanged token rides the documented cargo
//!   grammar as `CARGO_REGISTRY_TOKEN` (the blog's own workflow spells
//!   the same variable) through the existing [`CredentialEnv`] — the
//!   spawn effect injects and scrubs it exactly like an env-resolved
//!   token, and the provenance summary names the source
//!   `oidc:<audience>` (names only, never values).
//!
//! **Precedence and scope**: the documented env grammar wins (an
//! explicit `CARGO_REGISTRY_TOKEN` is never overridden); the exchange
//! applies only to the default crates.io registry — trusted publishing
//! is GitHub-Actions + crates.io today (custom registries keep the env
//! grammar, honest `none`). **Dry mode**: the forecast projects
//! `oidc:<audience>` without any network — the exchange is disabled for
//! the dry spawn (the tool's documented `--dry-run` form needs no
//! token), I5. Layer note: std + `serde_json` + the forge transport
//! vocabulary; the crate still owns no HTTP client (ADR-0031's
//! conscious layer widening, recorded in the check-layers rule).

use std::collections::BTreeMap;
use std::fmt;
use std::fmt::Write;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

use halyard_forge::HttpRequest;
use halyard_forge::HttpResponse;
use halyard_forge::HttpTransport;
use serde_json::Value;

use crate::auth::CredentialEnv;
use crate::auth::EnvPublishAuth;
use crate::auth::PublishAuth;
use crate::auth::is_crates_io;
use crate::{AuthOutcome, PublishError, PublishTarget};

// ---------------------------------------------------------------------------
// The runtime context
// ---------------------------------------------------------------------------

/// The GitHub Actions OIDC runtime context (both documented variables,
/// captured once). The request token is a credential: [`fmt::Debug`]
/// and [`fmt::Display`] redact it, and [`PublishAuth::secrets`] lists
/// it so surfaced tool output can never carry it.
pub struct OidcContext {
    /// `ACTIONS_ID_TOKEN_REQUEST_URL` — the runner's OIDC request base
    /// (the toolkit appends `&audience=…` to it).
    pub request_url: String,
    /// `ACTIONS_ID_TOKEN_REQUEST_TOKEN` — the bearer for the request.
    /// Never rendered, always scrub-listed.
    pub request_token: String,
}

impl fmt::Debug for OidcContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OidcContext")
            .field("request_url", &self.request_url)
            .field("request_token", &"[secret]")
            .finish()
    }
}

impl fmt::Display for OidcContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Provenance only — the URL names the runner, never the token.
        write!(f, "gha-oidc({})", self.request_url)
    }
}

/// Detects the GitHub Actions OIDC runtime context over the injected
/// `reader` (the shell passes a `std::env` reader; tests inject a
/// table — no test mutates the process environment). Both documented
/// variables present and non-empty → [`Some`] with the context; both
/// absent → `None`; a present-but-empty or half-present pair is a
/// misconfiguration — `E-PUB-AUTH` naming the variable, refused before
/// any network.
///
/// # Errors
///
/// [`PublishError::Auth`] when exactly one variable is set (the other
/// missing) or either is set but empty — the workflow's `id-token`
/// permission is half-configured; the message names the variable and
/// the fix.
pub fn gha_oidc_context(
    reader: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<OidcContext>, PublishError> {
    const URL_VAR: &str = "ACTIONS_ID_TOKEN_REQUEST_URL";
    const TOKEN_VAR: &str = "ACTIONS_ID_TOKEN_REQUEST_TOKEN";
    let url = reader(URL_VAR);
    let token = reader(TOKEN_VAR);
    match (url, token) {
        (Some(url), Some(token)) => {
            if url.is_empty() {
                return Err(empty_context_error(URL_VAR));
            }
            if token.is_empty() {
                return Err(empty_context_error(TOKEN_VAR));
            }
            Ok(Some(OidcContext {
                request_url: url,
                request_token: token,
            }))
        }
        (None, None) => Ok(None),
        (Some(_), None) => Err(half_context_error(URL_VAR, TOKEN_VAR)),
        (None, Some(_)) => Err(half_context_error(TOKEN_VAR, URL_VAR)),
    }
}

/// The present-but-empty context refusal (the empty-token rule's
/// wording, the context variables).
fn empty_context_error(variable: &str) -> PublishError {
    PublishError::Auth {
        registry: "env".to_owned(),
        message: format!(
            "the GitHub Actions OIDC variable {variable} is set but empty — the runner \
             exposed a half-configured context (the workflow's `id-token` permission is \
             `write` but the variable carries nothing); unset it or fix the workflow \
             permissions (see E-PUB-AUTH)"
        ),
    }
}

/// The half-present context refusal: one variable without the other.
fn half_context_error(present: &str, missing: &str) -> PublishError {
    PublishError::Auth {
        registry: "env".to_owned(),
        message: format!(
            "the GitHub Actions OIDC context is half-configured: {present} is set but \
             {missing} is missing — the workflow needs `permissions: id-token: write` so \
             the runner exposes both variables; set the permission or unset {present} to \
             keep the documented env grammar (see E-PUB-AUTH)"
        ),
    }
}

// ---------------------------------------------------------------------------
// The URL grammar (VERIFIED)
// ---------------------------------------------------------------------------

/// The OIDC audience for `registry_url`: the URL minus its
/// `https?://` scheme (the reference action's rule; `crates.io` for
/// the default). A URL without a scheme is returned unchanged — the
/// audience is then the URL itself, which is the same rule.
#[must_use]
pub fn audience_for(registry_url: &str) -> String {
    registry_url
        .strip_prefix("https://")
        .unwrap_or_else(|| registry_url.strip_prefix("http://").unwrap_or(registry_url))
        .to_owned()
}

/// Percent-encodes `audience` into the query component of the
/// id-token request: RFC 3986 unreserved characters pass through,
/// everything else becomes `%XX` uppercase hex (the actions toolkit's
/// `encodeURIComponent` shape — verified).
#[must_use]
pub fn encode_audience(audience: &str) -> String {
    let mut encoded = String::with_capacity(audience.len());
    for byte in audience.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(*byte as char);
            }
            other => {
                encoded.push('%');
                let high = other >> 4;
                let low = other & 0x0F;
                encoded.push(
                    char::from_digit(u32::from(high), 16)
                        .unwrap_or('0')
                        .to_ascii_uppercase(),
                );
                encoded.push(
                    char::from_digit(u32::from(low), 16)
                        .unwrap_or('0')
                        .to_ascii_uppercase(),
                );
            }
        }
    }
    encoded
}

/// The registry's trusted-publishing token endpoint: `{registry_url}/
/// api/v1/trusted_publishing/tokens` (the reference action's
/// `getTokensEndpoint`; a trailing slash is stripped first).
#[must_use]
pub fn tokens_endpoint(registry_url: &str) -> String {
    let trimmed = registry_url.strip_suffix('/').unwrap_or(registry_url);
    format!("{trimmed}/api/v1/trusted_publishing/tokens")
}

// ---------------------------------------------------------------------------
// The exchange client
// ---------------------------------------------------------------------------

/// The id-token + token exchange over an injected [`HttpTransport`] —
/// the same transport vocabulary the registry clients speak (the
/// scripted [`MemoryTransport`](halyard_forge::MemoryTransport) in
/// tests, the blocking `UreqTransport` at runtime). The client owns no
/// HTTP connection and interprets statuses itself: HTTP statuses are
/// values; only connection-level failures arrive as
/// [`ForgeError::Transport`](halyard_forge::ForgeError).
pub struct OidcExchanger {
    transport: Arc<dyn HttpTransport>,
    /// The static User-Agent (crates.io asks clients to identify
    /// themselves; the reference action sends its own package header).
    user_agent: &'static str,
}

impl fmt::Debug for OidcExchanger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The transport is opaque (any `HttpTransport` impl); the
        // client identifies itself through the agent header only.
        f.debug_struct("OidcExchanger")
            .field("transport", &"[injected HttpTransport]")
            .field("user_agent", &self.user_agent)
            .finish()
    }
}

impl OidcExchanger {
    /// Builds the exchanger over `transport`.
    #[must_use]
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self {
            transport,
            user_agent: "halyard/oidc-trusted-publishing",
        }
    }

    /// Requests the id-token for `audience` from the runner context
    /// (verified shape: `GET {request_url}&audience={encoded}` with the
    /// bearer header; 2xx body JSON `{"value": "<id-token>"}`).
    ///
    /// # Errors
    ///
    /// [`PublishError::Auth`] for a transport failure, a non-2xx
    /// status, a body without the `value` field, or unparseable JSON —
    /// every message scrubbed of the context token and the id-token.
    fn request_id_token(
        &self,
        context: &OidcContext,
        audience: &str,
    ) -> Result<String, PublishError> {
        let url = format!(
            "{}&audience={}",
            context.request_url,
            encode_audience(audience)
        );
        let request = HttpRequest {
            method: "GET",
            url,
            headers: vec![
                (
                    "Authorization".to_owned(),
                    format!("Bearer {}", context.request_token),
                ),
                ("User-Agent".to_owned(), self.user_agent.to_owned()),
            ],
            body: None,
        };
        let response = self.execute(request)?;
        if !is_success(response.status) {
            return Err(Self::status_error(
                "the GitHub Actions id-token request",
                &response,
            ));
        }
        let body = parse_json(&response.body, "the GitHub Actions id-token response")?;
        let id_token = body
            .get("value")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| PublishError::Auth {
                registry: "crates.io".to_owned(),
                message: "the GitHub Actions id-token response has no `value` field — the \
                          runner's OIDC endpoint answered outside the documented shape (see \
                          E-PUB-AUTH)"
                    .to_owned(),
            })?;
        Ok(id_token.to_owned())
    }

    /// Exchanges `id_token` for the registry's temporary token
    /// (verified shape: `POST {tokens_endpoint}` JSON body
    /// `{"jwt": "<id-token>"}`; 2xx body JSON `{"token": "<token>"}`).
    ///
    /// # Errors
    ///
    /// [`PublishError::Auth`] for a transport failure, a non-2xx
    /// status (the parsed `errors[].detail` details ride the message),
    /// a body without the `token` field, or unparseable JSON — every
    /// message scrubbed of both tokens.
    fn exchange(&self, registry_url: &str, id_token: &str) -> Result<String, PublishError> {
        let request = HttpRequest {
            method: "POST",
            url: tokens_endpoint(registry_url),
            headers: vec![
                ("Content-Type".to_owned(), "application/json".to_owned()),
                ("User-Agent".to_owned(), self.user_agent.to_owned()),
            ],
            body: Some(format!(r#"{{"jwt":"{id_token}"}}"#)),
        };
        let response = self.execute(request)?;
        if !is_success(response.status) {
            return Err(Self::status_error(
                "the crates.io trusted-publishing exchange",
                &response,
            ));
        }
        let body = parse_json(&response.body, "the crates.io trusted-publishing response")?;
        let token = body
            .get("token")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| PublishError::Auth {
                registry: "crates.io".to_owned(),
                message: "the crates.io trusted-publishing response has no `token` field — \
                          the registry answered outside the documented shape (see E-PUB-AUTH)"
                    .to_owned(),
            })?;
        Ok(token.to_owned())
    }

    /// Executes one exchange step, mapping a transport failure onto the
    /// seam's auth code (the OIDC exchange is an auth step; a DNS/TLS
    /// failure is not a silently absent credential).
    fn execute(&self, request: HttpRequest) -> Result<HttpResponse, PublishError> {
        self.transport
            .execute(request)
            .map_err(|error| PublishError::Auth {
                registry: "crates.io".to_owned(),
                message: format!("the OIDC exchange transport failed: {error}"),
            })
    }

    /// The non-2xx refusal: status + crates.io's parsed
    /// `errors[].detail` details when the body carries them, else a
    /// body excerpt — all scrubbed by the caller before surfacing.
    fn status_error(operation: &str, response: &HttpResponse) -> PublishError {
        let mut message = format!("{operation} failed with status {}", response.status);
        if let Ok(body) = parse_json(&response.body, operation) {
            if let Some(details) = body.get("errors").and_then(Value::as_array) {
                for detail in details {
                    if let Some(text) = detail.get("detail").and_then(Value::as_str) {
                        let _ = write!(message, "; {text}");
                    }
                }
            }
        } else {
            // Not the documented error shape: quote a bounded excerpt
            // (the scrub pass runs before anything surfaces).
            let excerpt: String = response.body.chars().take(200).collect();
            message.push_str("; body: ");
            message.push_str(&excerpt);
        }
        PublishError::Auth {
            registry: "crates.io".to_owned(),
            message,
        }
    }
}

/// HTTP 2xx (statuses are values; anything outside the success range
/// is the adapter's refusal, never a transport error).
fn is_success(status: u16) -> bool {
    (200..300).contains(&status)
}

/// Parses a JSON body; unparseable JSON refuses with a bounded excerpt
/// (the scrub pass runs before anything surfaces).
fn parse_json(body: &str, operation: &str) -> Result<Value, PublishError> {
    serde_json::from_str(body).map_err(|error| {
        let excerpt: String = body.chars().take(200).collect();
        PublishError::Auth {
            registry: "crates.io".to_owned(),
            message: format!("{operation} is not valid JSON: {error}; body: {excerpt}"),
        }
    })
}

// ---------------------------------------------------------------------------
// The provider
// ---------------------------------------------------------------------------

/// The documented env grammar first, the OIDC trusted-publishing
/// exchange as the fallback (ADR-0031): an explicit env token is never
/// overridden; a crates.io target with no documented variable and a
/// detected GitHub Actions context exchanges for the temporary registry
/// token and injects it as `CARGO_REGISTRY_TOKEN` — memoized, so one
/// exchange serves the whole run.
///
/// `exchange_enabled = false` (the shell's dry forecast) keeps the
/// projection honest without network: credential resolution stays
/// [`AuthOutcome::Absent`] while [`PublishAuth::provenance`] still
/// spells `oidc:<audience>`.
///
/// The exchanged and context tokens are scrub-listed by
/// [`PublishAuth::secrets`] (a value `credentials` can resolve is a
/// value `secrets` lists), and the provider's `Debug` renders names
/// only — no secret ever rides a log line or a panic message.
pub struct OidcPublishAuth {
    /// The base provider (the documented grammar, captured once).
    base: EnvPublishAuth,
    /// The detected runtime context (`None` outside GitHub Actions).
    context: Option<OidcContext>,
    /// The exchange client over the injected transport.
    exchanger: OidcExchanger,
    /// The registry URL the exchange talks to (`https://crates.io` for
    /// the default; the audience derives from it).
    registry_url: String,
    /// Whether the exchange may fire (live runs only — the dry forecast
    /// never touches the network, I5).
    exchange_enabled: bool,
    /// Memoized exchanged credentials per registry (one exchange per
    /// run; poisoning cannot corrupt the invariants — the map is a
    /// plain insert-only memo — so the guard is recovered and the
    /// panic propagates from the original site).
    exchanged: Mutex<BTreeMap<String, CredentialEnv>>,
}

impl fmt::Debug for OidcPublishAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Names only: the base provider's Debug redacts, the context
        // redacts, and the exchanged map renders registry names.
        let memo = self
            .exchanged
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let registries: Vec<&String> = memo.keys().collect();
        f.debug_struct("OidcPublishAuth")
            .field("base", &self.base)
            .field("context", &self.context)
            .field("exchanger", &self.exchanger)
            .field("registry_url", &self.registry_url)
            .field("exchange_enabled", &self.exchange_enabled)
            .field("exchanged_registries", &registries)
            .finish()
    }
}

impl OidcPublishAuth {
    /// Builds the provider. `context` is the detected GitHub Actions
    /// runtime context ([`gha_oidc_context`]); `registry_url` is the
    /// exchange target (`https://crates.io` for the default);
    /// `exchange_enabled` gates the network exchange off for the dry
    /// forecast.
    #[must_use]
    pub fn new(
        base: EnvPublishAuth,
        context: Option<OidcContext>,
        transport: Arc<dyn HttpTransport>,
        registry_url: String,
        exchange_enabled: bool,
    ) -> Self {
        Self {
            base,
            context,
            exchanger: OidcExchanger::new(transport),
            registry_url,
            exchange_enabled,
            exchanged: Mutex::new(BTreeMap::new()),
        }
    }

    /// Whether the OIDC fallback covers `target`: a context is present,
    /// the target publishes cargo to the default crates.io registry,
    /// and the documented env grammar covered nothing (trusted
    /// publishing is GitHub-Actions + crates.io today — custom
    /// registries keep the env grammar).
    fn covers(&self, target: &PublishTarget) -> bool {
        self.context.is_some() && target.ecosystem == "cargo" && is_crates_io(&target.registry)
    }

    /// The memoized exchange: one id-token request + one registry
    /// exchange per run, the result injected under the documented
    /// cargo variable name. The memo lock never spans the exchange —
    /// the failure path re-locks the map for the scrub pass, so the
    /// guard is scoped to the lookup and the insert only.
    fn exchange_for(
        &self,
        target: &PublishTarget,
        context: &OidcContext,
    ) -> Result<CredentialEnv, PublishError> {
        {
            let memo = self
                .exchanged
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some(credential) = memo.get(&target.registry) {
                return Ok(credential.clone());
            }
        }
        let audience = audience_for(&self.registry_url);
        let id_token = self
            .exchanger
            .request_id_token(context, &audience)
            .map_err(|error| self.scrub_error(error))?;
        let token = self
            .exchanger
            .exchange(&self.registry_url, &id_token)
            .map_err(|error| self.scrub_error(error))?;
        let credential = CredentialEnv {
            key: "CARGO_REGISTRY_TOKEN".to_owned(),
            value: token,
        };
        let mut memo = self
            .exchanged
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        memo.insert(target.registry.clone(), credential.clone());
        Ok(credential)
    }

    /// Runs the scrub pass over an exchange refusal: the context token,
    /// the id-token and any resolved exchanged token never ride a
    /// surfaced message (M5.3 step 1's contract, extended to the
    /// exchanged tokens).
    fn scrub_error(&self, error: PublishError) -> PublishError {
        let PublishError::Auth { registry, message } = error else {
            return error;
        };
        let mut secrets: Vec<&str> = Vec::new();
        if let Some(context) = &self.context {
            secrets.push(context.request_token.as_str());
        }
        let memo = self
            .exchanged
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for credential in memo.values() {
            secrets.push(credential.value.as_str());
        }
        let message = crate::auth::scrub_secrets(&message, &secrets);
        PublishError::Auth { registry, message }
    }
}

impl PublishAuth for OidcPublishAuth {
    fn credentials(&self, target: &PublishTarget) -> Result<AuthOutcome, PublishError> {
        // The documented grammar wins — an explicit env token is never
        // overridden, and a misconfiguration (present-but-empty)
        // refuses exactly as the base provider would.
        match self.base.credentials(target)? {
            AuthOutcome::Token(credential) => return Ok(AuthOutcome::Token(credential)),
            AuthOutcome::Absent => {}
        }
        if !self.exchange_enabled {
            // The dry forecast: tokenless spawn, projected provenance,
            // zero network (I5).
            return Ok(AuthOutcome::Absent);
        }
        if let (Some(context), true) = (&self.context, self.covers(target)) {
            return Ok(AuthOutcome::Token(self.exchange_for(target, context)?));
        }
        Ok(AuthOutcome::Absent)
    }

    fn secrets(&self) -> Vec<String> {
        // The contract (a value `credentials` can resolve is a value
        // `secrets` lists): the base values (name-sorted), then the
        // exchanged tokens (registry-name-sorted), then the context
        // request token. Deterministic given the same resolution
        // sequence.
        let mut secrets = self.base.secrets();
        secrets.extend(
            self.exchanged
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .values()
                .map(|credential| credential.value.clone()),
        );
        if let Some(context) = &self.context {
            secrets.push(context.request_token.clone());
        }
        secrets
    }

    fn provenance(&self, target: &PublishTarget) -> Result<String, PublishError> {
        // Env-resolved credentials keep the documented spelling; an
        // OIDC-eligible registry projects `oidc:<audience>` — resolved
        // from the provider alone, the exchange never fires for the
        // spelling (the summary renders before and outside the run).
        match self.base.credentials(target)? {
            AuthOutcome::Token(credential) => return Ok(credential.to_string()),
            AuthOutcome::Absent => {}
        }
        if self.covers(target) {
            return Ok(format!("oidc:{}", audience_for(&self.registry_url)));
        }
        Ok("none".to_owned())
    }
}
