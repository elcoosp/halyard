//! [`GitLabRestForge`] — the GitLab REST adapter (M7 step 1, §17 "more
//! forges … GitLab (MR flow)"): a full [`Forge`] implementation over
//! REST v4, driven through an injected [`HttpTransport`] so contract
//! tests run it against the scripted
//! [`MemoryTransport`](halyard_forge::MemoryTransport) and the CLI runs
//! it against [`UreqTransport`](crate::UreqTransport).
//!
//! Semantics (all pinned by tests, mapping per
//! `docs/research/gitlab-rest-forge.md`):
//!
//! * **Idempotence (I1)** — `upsert_pr` finds by `source_branch` then
//!   rewrites in place (`PUT …/:iid`); `create_release` is
//!   GET-by-tag-first (`GET /releases/:tag`, 404 ⇒ the create);
//!   `close_pr` reports the not-found value and the vanished-MR race as
//!   success. The closed MR is never reused: the next upsert opens a
//!   fresh iid.
//! * **Budgets (§11.5)** — one recorded [`CallRecord`] per HTTP call
//!   (`method` = verb, `target` = path+query); the update path is find +
//!   PUT — two calls, the §11.4 shape. `commit_via_api` probes each
//!   file's existence first (the commits API's `create`/`update` action
//!   kinds are exclusive — research §6), so its budget is files + one
//!   commit call, recorded like every other exchange.
//! * **Fail closed** — no token ⇒ [`ForgeError::Auth`] before any call;
//!   no hidden retries (the pacing governor rides the live path);
//!   `429`/throttled `403` map to [`ForgeError::RateLimited`] with the
//!   server's own `retry-after`; conflicts never force.
//! * **`target_branch` derivation** — `upsert_pr` strips the §11.2
//!   [`RELEASE_BRANCH_PREFIX`]; a non-release branch is an
//!   [`ForgeError::Request`] caller bug before any call (the ADR-0013
//!   decision-6 mapping, carried over by ADR-0049).
//! * **Conscious differences from the GitHub adapter (ADR-0049)** — no
//!   `ETag` conditionals (GitLab does not document them on this surface:
//!   research §3), and `RateInfo` stays an honest zero until a
//!   deployment sends quota headers (research §7).

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

use crate::wire::{self, GitLabEndpoint, RELEASE_BRANCH_PREFIX};

