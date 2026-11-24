//! [`UreqTransport`] — the real [`HttpTransport`] (M7 step 2): blocking
//! [`ureq`] with rustls TLS, the §19 default decision 4 ("smallest, no
//! async runtime"; §14.2 forbids tokio in this codebase's path). The
//! client maps every HTTP *status* onto a response value and only
//! connection-level failures onto [`ForgeError::Transport`] — statuses
//! are the adapter's interpretation business, not the transport's.
//!
//! The Forgejo adapter's twin of the GitHub and GitLab adapters'
//! transports (ADR-0050): the same shape, the same timeouts, its own
//! crate so the adapters stay independently buildable.

use std::time::Duration;

use halyard_forge::error::ForgeError;
use halyard_forge::transport::{HttpRequest, HttpResponse, HttpTransport};
use ureq::Agent;

/// The connect timeout (fail closed, never hang CI).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// The overall response timeout (research-sized REST bodies return in
/// far less; the ceiling exists so a stalled socket cannot wedge a run).
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(60);

/// The blocking-ureq transport.
#[derive(Debug)]
pub struct UreqTransport {
    agent: Agent,
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqTransport {
    /// Builds the transport with the documented timeouts.
    #[must_use]
    pub fn new() -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout_connect(CONNECT_TIMEOUT)
                .timeout(RESPONSE_TIMEOUT)
                .build(),
        }
    }
}

impl HttpTransport for UreqTransport {
    fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ForgeError> {
        let mut outbound = self.agent.request(request.method, &request.url);
        for (name, value) in &request.headers {
            outbound = outbound.set(name, value);
        }
        // A body-bearing request sends its body; a bodyless one (every
        // GET this adapter builds) goes out as-is.
        let outcome = match request.body.as_deref() {
            Some(body) => outbound.send_string(body),
            None => outbound.call(),
        };
        match outcome {
            Ok(response) => into_response(response),
            // Status errors are values: the adapter maps them per
            // endpoint (research §1/§2/§6), so the transport hands the
            // full response over.
            Err(ureq::Error::Status(_status, response)) => into_response(response),
            Err(ureq::Error::Transport(inner)) => Err(ForgeError::transport(format!(
                "{} {}: {inner}",
                request.method, request.url
            ))),
        }
    }
}

/// Converts a ureq response (from the `Ok` or the `Error::Status` arm)
/// into the seam vocabulary. Body reads are capped by ureq's own
/// response limit, so a hostile server cannot balloon the run's memory.
fn into_response(response: ureq::Response) -> Result<HttpResponse, ForgeError> {
    let status = response.status();
    // Sorted by name so the captured headers are deterministic (I3);
    // repeated headers keep their wire order per name.
    let mut names = response.headers_names();
    names.sort_unstable();
    let mut headers = Vec::new();
    for name in &names {
        for value in response.all(name) {
            headers.push((name.clone(), value.to_owned()));
        }
    }
    let body = response
        .into_string()
        .map_err(|error| ForgeError::transport(format!("response body read failed: {error}")))?;
    Ok(HttpResponse {
        status,
        headers,
        body,
    })
}
