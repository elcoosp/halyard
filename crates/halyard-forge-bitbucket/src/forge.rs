//! [`BitbucketRestForge`] — the Bitbucket Cloud REST adapter (M7 step
//! 3, §17 "more forges … Bitbucket"): a full [`Forge`] implementation
//! over the shared REST 2.0 surface, driven through an injected
//! [`HttpTransport`] so contract tests run it against the scripted
//! [`MemoryTransport`](halyard_forge::MemoryTransport) and the CLI runs
//! it against [`UreqTransport`](crate::UreqTransport).
//!
//! Semantics (all pinned by tests, mapping per
//! `docs/research/bitbucket-rest-forge.md`):
//!
//! * **Idempotence (I1)** — `upsert_pr` finds by the documented `q`
//!   filter (`state = "OPEN" AND source.branch.name = …`) then
//!   rewrites in place (`PUT …/pullrequests/{id}`); `create_release`
//!   probes the tag (`GET …/refs/tags/{tag}` — the release marker on
//!   this surface: the API group index documents no Releases group,
//!   research §3); `close_pr` reports the not-found value before any
//!   decline POST. The declined pull is never reused: the next upsert
//!   opens a fresh id.
//! * **Budgets (§11.5)** — one recorded [`CallRecord`] per HTTP call
//!   (`method` = verb, `target` = path+query); the update path is find
//!   + PUT — two calls, the §11.4 shape.
//! * **The documented decline (§7.3)** — `commit_via_api` answers
//!   [`ForgeError::Unsupported`] naming the adapter: the commits
//!   resource "does NOT support new commit creation" (research §5), and
//!   a release commit carries the plan's whole `EditSet` as ONE
//!   `chore: release` commit. The Bitbucket lane rides
//!   `commit_mode = "git"` (the §11.2 CAS push).
//! * **Fail closed** — no token ⇒ [`ForgeError::Auth`] before any call;
//!   no hidden retries (the pacing governor rides the live path);
//!   `429` maps to [`ForgeError::RateLimited`] with the standard
//!   `retry-after` (else the 60 s floor); the documented close verb is
//!   the decline POST (research §8), a `400`/`409` on it maps onto the
//!   taxonomy and the re-run converges.
//! * **`destination` derivation** — `upsert_pr` strips the §11.2
//!   [`RELEASE_BRANCH_PREFIX`]; a non-release branch is an
//!   [`ForgeError::Request`] caller bug before any call (the ADR-0013
//!   decision-6 mapping, carried over by ADR-0051). The create always
//!   sends `destination` explicitly — the wire's default would be
//!   `repository.mainbranch` (research §2), never a guess.
//! * **Conscious differences from the other adapters (ADR-0051)** — no
//!   `ETag` conditionals (not documented on this surface: research §7),
//!   and the quota snapshot records only the documented
//!   `X-RateLimit-Limit` header — no remaining-requests header exists,
//!   so `remaining` stays the honest zero (research §6).

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;

use halyard_core::reason::Oid;
use halyard_forge::error::ForgeError;
use halyard_forge::transport::{HttpRequest, HttpResponse, HttpTransport};
use halyard_forge::{
    ApiCommit, BranchName, CallRecord, CommentSpec, Forge, Pr, PrComment, PrSpec, RateInfo,
    Release, ReleaseSpec,
};

use crate::wire::{self, BitbucketEndpoint, RELEASE_BRANCH_PREFIX};

/// The comment scan's page size (research §4: the documented
/// `pagelen` parameter; the walk terminates on a short page — the
/// GitLab lane's ascending-scan rule).
const COMMENT_PAGE_LEN: u32 = 50;

/// The adapter's mutable state: the last-observed rate snapshot and the
/// call log. Poisoning cannot corrupt any invariant (every mutation is
/// a total replace or append), so guards are recovered and the panic
/// propagates from the original site — the `MemoryForge` discipline.
#[derive(Debug)]
struct AdapterState {
    /// Last observed quota header (zeros before any response —
    /// nothing observed yet; research §6: only the limit header is
    /// documented, the remaining count has no header to read).
    rate: RateInfo,
    /// One record per HTTP call, oldest first (§11.5).
    calls: Vec<CallRecord>,
}

