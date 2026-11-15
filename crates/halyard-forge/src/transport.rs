//! The transport seam (M4.1b, ADR-0013): the plain-data HTTP vocabulary
//! the REST adapters speak — [`HttpRequest`]/[`HttpResponse`], the
//! [`HttpTransport`] effect seam and the scripted [`MemoryTransport`]
//! reference fake (the transport analogue of the forge crate's
//! `MemoryForge`).
//!
//! The seam stays dependency-free and layer-rule-clean: an HTTP
//! *status* is a **value** (the adapter interprets it per endpoint, §11.4
//! not-found semantics); only connection-level failures — and 5xx
//! responses, which never yield a usable result — are
//! [`ForgeError::Transport`] errors. The real client
//! ([`UreqTransport`](https://docs.rs/halyard-forge-github) in the
//! `halyard-forge-github` crate) maps its own connection failures onto
//! the same code, so contract tests can drive any adapter offline
//! through [`MemoryTransport`] and the §15.1 loopback lane tests the
//! real socket behavior without new dependencies.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;

use crate::error::ForgeError;

/// One outbound HTTP request — plain, ordered data: the method, the
/// absolute URL, headers in send order and an optional body (JSON for
/// every REST adapter this workspace ships).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRequest {
    /// The HTTP verb (`"GET"`, `"POST"`, `"PATCH"`).
    pub method: &'static str,
    /// The absolute URL (scheme + host + path + query).
    pub url: String,
    /// Headers in send order — deterministic output (I3) rides on
    /// never reordering these.
    pub headers: Vec<(String, String)>,
    /// The request body, when the method carries one.
    pub body: Option<String>,
}

impl HttpRequest {
    /// Case-insensitive single-header lookup (first match wins).
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// One inbound HTTP response — status, headers and body. Any status is
/// a value here: interpretation is the adapter's job, not the
/// transport's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpResponse {
    /// The HTTP status code (`200`, `304`, `404`, …).
    pub status: u16,
    /// Response headers as received (the adapter snapshots the
    /// `x-ratelimit-*`/`etag`/`retry-after` names it owns).
    pub headers: Vec<(String, String)>,
    /// The response body (text; REST v3 bodies are JSON/UTF-8).
    pub body: String,
}

impl HttpResponse {
    /// Case-insensitive single-header lookup (first match wins).
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// The transport effect seam: one execute per HTTP exchange. Adapters
/// are handed concrete transports (or `Box<dyn HttpTransport>`); the
/// seam is object-safe for the same reason [`crate::Forge`] is.
///
/// # Errors
///
/// [`ForgeError::Transport`] for connection-level failures (DNS, TLS,
/// refused, timeout) and unusable server outcomes (5xx). HTTP statuses
/// — including errors — are values on [`HttpResponse`].
pub trait HttpTransport: Send + Sync {
    /// Executes one HTTP exchange.
    ///
    /// # Errors
    ///
    /// [`ForgeError::Transport`] as above; the transport never
    /// interprets statuses itself.
    fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ForgeError>;
}

/// The scripted [`HttpTransport`] — the transport analogue of
/// [`MemoryForge`](crate::MemoryForge): a FIFO of recorded outcomes
/// (responses or failures) plus the sent-request log the contract
/// tests assert against. Deterministic (I3): no clock, no network, no
/// randomness. Cloning shares the script and the log (an `Arc` inner —
/// the adapter and the test driver hold the same transport).
#[derive(Clone, Debug, Default)]
pub struct MemoryTransport {
    inner: Arc<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    script: Mutex<VecDeque<Result<HttpResponse, ForgeError>>>,
    sent: Mutex<Vec<HttpRequest>>,
}

impl MemoryTransport {
    /// A fresh, empty script.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a scripted success — the next [`HttpTransport::execute`]
    /// returns it verbatim.
    pub fn enqueue(&self, response: HttpResponse) {
        self.inner.script().push_back(Ok(response));
    }

    /// Records a scripted failure — the next execute surfaces it
    /// verbatim (the §15.2 failure injector at the transport layer).
    pub fn enqueue_failure(&self, error: ForgeError) {
        self.inner.script().push_back(Err(error));
    }

    /// The requests executed so far, oldest first — symmetric
    /// observability with the forge call log (§11.5: a dropped call is
    /// an unobservable one, so even a failing exchange records its
    /// request).
    #[must_use]
    pub fn sent(&self) -> Vec<HttpRequest> {
        self.inner.sent_log().clone()
    }
}

impl Inner {
    /// Locks the script; poisoning cannot corrupt the invariants (the
    /// script is a total FIFO), so the guard is recovered and the
    /// panic propagates from the *original* site.
    fn script(&self) -> MutexGuard<'_, VecDeque<Result<HttpResponse, ForgeError>>> {
        self.script.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Locks the sent log (same poisoning discipline).
    fn sent_log(&self) -> MutexGuard<'_, Vec<HttpRequest>> {
        self.sent.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl HttpTransport for MemoryTransport {
    fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ForgeError> {
        self.inner.sent_log().push(request.clone());
        self.inner.script().pop_front().unwrap_or_else(|| {
            Err(ForgeError::transport(format!(
                "memory transport script exhausted by {} {}",
                request.method, request.url
            )))
        })
    }
}