/// The adapter's mutable state: the last-observed rate snapshot and the
/// call log. Poisoning cannot corrupt any invariant (every mutation is
/// a total replace or append), so guards are recovered and the panic
/// propagates from the original site — the `MemoryForge` discipline.
#[derive(Debug)]
struct AdapterState {
    /// Last observed quota headers (zeros before any response —
    /// nothing observed yet; research §7).
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

/// The GitLab REST [`Forge`]. Generic over the transport so benches and
/// tests can pin the wire behavior offline; the CLI injects the real
/// client. Credentials are the static `PRIVATE-TOKEN` (a personal
/// access token — the CI-job-token form is a later slice, research §1).
pub struct GitLabRestForge<T: HttpTransport> {
    endpoint: GitLabEndpoint,
    /// An empty token fails closed with `E-FORGE-AUTH` before any call.
    token: Arc<str>,
    transport: T,
    state: Mutex<AdapterState>,
}

impl<T: HttpTransport> GitLabRestForge<T> {
    /// Builds the adapter over `endpoint` with the given token (empty =
    /// anonymous — every method then fails closed) and the injected
    /// transport.
    #[must_use]
    pub fn with_transport(endpoint: GitLabEndpoint, token: &str, transport: T) -> Self {
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

    /// Builds the mandatory GitLab headers (research §1): the
    /// `PRIVATE-TOKEN` auth over a usable token — an empty token fails
    /// closed before anything reaches the wire — plus the UA
    /// convention the GitHub adapter established.
    fn headers(&self) -> Result<Vec<(String, String)>, ForgeError> {
        if self.token.is_empty() {
            return Err(ForgeError::auth(
                "no forge token configured; export GITLAB_TOKEN (or HALYARD_FORGE_TOKEN) and \
                 re-run `halyard doctor` to verify it can reach the repository",
            ));
        }
        Ok(vec![
            ("PRIVATE-TOKEN".to_owned(), self.token.to_string()),
            (
                "User-Agent".to_owned(),
                format!("halyard/{}", env!("CARGO_PKG_VERSION")),
            ),
        ])
    }

    /// Records one call (path + query as the target) — every adapter
    /// failure is recorded too: a dropped call is an unobservable one
    /// (§11.5).
    fn record(
        state: &mut AdapterState,
        method: &'static str,
        url: &str,
        endpoint: &GitLabEndpoint,
    ) {
        let target = url
            .strip_prefix(endpoint.api_base())
            .unwrap_or(url)
            .to_owned();
        state.calls.push(CallRecord { method, target });
    }

    /// Executes one exchange through the transport, records the call and
    /// snapshots the quota headers when the deployment sends them
    /// (research §7: absent headers are the honest zero, never a
    /// fabricated quota).
    fn call(
        &self,
        state: &mut AdapterState,
        method: &'static str,
        request: HttpRequest,
    ) -> Result<HttpResponse, ForgeError> {
        Self::record(state, method, &request.url, &self.endpoint);
        let response = self.transport.execute(request)?;
        let limit = response
            .header("x-ratelimit-limit")
            .and_then(|value| value.parse().ok());
        let remaining = response
            .header("x-ratelimit-remaining")
            .and_then(|value| value.parse().ok());
        if let Some(limit) = limit {
            state.rate.limit = limit;
        }
        if let Some(remaining) = remaining {
            state.rate.remaining = remaining;
        }
        Ok(response)
    }

    /// Maps an error status onto the taxonomy (research §1, §3, §7).
    /// `context` names the probe so every reason is actionable.
    fn map_status(response: &HttpResponse, context: &str) -> ForgeError {
        let status = response.status;
        let message = wire::server_message(&response.body);
        match status {
            401 => ForgeError::auth(format!(
                "{context} failed: the forge rejected the credentials (401 — an invalid \
                 token): {message}"
            )),
            403 => {
                if let Some(wait) = retry_wait(response) {
                    ForgeError::rate_limited(wait)
                } else {
                    ForgeError::auth(format!(
                        "{context} failed: the credentials lack the required access (403): \
                         {message}"
                    ))
                }
            }
            // The documented throttling status (research §3: the MR
            // update limit; research §7).
            429 => ForgeError::rate_limited(retry_wait(response).unwrap_or(60)),
            // GitLab answers a project the credentials cannot see with
            // the same 404 a missing project gets (the REST's documented
            // error semantics); for this adapter the credentials are the
            // actionable cause, exactly like the GitHub mapping.
            404 => ForgeError::auth(format!(
                "{context} failed: the project is unknown to these credentials (404): {message}"
            )),
            400 => ForgeError::request(format!(
                "{context} failed: the forge rejected the request (400): {message}"
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
    /// upsert MR-lookup and the close path: the open-MR list filtered by
    /// `source_branch` (research §3), confirmed client-side on the
    /// parsed `source_branch` field.
    fn find_mr_exchange(
        &self,
        state: &mut AdapterState,
        branch: &BranchName,
        context: &str,
    ) -> Result<Option<Pr>, ForgeError> {
        let headers = self.headers()?;
        let path = format!(
            "{}/merge_requests?source_branch={}&state=opened",
            self.endpoint.repo_prefix(),
            branch.as_str()
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
        let list = wire::parse_mr_list(&response.body)?;
        let found = list
            .into_iter()
            .find(|mr| {
                mr.source_branch
                    .as_deref()
                    .is_some_and(|source| source == branch.as_str())
            })
            .map(|mr| into_seam_pr(mr, branch.as_str()));
        Ok(found)
    }

    /// The release-by-tag probe shared by `release_exists` and
    /// `create_release` (GET-by-tag-first, §11.1 step 2).
    fn release_by_tag_exchange(
        &self,
        state: &mut AdapterState,
        tag: &str,
        context: &str,
    ) -> Result<Option<Release>, ForgeError> {
        let headers = self.headers()?;
        let path = format!(
            "{}/releases/{}",
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
            // The documented not-found *value* for this probe
            // (research §5).
            404 => Ok(None),
            status if (200..300).contains(&status) => {
                let release = wire::parse_release(&response.body)?;
                Ok(Some(Release {
                    tag: tag.to_owned(),
                    url: release
                        .links
                        .and_then(|links| links.self_url)
                        .unwrap_or_default(),
                    // A GET never creates; `created` is the caller's
                    // decision downstream.
                    created: false,
                }))
            }
            _ => Err(Self::map_status(&response, context)),
        }
    }

    /// The per-file existence probe the commits API's exclusive
    /// `create`/`update` action kinds demand (research §6).
    fn file_exists(
        &self,
        state: &mut AdapterState,
        headers: &[(String, String)],
        branch: &str,
        path: &str,
    ) -> Result<bool, ForgeError> {
        let file_path = path
            .split('/')
            .map(wire::encode_path_segment)
            .collect::<Vec<_>>()
            .join("%2F");
        let request = HttpRequest {
            method: "GET",
            url: self.endpoint.url(&format!(
                "{}/repository/files/{file_path}?ref={branch}",
                self.endpoint.repo_prefix()
            )),
            headers: headers.to_vec(),
            body: None,
        };
        let response = self.call(state, "GET", request)?;
        match response.status {
            200 => Ok(true),
            404 => Ok(false),
            _ => Err(Self::map_status(&response, "the commit file probe")),
        }
    }
}

/// Derives the suggested wait from a rate-limit-shaped response
/// (research §7): `retry-after` wins; anything else is not a documented
/// quota shape (`None` — a plain 403 scope rejection).
fn retry_wait(response: &HttpResponse) -> Option<u64> {
    response
        .header("retry-after")
        .and_then(|value| value.parse().ok())
}

/// Maps a parsed wire MR onto the seam vocabulary.
fn into_seam_pr(mr: wire::RestMr, branch: &str) -> Pr {
    Pr {
        number: mr.iid,
        branch: branch.to_owned(),
        title: mr.title.unwrap_or_default(),
        body: mr.description.unwrap_or_default(),
        url: mr.web_url.unwrap_or_default(),
    }
}

impl<T: HttpTransport> Forge for GitLabRestForge<T> {
    fn find_release_pr(&self, branch: &BranchName) -> Result<Option<Pr>, ForgeError> {
        let mut state = self.state();
        self.find_mr_exchange(&mut state, branch, "the Release-PR probe")
    }

    fn upsert_pr(&self, spec: &PrSpec) -> Result<Pr, ForgeError> {
        let mut state = self.state();
        // The §11.2 inverse mapping (ADR-0049, carried from the GitHub
        // adapter's decision 6): the target is the release prefix
        // stripped — fail closed before any call on a foreign branch
        // shape.
        let target = spec
            .branch
            .as_str()
            .strip_prefix(RELEASE_BRANCH_PREFIX)
            .filter(|target| !target.is_empty())
            .ok_or_else(|| {
                ForgeError::request(format!(
                    "the Release-PR branch {:?} is not the §11.2 shape halyard/release/<base>; \
                     the MR target is derived from it, so upsert_pr declines to guess",
                    spec.branch.as_str()
                ))
            })?;
        let existing = self.find_mr_exchange(&mut state, &spec.branch, "the upsert lookup")?;
        let headers = self.headers()?;
        if let Some(found) = existing {
            // Same branch ⇒ same MR, rewritten in place (I1).
            let request = HttpRequest {
                method: "PUT",
                url: self.endpoint.url(&format!(
                    "{}/merge_requests/{}",
                    self.endpoint.repo_prefix(),
                    found.number
                )),
                headers,
                body: Some(wire::mr_update_body(spec)),
            };
            let response = self.call(&mut state, "PUT", request)?;
            if !(200..300).contains(&response.status) {
                return Err(Self::map_status(&response, "the Release-PR rewrite"));
            }
            let mr = wire::parse_mr(&response.body)?;
            Ok(into_seam_pr(mr, spec.branch.as_str()))
        } else {
            let request = HttpRequest {
                method: "POST",
                url: self
                    .endpoint
                    .url(&format!("{}/merge_requests", self.endpoint.repo_prefix())),
                headers,
                body: Some(wire::mr_create_body(spec, target)),
            };
            let response = self.call(&mut state, "POST", request)?;
            if !(200..300).contains(&response.status) {
                return Err(Self::map_status(&response, "the Release-PR creation"));
            }
            let mr = wire::parse_mr(&response.body)?;
            Ok(into_seam_pr(mr, spec.branch.as_str()))
        }
    }

    fn release_exists(&self, tag: &str) -> Result<bool, ForgeError> {
        let mut state = self.state();
        Ok(self
            .release_by_tag_exchange(&mut state, tag, "the release probe")?
            .is_some())
    }

    fn create_release(&self, spec: &ReleaseSpec) -> Result<Release, ForgeError> {
        let mut state = self.state();
        // GET-by-tag-first (§11.1 step 2, I1). A 404 here means "no
        // release for this tag" — but it may equally mean "project
        // invisible"; the POST below disambiguates, because a project
        // the credentials cannot see fails the create with the same 404
        // and surfaces the auth mapping there.
        if let Some(existing) =
            self.release_by_tag_exchange(&mut state, &spec.tag, "the release probe")?
        {
            let mut found = existing;
            found.created = false;
            return Ok(found);
        }
        let headers = self.headers()?;
        let request = HttpRequest {
            method: "POST",
            url: self
                .endpoint
                .url(&format!("{}/releases", self.endpoint.repo_prefix())),
            headers,
            body: Some(wire::release_create_body(spec)),
        };
        let response = self.call(&mut state, "POST", request)?;
        if !(200..300).contains(&response.status) {
            return Err(Self::map_status(&response, "the release creation"));
        }
        let release = wire::parse_release(&response.body)?;
        Ok(Release {
            tag: release.tag_name.unwrap_or_else(|| spec.tag.clone()),
            url: release
                .links
                .and_then(|links| links.self_url)
                .unwrap_or_default(),
            created: true,
        })
    }

    fn close_pr(&self, branch: &BranchName) -> Result<bool, ForgeError> {
        let mut state = self.state();
        let Some(found) = self.find_mr_exchange(&mut state, branch, "the close probe")? else {
            // Not-found is a value (§11.4); the re-close is a no-op (I1).
            return Ok(false);
        };
        let headers = self.headers()?;
        let request = HttpRequest {
            method: "PUT",
            url: self.endpoint.url(&format!(
                "{}/merge_requests/{}",
                self.endpoint.repo_prefix(),
                found.number
            )),
            headers,
            body: Some(wire::mr_close_body()),
        };
        let response = self.call(&mut state, "PUT", request)?;
        if response.status == 404 {
            // The MR vanished between the find and the close (a racing
            // actor closed or deleted it): the end state is already the
            // intended one (I1), so the pass reports success.
            return Ok(true);
        }
        if !(200..300).contains(&response.status) {
            return Err(Self::map_status(&response, "the Release-PR close"));
        }
        Ok(true)
    }

    fn commit_via_api(&self, spec: &ApiCommit) -> Result<Oid, ForgeError> {
        let mut state = self.state();
        let headers = self.headers()?;
        // The commits API commits against the branch tip in one call
        // (no tree/blob choreography — research §6). The exclusive
        // action kinds demand a per-file existence probe: `update` on a
        // missing file and `create` on an existing one are both
        // rejected, so the probe is the documented-safe
        // disambiguation. Every probe is recorded (§11.5).
        let mut actions = Vec::with_capacity(spec.files.len());
        for file in &spec.files {
            let exists =
                self.file_exists(&mut state, &headers, spec.branch.as_str(), &file.path)?;
            actions.push(wire::commit_action(file, exists));
        }
        let request = HttpRequest {
            method: "POST",
            url: self.endpoint.url(&format!(
                "{}/repository/commits",
                self.endpoint.repo_prefix()
            )),
            headers,
            body: Some(wire::commit_create_body(
                spec.branch.as_str(),
                &spec.message,
                &actions,
            )),
        };
        let response = self.call(&mut state, "POST", request)?;
        if !(200..300).contains(&response.status) {
            return Err(Self::map_status(&response, "the API commit"));
        }
        let sha = wire::parse_created_sha(&response.body)?;
        Oid::from_hex(sha.as_bytes()).ok_or_else(|| {
            ForgeError::transport(format!(
                "the API commit returned a non-object sha: {sha:.200}"
            ))
        })
    }

    fn upsert_pr_comment(&self, spec: &CommentSpec) -> Result<PrComment, ForgeError> {
        // The scan's page size (the offset pagination the API overview
        // documents as the default; the note list defaults to
        // descending, so the ascending walk must ask — research §4).
        const PAGE: u32 = 100;
        let mut state = self.state();
        // The shared fail-closed spec contract: PR number, marker,
        // body-with-marker — an invalid spec never reaches the wire.
        spec.validate()?;
        let mut page = 1_u32;
        loop {
            let path = format!(
                "{}/merge_requests/{}/notes?sort=asc&order_by=created_at&per_page={PAGE}&page={page}",
                self.endpoint.repo_prefix(),
                spec.pr_number
            );
            let headers = self.headers()?;
            let request = HttpRequest {
                method: "GET",
                url: self.endpoint.url(&path),
                headers,
                body: None,
            };
            let response = self.call(&mut state, "GET", request)?;
            if !(200..300).contains(&response.status) {
                return Err(Self::map_status(&response, "the check-comment scan"));
            }
            let notes = wire::parse_note_list(&response.body)?;
            if let Some(found) = notes.iter().find(|note| {
                note.body
                    .as_deref()
                    .is_some_and(|body| body.contains(&spec.marker))
            }) {
                let headers = self.headers()?;
                let request = HttpRequest {
                    method: "PUT",
                    url: self.endpoint.url(&format!(
                        "{}/merge_requests/{}/notes/{}",
                        self.endpoint.repo_prefix(),
                        spec.pr_number,
                        found.id
                    )),
                    headers,
                    body: Some(wire::note_body(&spec.body)),
                };
                let response = self.call(&mut state, "PUT", request)?;
                if !(200..300).contains(&response.status) {
                    return Err(Self::map_status(&response, "the check-comment rewrite"));
                }
                let updated = wire::parse_note(&response.body)?;
                return Ok(PrComment {
                    url: updated.web_url.unwrap_or_default(),
                    created: false,
                });
            }
            if notes.len() < PAGE as usize {
                break;
            }
            page = page.saturating_add(1);
        }
        let headers = self.headers()?;
        let request = HttpRequest {
            method: "POST",
            url: self.endpoint.url(&format!(
                "{}/merge_requests/{}/notes",
                self.endpoint.repo_prefix(),
                spec.pr_number
            )),
            headers,
            body: Some(wire::note_body(&spec.body)),
        };
        let response = self.call(&mut state, "POST", request)?;
        if !(200..300).contains(&response.status) {
            return Err(Self::map_status(&response, "the check-comment creation"));
        }
        let created = wire::parse_note(&response.body)?;
        Ok(PrComment {
            url: created.web_url.unwrap_or_default(),
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