impl Default for AdapterState {
    fn default() -> Self {
        Self {
            rate: RateInfo {
                limit: 0,
                remaining: 0,
            },
            calls: Vec::new(),
        }
    }
}

/// The Bitbucket REST [`Forge`]. Generic over the transport so benches
/// and tests can pin the wire behavior offline; the CLI injects the
/// real client. Credentials are the static `Authorization: Bearer`
/// header (an OAuth 2.0 access token — research §1).
pub struct BitbucketRestForge<T: HttpTransport> {
    endpoint: BitbucketEndpoint,
    /// An empty token fails closed with `E-FORGE-AUTH` before any call.
    token: Arc<str>,
    transport: T,
    state: Mutex<AdapterState>,
}

impl<T: HttpTransport> BitbucketRestForge<T> {
    /// Builds the adapter over `endpoint` with the given token (empty =
    /// anonymous — every method then fails closed) and the injected
    /// transport.
    #[must_use]
    pub fn with_transport(endpoint: BitbucketEndpoint, token: &str, transport: T) -> Self {
        Self {
            endpoint,
            token: Arc::from(token),
            transport,
            state: Mutex::new(AdapterState::default()),
        }
    }

    /// Locks the adapter state (the `MemoryForge` poisoning discipline).
    fn state(&self) -> MutexGuard<'_, AdapterState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Builds the mandatory Bitbucket headers (research §1): the
    /// `Authorization: Bearer` auth over a usable token — an empty
    /// token fails closed before anything reaches the wire — plus the
    /// UA convention the other adapters established.
    fn headers(&self) -> Result<Vec<(String, String)>, ForgeError> {
        if self.token.is_empty() {
            return Err(ForgeError::auth(
                "no forge token configured; export BITBUCKET_TOKEN (or HALYARD_FORGE_TOKEN) \
                 and re-run `halyard doctor` to verify it can reach the repository",
            ));
        }
        Ok(vec![
            ("Authorization".to_owned(), format!("Bearer {}", self.token)),
            (
                "User-Agent".to_owned(),
                format!("halyard/{}", env!("CARGO_PKG_VERSION")),
            ),
        ])
    }

    /// Records one call (path + query as the target) — every adapter
    /// failure is recorded too: a dropped call is an unobservable one
    /// (§11.5).
    fn record(state: &mut AdapterState, method: &'static str, url: &str, base: &str) {
        let target = url.strip_prefix(base).unwrap_or(url).to_owned();
        state.calls.push(CallRecord { method, target });
    }

    /// Executes one exchange through the transport, records the call and
    /// snapshots the quota header when the deployment sends it
    /// (research §6: the limit header is the only documented one; the
    /// remaining count stays the honest zero).
    fn call(
        &self,
        state: &mut AdapterState,
        method: &'static str,
        request: HttpRequest,
    ) -> Result<HttpResponse, ForgeError> {
        Self::record(state, method, &request.url, self.endpoint.api_base());
        let response = self.transport.execute(request)?;
        if let Some(limit) = response
            .header("x-ratelimit-limit")
            .and_then(|value| value.parse().ok())
        {
            state.rate.limit = limit;
        }
        Ok(response)
    }

    /// Maps an error status onto the taxonomy (research §1, §2, §6).
    /// `context` names the probe so every reason is actionable.
    fn map_status(response: &HttpResponse, context: &str) -> ForgeError {
        let status = response.status;
        let message = wire::server_message(&response.body);
        match status {
            401 | 403 => ForgeError::auth(format!(
                "{context} failed: the forge rejected the credentials ({status}): {message}"
            )),
            // The standard throttling status (research §6: deployments
            // report 429 on exhaustion; `retry-after` per RFC 9110
            // §10.2.3 when sent, else the 60 s floor).
            429 => ForgeError::rate_limited(retry_wait(response).unwrap_or(60)),
            // A repository the credentials cannot see answers the same
            // 404 a missing repository gets; for this adapter the
            // credentials are the actionable cause, exactly like the
            // other lanes' mapping. (The tag probe's 404 is the
            // documented not-found *value* and never reaches here.)
            404 => ForgeError::auth(format!(
                "{context} failed: the repository is unknown to these credentials (404): \
                 {message}"
            )),
            400 | 422 => ForgeError::request(format!(
                "{context} failed: the forge rejected the request ({status}): {message}"
            )),
            409 => ForgeError::conflict(format!("{context} failed (409): {message}")),
            500..=599 => ForgeError::transport(format!(
                "{context} failed: the forge failed server-side ({status}): {message}"
            )),
            _ => ForgeError::request(format!(
                "{context} failed with an unexpected status ({status}): {message}"
            )),
        }
    }

