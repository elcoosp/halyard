//! [`ForgejoRestForge`] — the Forgejo/Gitea REST adapter (M7 step 2,
//! §17 "more forges … Gitea/Forgejo"): a full [`Forge`] implementation
//! over the shared REST v1 surface, driven through an injected
//! [`HttpTransport`] so contract tests run it against the scripted
//! [`MemoryTransport`](halyard_forge::MemoryTransport) and the CLI runs
//! it against [`UreqTransport`](crate::UreqTransport).
//!
//! Semantics (all pinned by tests, mapping per
//! `docs/research/forgejo-rest-forge.md`):
//!
//! * **Idempotence (I1)** — `upsert_pr` finds by the `head` filter then
//!   rewrites in place (`PATCH …/pulls/{index}`); `create_release` is
//!   GET-by-tag-first (`GET /releases/tags/{tag}`, 404 ⇒ the create);
//!   `close_pr` reports the not-found value and the vanished-pull race
//!   as success. The closed pull is never reused: the next upsert opens
//!   a fresh index.
//! * **Budgets (§11.5)** — one recorded [`CallRecord`] per HTTP call
//!   (`method` = verb, `target` = path+query); the update path is find +
//!   PATCH — two calls, the §11.4 shape.
//! * **The documented decline (§7.3)** — `commit_via_api` answers
//!   [`ForgeError::Unsupported`] naming the adapter: the contents API
//!   writes one file per commit, and a release commit carries the
//!   plan's whole `EditSet` as ONE `chore: release` commit — a
//!   per-file commit train is semantically wrong (research §5). The
//!   Forgejo lane rides `commit_mode = "git"` (the §11.2 CAS push).
//! * **Fail closed** — no token ⇒ [`ForgeError::Auth`] before any call;
//!   no hidden retries (the pacing governor rides the live path);
//!   `429` maps to [`ForgeError::RateLimited`] with the standard
//!   `retry-after` (else the 60 s floor); the documented `409` on the
//!   pull PATCH maps to [`ForgeError::Conflict`].
//! * **`base` derivation** — `upsert_pr` strips the §11.2
//!   [`RELEASE_BRANCH_PREFIX`]; a non-release branch is an
//!   [`ForgeError::Request`] caller bug before any call (the ADR-0013
//!   decision-6 mapping, carried over by ADR-0050).
//! * **Conscious differences from the other adapters (ADR-0050)** — no
//!   `ETag` conditionals (not defined on this surface: research §7), and
//!   the `RateInfo` snapshot stays an honest zero until a deployment
//!   sends quota headers (research §6 — none are defined).

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

use crate::wire::{self, ForgejoEndpoint, RELEASE_BRANCH_PREFIX};