    /// The `find_release_pr` exchange, shared by the machine probe, the
    /// upsert pull-lookup and the close path: the open-pull list
    /// filtered by the documented `q` grammar (research §2), confirmed
    /// client-side on the parsed `source.branch.name` field.
    fn find_pull_exchange(
        &self,
        state: &mut AdapterState,
        branch: &BranchName,
        context: &str,
    ) -> Result<Option<Pr>, ForgeError> {
        let headers = self.headers()?;
        let path = format!(
            "{}/pullrequests?{}",
            self.endpoint.repo_prefix(),
            wire::find_query(branch.as_str())
        );
        let request = HttpRequest {
            method: "GET",
            url: self.endpoint.url(&path),
            headers,
            body: None,
        };
        let response = self.call(state, "GET", request)?;
        if !(200..300).contains(&response.status) {
            return Err(Self::map_status(&response, context));
        }
        let list = wire::parse_pull_list(&response.body)?;
        let found = list
            .into_iter()
            .find(|pull| {
                pull.source
                    .as_ref()
                    .and_then(|source| source.branch.as_ref())
                    .and_then(|branch_name| branch_name.name.as_deref())
                    .is_some_and(|name| name == branch.as_str())
            })
            .map(|pull| into_seam_pr(pull, branch.as_str()));
        Ok(found)
    }

    /// The tag probe shared by `release_exists` and `create_release`
    /// (research §3: the release marker on this surface is the tag —
    /// the API group index documents no Releases group; `404` is the
    /// documented not-found value).
    fn tag_probe_exchange(
        &self,
        state: &mut AdapterState,
        tag: &str,
        context: &str,
    ) -> Result<Option<Release>, ForgeError> {
        let headers = self.headers()?;
        let path = format!(
            "{}/refs/tags/{}",
            self.endpoint.repo_prefix(),
            wire::encode_path_segment(tag)
        );
        let request = HttpRequest {
            method: "GET",
            url: self.endpoint.url(&path),
            headers,
            body: None,
        };
        let response = self.call(state, "GET", request)?;
        match response.status {
            404 => Ok(None),
            status if (200..300).contains(&status) => {
                let tag_body = wire::parse_tag(&response.body)?;
                Ok(Some(Release {
                    tag: tag_body.name.unwrap_or_else(|| tag.to_owned()),
                    url: tag_body
                        .links
                        .and_then(|links| links.html)
                        .and_then(|html| html.href)
                        .unwrap_or_default(),
                    // A GET never creates; `created` is the caller's
                    // decision downstream.
                    created: false,
                }))
            }
            _ => Err(Self::map_status(&response, context)),
        }
    }
}

/// Derives the suggested wait from a rate-limit-shaped response
/// (research §6): the standard `retry-after` header when a deployment
/// sends one, else the caller's 60 s floor.
fn retry_wait(response: &HttpResponse) -> Option<u64> {
    response
        .header("retry-after")
        .and_then(|value| value.parse().ok())
}

/// Maps a parsed wire pull onto the seam vocabulary.
fn into_seam_pr(pull: wire::RestPullRequest, branch: &str) -> Pr {
    Pr {
        number: pull.id,
        branch: branch.to_owned(),
        title: pull.title.unwrap_or_default(),
        body: pull.description.unwrap_or_default(),
        url: pull
            .links
            .and_then(|links| links.html)
            .and_then(|html| html.href)
            .unwrap_or_default(),
    }
}

impl<T: HttpTransport> Forge for BitbucketRestForge<T> {
    fn find_release_pr(&self, branch: &BranchName) -> Result<Option<Pr>, ForgeError> {
        let mut state = self.state();
        self.find_pull_exchange(&mut state, branch, "the Release-PR probe")
    }

    fn upsert_pr(&self, spec: &PrSpec) -> Result<Pr, ForgeError> {
        let mut state = self.state();
        // The §11.2 inverse mapping (ADR-0051, carried from the GitHub,
        // GitLab and Forgejo adapters' decision 6): the pull's
        // `destination` is the release prefix stripped — fail closed
        // before any call on a foreign branch shape.
        let base = spec
            .branch
            .as_str()
            .strip_prefix(RELEASE_BRANCH_PREFIX)
            .filter(|base| !base.is_empty())
            .ok_or_else(|| {
                ForgeError::request(format!(
                    "the Release-PR branch {:?} is not the §11.2 shape halyard/release/<base>; \
                     the pull's destination is derived from it, so upsert_pr declines to guess",
                    spec.branch.as_str()
                ))
            })?;
        let existing = self.find_pull_exchange(&mut state, &spec.branch, "the upsert lookup")?;
        let headers = self.headers()?;
        if let Some(found) = existing {
            // Same branch ⇒ same pull, rewritten in place (I1).
            let request = HttpRequest {
                method: "PUT",
                url: self.endpoint.url(&format!(
                    "{}/pullrequests/{}",
                    self.endpoint.repo_prefix(),
                    found.number
                )),
                headers,
                body: Some(wire::pr_update_body(spec)),
            };
            let response = self.call(&mut state, "PUT", request)?;
            if !(200..300).contains(&response.status) {
                return Err(Self::map_status(&response, "the Release-PR rewrite"));
            }
            let pull = wire::parse_pull(&response.body)?;
            Ok(into_seam_pr(pull, spec.branch.as_str()))
        } else {
            let request = HttpRequest {
                method: "POST",
                url: self
                    .endpoint
                    .url(&format!("{}/pullrequests", self.endpoint.repo_prefix())),
                headers,
                body: Some(wire::pr_create_body(spec, base)),
            };
            let response = self.call(&mut state, "POST", request)?;
            if !(200..300).contains(&response.status) {
                return Err(Self::map_status(&response, "the Release-PR creation"));
            }
            let pull = wire::parse_pull(&response.body)?;
            Ok(into_seam_pr(pull, spec.branch.as_str()))
        }
    }

    fn release_exists(&self, tag: &str) -> Result<bool, ForgeError> {
        let mut state = self.state();
        Ok(self
            .tag_probe_exchange(&mut state, tag, "the release probe")?
            .is_some())
    }

    fn create_release(&self, spec: &ReleaseSpec) -> Result<Release, ForgeError> {
        let mut state = self.state();
        // The tag probe (research §3): the release marker on this
        // surface is the git tag — the API group index documents no
        // Releases group, so there is no release object to create and
        // halyard invents none (§7.3). The tag present ⇒ the marker is
        // verifiably in place (the git-side tag phase pushed it; the
        // idempotent re-run reports the existed shape, I1). The tag
        // absent ⇒ this surface cannot serve the release act at all —
        // the honest decline, which the conformance matrix reports as
        // the `Declined` verdict the M7 suite was built for.
        match self.tag_probe_exchange(&mut state, &spec.tag, "the release probe")? {
            Some(found) => Ok(found),
            None => Err(ForgeError::unsupported("BitbucketRestForge")),
        }
    }

    fn close_pr(&self, branch: &BranchName) -> Result<bool, ForgeError> {
        let mut state = self.state();
        let Some(found) = self.find_pull_exchange(&mut state, branch, "the close probe")? else {
            // Not-found is a value (§11.4); the re-close is a no-op
            // (I1) — the find filter pins `state = "OPEN"`, so an
            // already-declined pull is the not-found value before any
            // decline POST.
            return Ok(false);
        };
        let headers = self.headers()?;
        // The close verb on this surface is the decline POST (research
        // §8: "Declines the pull request.") — not a state field on the
        // update PUT, which only rewrites title/description.
        let request = HttpRequest {
            method: "POST",
            url: self.endpoint.url(&format!(
                "{}/pullrequests/{}/decline",
                self.endpoint.repo_prefix(),
                found.number
            )),
            headers,
            body: None,
        };
        let response = self.call(&mut state, "POST", request)?;
        if !(200..300).contains(&response.status) {
            return Err(Self::map_status(&response, "the Release-PR close"));
        }
        Ok(true)
    }