/// The adapter's mutable state: the last-observed rate snapshot and the
/// call log. Poisoning cannot corrupt any invariant (every mutation is
/// a total replace or append), so guards are recovered and the panic
/// propagates from the original site — the `MemoryForge` discipline.
#[derive(Debug)]
struct AdapterState {
    /// Last observed quota headers (zeros before any response —
    /// nothing observed yet; research §6).
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

/// The Forgejo REST [`Forge`]. Generic over the transport so benches and
/// tests can pin the wire behavior offline; the CLI injects the real
/// client. Credentials are the static `Authorization: token` header
/// (a personal access token or application token — research §1).
pub struct ForgejoRestForge<T: HttpTransport> {
    endpoint: ForgejoEndpoint,
    /// An empty token fails closed with `E-FORGE-AUTH` before any call.
    token: Arc<str>,
    transport: T,
    state: Mutex<AdapterState>,
}

impl<T: HttpTransport> ForgejoRestForge<T> {
    /// Builds the adapter over `endpoint` with the given token (empty =
    /// anonymous — every method then fails closed) and the injected
    /// transport.
    #[must_use]
    pub fn with_transport(endpoint: ForgejoEndpoint, token: &str, transport: T) -> Self {
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

    /// Builds the mandatory Forgejo headers (research §1): the
    /// `Authorization: token` auth over a usable token — an empty token
    /// fails closed before anything reaches the wire — plus the UA
    /// convention the GitHub adapter established.
    fn headers(&self) -> Result<Vec<(String, String)>, ForgeError> {
        if self.token.is_empty() {
            return Err(ForgeError::auth(
                "no forge token configured; export FORGEJO_TOKEN (or GITEA_TOKEN, or \
                 HALYARD_FORGE_TOKEN) and re-run `halyard doctor` to verify it can reach \
                 the repository",
            ));
        }
        Ok(vec![
            ("Authorization".to_owned(), format!("token {}", self.token)),
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
    /// snapshots the quota headers when the deployment sends them
    /// (research §6: absent headers are the honest zero, never a
    /// fabricated quota).
    fn call(
        &self,
        state: &mut AdapterState,
        method: &'static str,
        request: HttpRequest,
    ) -> Result<HttpResponse, ForgeError> {
        Self::record(state, method, &request.url, self.endpoint.api_base());
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

    /// Maps an error status onto the taxonomy (research §1, §2, §6).
    /// `context` names the probe so every reason is actionable.
    fn map_status(response: &HttpResponse, context: &str) -> ForgeError {
        let status = response.status;
        let message = wire::server_message(&response.body);
        match status {
            401 | 403 => ForgeError::auth(format!(
                "{context} failed: the forge rejected the credentials ({status}): {message}"
            )),
            // The standard throttling status (research §6: `retry-after`
            // per RFC 9110 §10.2.3, else the 60 s floor).
            429 => ForgeError::rate_limited(retry_wait(response).unwrap_or(60)),
            // A repository the credentials cannot see answers the same
            // 404 a missing repository gets; for this adapter the
            // credentials are the actionable cause, exactly like the
            // other lanes' mapping. (The release probe's 404 is the
            // documented not-found *value* and never reaches here.)
            404 => ForgeError::auth(format!(
                "{context} failed: the repository is unknown to these credentials (404): \
                 {message}"
            )),
            400 | 422 => ForgeError::request(format!(
                "{context} failed: the forge rejected the request ({status}): {message}"
            )),
            // The documented conflict on the pull PATCH (research §8:
            // the 409 a closed/merged head answers with).
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
    /// filtered by the documented `head` parameter (research §2),
    /// confirmed client-side on the parsed `head.ref` field.
    fn find_pull_exchange(
        &self,
        state: &mut AdapterState,
        branch: &BranchName,
        context: &str,
    ) -> Result<Option<Pr>, ForgeError> {
        let headers = self.headers()?;
        let path = format!(
            "{}/pulls?state=open&head={}",
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
        let list = wire::parse_pull_list(&response.body)?;
        let found = list
            .into_iter()
            .find(|pull| {
                pull.head
                    .as_ref()
                    .and_then(|head| head.ref_field.as_deref())
                    .is_some_and(|head| head == branch.as_str())
            })
            .map(|pull| into_seam_pr(pull, branch.as_str()));
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
            "{}/releases/tags/{}",
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
            // (research §3).
            404 => Ok(None),
            status if (200..300).contains(&status) => {
                let release = wire::parse_release(&response.body)?;
                Ok(Some(Release {
                    tag: release.tag_name.unwrap_or_else(|| tag.to_owned()),
                    url: release.html_url.unwrap_or_default(),
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
/// (research §6): the standard `retry-after` header, else the caller's
/// 60 s floor.
fn retry_wait(response: &HttpResponse) -> Option<u64> {
    response
        .header("retry-after")
        .and_then(|value| value.parse().ok())
}

/// Maps a parsed wire pull onto the seam vocabulary.
fn into_seam_pr(pull: wire::RestPullRequest, branch: &str) -> Pr {
    Pr {
        number: pull.number,
        branch: branch.to_owned(),
        title: pull.title.unwrap_or_default(),
        body: pull.body.unwrap_or_default(),
        url: pull.html_url.unwrap_or_default(),
    }
}

impl<T: HttpTransport> Forge for ForgejoRestForge<T> {
    fn find_release_pr(&self, branch: &BranchName) -> Result<Option<Pr>, ForgeError> {
        let mut state = self.state();
        self.find_pull_exchange(&mut state, branch, "the Release-PR probe")
    }

    fn upsert_pr(&self, spec: &PrSpec) -> Result<Pr, ForgeError> {
        let mut state = self.state();
        // The §11.2 inverse mapping (ADR-0050, carried from the GitHub
        // and GitLab adapters' decision 6): the pull's `base` is the
        // release prefix stripped — fail closed before any call on a
        // foreign branch shape.
        let base = spec
            .branch
            .as_str()
            .strip_prefix(RELEASE_BRANCH_PREFIX)
            .filter(|base| !base.is_empty())
            .ok_or_else(|| {
                ForgeError::request(format!(
                    "the Release-PR branch {:?} is not the §11.2 shape halyard/release/<base>; \
                     the pull's base is derived from it, so upsert_pr declines to guess",
                    spec.branch.as_str()
                ))
            })?;
        let existing = self.find_pull_exchange(&mut state, &spec.branch, "the upsert lookup")?;
        let headers = self.headers()?;
        if let Some(found) = existing {
            // Same branch ⇒ same pull, rewritten in place (I1).
            let request = HttpRequest {
                method: "PATCH",
                url: self.endpoint.url(&format!(
                    "{}/pulls/{}",
                    self.endpoint.repo_prefix(),
                    found.number
                )),
                headers,
                body: Some(wire::pr_update_body(spec)),
            };
            let response = self.call(&mut state, "PATCH", request)?;
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
                    .url(&format!("{}/pulls", self.endpoint.repo_prefix())),
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
            .release_by_tag_exchange(&mut state, tag, "the release probe")?
            .is_some())
    }

    fn create_release(&self, spec: &ReleaseSpec) -> Result<Release, ForgeError> {
        let mut state = self.state();
        // GET-by-tag-first (§11.1 step 2, I1). A 404 here means "no
        // release for this tag" — but it may equally mean "repository
        // invisible"; the POST below disambiguates, because a repository
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
            url: release.html_url.unwrap_or_default(),
            created: true,
        })
    }

    fn close_pr(&self, branch: &BranchName) -> Result<bool, ForgeError> {
        let mut state = self.state();
        let Some(found) = self.find_pull_exchange(&mut state, branch, "the close probe")? else {
            // Not-found is a value (§11.4); the re-close is a no-op (I1).
            return Ok(false);
        };
        let headers = self.headers()?;
        let request = HttpRequest {
            method: "PATCH",
            url: self.endpoint.url(&format!(
                "{}/pulls/{}",
                self.endpoint.repo_prefix(),
                found.number
            )),
            headers,
            body: Some(wire::pr_close_body()),
        };
        let response = self.call(&mut state, "PATCH", request)?;
        if response.status == 404 {
            // The pull vanished between the find and the close (a
            // racing actor closed or deleted it): the end state is
            // already the intended one (I1), so the pass reports
            // success.
            return Ok(true);
        }
        if !(200..300).contains(&response.status) {
            return Err(Self::map_status(&response, "the Release-PR close"));
        }
        Ok(true)
    }

    fn commit_via_api(&self, spec: &ApiCommit) -> Result<Oid, ForgeError> {
        // The documented honest decline (research §5): the contents API
        // writes ONE file per commit — a release commit carries the
        // plan's whole EditSet as a single `chore: release` commit, and
        // a per-file commit train is semantically wrong for that. The
        // Forgejo lane rides `commit_mode = "git"` (the §11.2 CAS push
        // the M4.2 remainder shipped); the conformance matrix reports
        // this as the `Declined` verdict the M7 suite was built for.
        let _ = spec;
        Err(ForgeError::unsupported("ForgejoRestForge"))
    }

    fn upsert_pr_comment(&self, spec: &CommentSpec) -> Result<PrComment, ForgeError> {
        let mut state = self.state();
        // The shared fail-closed spec contract: PR number, marker,
        // body-with-marker — an invalid spec never reaches the wire.
        spec.validate()?;
        // A pull request **is** an issue on this API: the scan
        // addresses the PR's issue index (research §4). The endpoint
        // documents no pagination parameters (`since`/`before` only),
        // so the marker scan walks the returned array once.
        let scan_path = format!(
            "{}/issues/{}/comments",
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
        let comments = wire::parse_comment_list(&response.body)?;
        if let Some(found) = comments
            .iter()
            .find(|comment| {
                comment
                    .body
                    .as_deref()
                    .is_some_and(|body| body.contains(&spec.marker))
            })
            .cloned()
        {
            let headers = self.headers()?;
            let request = HttpRequest {
                method: "PATCH",
                url: self.endpoint.url(&format!(
                    "{}/issues/comments/{}",
                    self.endpoint.repo_prefix(),
                    found.id
                )),
                headers,
                body: Some(wire::comment_body(&spec.body)),
            };
            let response = self.call(&mut state, "PATCH", request)?;
            if !(200..300).contains(&response.status) {
                return Err(Self::map_status(&response, "the check-comment rewrite"));
            }
            let updated = wire::parse_comment(&response.body)?;
            return Ok(PrComment {
                url: updated.html_url.unwrap_or_default(),
                created: false,
            });
        }
        let headers = self.headers()?;
        let request = HttpRequest {
            method: "POST",
            url: self.endpoint.url(&scan_path),
            headers,
            body: Some(wire::comment_body(&spec.body)),
        };
        let response = self.call(&mut state, "POST", request)?;
        if !(200..300).contains(&response.status) {
            return Err(Self::map_status(&response, "the check-comment creation"));
        }
        let created = wire::parse_comment(&response.body)?;
        Ok(PrComment {
            url: created.html_url.unwrap_or_default(),
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