    fn commit_via_api(&self, spec: &ApiCommit) -> Result<Oid, ForgeError> {
        // The documented honest decline (research §5): the commits
        // resource "does NOT support new commit creation" — a release
        // commit carries the plan's whole EditSet as a single
        // `chore: release` commit, and this surface offers no atomic
        // multi-file write to carry it. The Bitbucket lane rides
        // `commit_mode = "git"` (the §11.2 CAS push); the conformance
        // matrix reports this as the `Declined` verdict the M7 suite
        // was built for.
        let _ = spec;
        Err(ForgeError::unsupported("BitbucketRestForge"))
    }

    fn upsert_pr_comment(&self, spec: &CommentSpec) -> Result<PrComment, ForgeError> {
        let mut state = self.state();
        // The shared fail-closed spec contract: PR number, marker,
        // body-with-marker — an invalid spec never reaches the wire.
        spec.validate()?;
        // The marker scan walks the documented `page`/`pagelen`
        // pagination (research §4: "The default sorting is oldest to
        // newest" — the first marker hit is the oldest comment carrying
        // it), terminating on a short page — the GitLab lane's
        // ascending-scan rule.
        let mut page = 1_u32;
        loop {
            let scan_path = format!(
                "{}/pullrequests/{}/comments?page={page}&pagelen={COMMENT_PAGE_LEN}",
                self.endpoint.repo_prefix(),
                spec.pr_number
            );
            let headers = self.headers()?;
            let request = HttpRequest {
                method: "GET",
                url: self.endpoint.url(&scan_path),
                headers,
                body: None,
            };
            let response = self.call(&mut state, "GET", request)?;
            if !(200..300).contains(&response.status) {
                return Err(Self::map_status(&response, "the check-comment scan"));
            }
            let envelope = wire::parse_comment_page(&response.body)?;
            let short = envelope.values.len() < COMMENT_PAGE_LEN as usize;
            if let Some(found) = envelope
                .values
                .iter()
                .find(|comment| {
                    comment
                        .content
                        .as_ref()
                        .and_then(|content| content.raw.as_deref())
                        .is_some_and(|body| body.contains(&spec.marker))
                })
                .cloned()
            {
                let headers = self.headers()?;
                let request = HttpRequest {
                    method: "PUT",
                    url: self.endpoint.url(&format!(
                        "{}/pullrequests/{}/comments/{}",
                        self.endpoint.repo_prefix(),
                        spec.pr_number,
                        found.id
                    )),
                    headers,
                    body: Some(wire::comment_body(&spec.body)),
                };
                let response = self.call(&mut state, "PUT", request)?;
                if !(200..300).contains(&response.status) {
                    return Err(Self::map_status(&response, "the check-comment rewrite"));
                }
                let updated = wire::parse_comment(&response.body)?;
                return Ok(PrComment {
                    url: comment_url(&updated),
                    created: false,
                });
            }
            if short {
                break;
            }
            page += 1;
        }
        let headers = self.headers()?;
        let request = HttpRequest {
            method: "POST",
            url: self.endpoint.url(&format!(
                "{}/pullrequests/{}/comments",
                self.endpoint.repo_prefix(),
                spec.pr_number
            )),
            headers,
            body: Some(wire::comment_body(&spec.body)),
        };
        let response = self.call(&mut state, "POST", request)?;
        if !(200..300).contains(&response.status) {
            return Err(Self::map_status(&response, "the check-comment creation"));
        }
        let created = wire::parse_comment(&response.body)?;
        Ok(PrComment {
            url: comment_url(&created),
            created: true,
        })
    }

    fn rate_limit(&self) -> RateInfo {
        let state = self.state();
        state.rate
    }

    fn calls(&self) -> Vec<CallRecord> {
        self.state().calls.clone()
    }
}

/// The comment's web URL (`links.html.href`) — absent is the empty
/// string, never composed (research §4).
fn comment_url(comment: &wire::RestComment) -> String {
    comment
        .links
        .clone()
        .and_then(|links| links.html)
        .and_then(|html| html.href)
        .unwrap_or_default()
}
