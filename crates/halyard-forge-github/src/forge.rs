//! [`GitHubRestForge`] — the GitHub REST adapter (M4.1b, §11.4): a full
//! [`Forge`] implementation over REST v3, driven through an injected
//! [`HttpTransport`] so contract tests run it against the scripted
//! [`MemoryTransport`](halyard_forge::MemoryTransport) and the CLI runs
//! it against [`UreqTransport`](crate::UreqTransport).
//!
//! Semantics (all pinned by tests, mapping per
//! `docs/research/github-rest-forge.md`):
//!
//! * **Idempotence (I1)** — `upsert_pr` finds by branch then rewrites in
//!   place; `create_release` is GET-by-tag-first; `close_pr` reports the
//!   not-found value. The GET/POST race (`422` with
//!   `code: "already_exists"`) surfaces as [`ForgeError::Conflict`]: the
//!   re-run finds the winner's artifact, never duplicates it.
//! * **Budgets (§11.5)** — one recorded [`CallRecord`] per HTTP call
//!   (`method` = verb, `target` = path+query); `find_release_pr` rides a
//!   cached `ETag` (`If-None-Match` → `304`, which does not count against
//!   the primary limit). The update path is find + find + PATCH — the
//!   second lookup is the conditional.
//! * **Fail closed** — no token ⇒ [`ForgeError::Auth`] before any call;
//!   no hidden retries (the pacing governor is M5.5); rate limiting maps
//!   to [`ForgeError::RateLimited`] with the server's own wait;
//!   conflicts never force (`force: false` is explicit on the ref
//!   update).
//! * **`base` derivation** — `upsert_pr` strips the §11.2
//!   [`RELEASE_BRANCH_PREFIX`]; a
//!   non-release branch is an [`ForgeError::Request`] caller bug before
//!   any call.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;
use std::time::{SystemTime, UNIX_EPOCH};

use halyard_core::reason::Oid;
use halyard_forge::error::ForgeError;
use halyard_forge::transport::{HttpRequest, HttpTransport};
use halyard_forge::{
    ApiCommit, ApiFile, BranchName, CallRecord, CommentSpec, Forge, Pr, PrComment, PrSpec,
    RateInfo, Release, ReleaseSpec,
};

use crate::app_auth::{
    AppCredentials, EpochClock, ForgeCredentials, INSTALLATION_TOKEN_TTL_SECS, system_epoch,
};
use crate::wire::{
    self, API_VERSION, ForgeEndpoint, RELEASE_BRANCH_PREFIX, RestCommitObject, RestPr, RestRef,
};

/// The adapter's mutable state: the `ETag` cache per release branch, the
/// cached installation token (App credentials), the last-observed rate
/// snapshot and the call log. Poisoning cannot corrupt any invariant
/// (every mutation is a total replace or append), so guards are
/// recovered and the panic propagates from the original site — the
/// `MemoryForge` discipline.
#[derive(Debug)]
struct AdapterState {
    /// `branch → (etag, cached find result)` — the conditional cache.
    etags: HashMap<String, (String, Option<Pr>)>,
    /// The minted installation token with its validity bound in epoch
    /// seconds (`None` before the first mint, token credentials never
    /// touch this).
    installation_token: Option<(String, u64)>,
    /// Last observed `x-ratelimit-*` snapshot (zeros before any
    /// response — nothing observed yet).
    rate: RateInfo,
    /// One record per HTTP call, oldest first (§11.5).
    calls: Vec<CallRecord>,
}

impl Default for AdapterState {
    fn default() -> Self {
        Self {
            etags: HashMap::new(),
            installation_token: None,
            rate: RateInfo {
                limit: 0,
                remaining: 0,
            },
            calls: Vec::new(),
        }
    }
}

/// The GitHub REST [`Forge`]. Generic over the transport so benches and
/// tests can pin the wire behavior offline; the CLI injects the real
/// client. Credentials are the M4.1b static token or the M4.1c GitHub
/// App (the JWT → installation-token mint is this adapter's own
/// exchange, ADR-0014).
pub struct GitHubRestForge<T: HttpTransport> {
    endpoint: ForgeEndpoint,
    /// The resolved-to-bearer credentials; an empty token fails closed
    /// with `E-FORGE-AUTH` before any call.
    credentials: ForgeCredentials,
    transport: T,
    state: Mutex<AdapterState>,
    /// The injected clock for the App path's token-expiry state (never
    /// read anywhere else — determinism I3 is a pure function of it).
    clock: EpochClock,
}

impl<T: HttpTransport> GitHubRestForge<T> {
    /// Builds the adapter over `endpoint` with the given bearer token
    /// (empty = anonymous — every method then fails closed) and the
    /// injected transport.
    #[must_use]
    pub fn with_transport(endpoint: ForgeEndpoint, token: &str, transport: T) -> Self {
        Self::with_credentials_and_clock(
            endpoint,
            ForgeCredentials::from_token(token),
            transport,
            Arc::new(system_epoch),
        )
    }

    /// Builds the adapter with GitHub App credentials (M4.1c): the
    /// installation token is minted on demand through the injected
    /// transport, one POST per
    /// [`INSTALLATION_TOKEN_TTL_SECS`].
    #[must_use]
    pub fn with_app_transport(endpoint: ForgeEndpoint, app: AppCredentials, transport: T) -> Self {
        Self::with_credentials_and_clock(
            endpoint,
            ForgeCredentials::App(Box::new(app)),
            transport,
            Arc::new(system_epoch),
        )
    }

    /// Builds the adapter over explicit credentials with the default
    /// (system) clock.
    #[must_use]
    pub fn with_credentials(
        endpoint: ForgeEndpoint,
        credentials: ForgeCredentials,
        transport: T,
    ) -> Self {
        Self::with_credentials_and_clock(endpoint, credentials, transport, Arc::new(system_epoch))
    }

    /// Builds the adapter over explicit credentials and an injected
    /// clock — the tests' and examples' determinism surface: the App
    /// path's expiry state is a pure function of the clock (I3).
    #[must_use]
    pub fn with_credentials_and_clock(
        endpoint: ForgeEndpoint,
        credentials: ForgeCredentials,
        transport: T,
        clock: EpochClock,
    ) -> Self {
        Self {
            endpoint,
            credentials,
            transport,
            state: Mutex::new(AdapterState::default()),
            clock,
        }
    }

    /// Locks the adapter state (the `MemoryForge` poisoning discipline).
    fn state(&self) -> MutexGuard<'_, AdapterState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Resolves the credentials to a bearer and builds the mandatory
    /// GitHub headers (research §1): an unusable credential set fails
    /// closed before anything reaches the wire, and App credentials
    /// mint through the transport when their cached token expired.
    fn headers(&self, state: &mut AdapterState) -> Result<Vec<(String, String)>, ForgeError> {
        let bearer = self.bearer(state)?;
        Ok(mandatory_headers(&bearer))
    }

    /// Resolves the credential set to the full `Authorization` header
    /// value. Token credentials ride as `Bearer <token>` (empty fails
    /// closed); App credentials ride `Bearer <installation token>` from
    /// the cache until [`INSTALLATION_TOKEN_TTL_SECS`] expires it, then
    /// re-mint.
    fn bearer(&self, state: &mut AdapterState) -> Result<String, ForgeError> {
        match &self.credentials {
            ForgeCredentials::Token(token) => {
                if token.is_empty() {
                    return Err(ForgeError::auth(
                        "no forge token configured; export GITHUB_TOKEN (or \
                         HALYARD_FORGE_TOKEN) and re-run `halyard doctor` to verify it can \
                         reach the repository",
                    ));
                }
                Ok(format!("Bearer {token}"))
            }
            ForgeCredentials::App(app) => {
                let now = (self.clock)();
                if let Some((token, valid_until)) = &state.installation_token
                    && now < *valid_until
                {
                    return Ok(format!("Bearer {token}"));
                }
                let token = self.mint_installation_token(state, app, now)?;
                Ok(format!("Bearer {token}"))
            }
        }
    }

    /// The App path's mint exchange (M4.1c, research §7): one
    /// `POST /app/installations/{id}/access_tokens` with the RS256 JWT
    /// as the bearer, the `{ "token": … }` body parsed from the 201 —
    /// recorded like any other call (§11.5), cached with its own
    /// conservative TTL (the wire's `expires_at` is deliberately not
    /// parsed). A failed mint fails closed before any repo call; the
    /// taxonomy maps the mint's statuses exactly like every other
    /// exchange's.
    fn mint_installation_token(
        &self,
        state: &mut AdapterState,
        app: &AppCredentials,
        now: u64,
    ) -> Result<String, ForgeError> {
        let jwt = app.mint_jwt(now)?;
        let headers = mandatory_headers(&format!("Bearer {jwt}"));
        let path = format!("/app/installations/{}/access_tokens", app.installation_id());
        let request = HttpRequest {
            method: "POST",
            url: self.endpoint.url(&path),
            headers,
            body: None,
        };
        let response = self.call(state, "POST", request)?;
        if !(200..300).contains(&response.status) {
            return Err(Self::map_status(&response, "the installation-token mint"));
        }
        let token = wire::parse_installation_token(&response.body)?;
        state.installation_token = Some((
            token.clone(),
            now.saturating_add(INSTALLATION_TOKEN_TTL_SECS),
        ));
        Ok(token)
    }

    /// Records one call (path + query as the target) — every adapter
    /// failure is recorded too: a dropped call is an unobservable one
    /// (§11.5).
    fn record(state: &mut AdapterState, method: &'static str, url: &str, endpoint: &ForgeEndpoint) {
        let target = url
            .strip_prefix(endpoint.api_base())
            .unwrap_or(url)
            .to_owned();
        state.calls.push(CallRecord { method, target });
    }

    /// Executes one exchange through the transport, records the call and
    /// snapshots the rate headers (research §2: headers are the
    /// authoritative budget source).
    fn call(
        &self,
        state: &mut AdapterState,
        method: &'static str,
        request: HttpRequest,
    ) -> Result<halyard_forge::transport::HttpResponse, ForgeError> {
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

    /// Maps an error status onto the taxonomy (research §5). `context`
    /// names the probe so every reason is actionable.
    fn map_status(response: &halyard_forge::transport::HttpResponse, context: &str) -> ForgeError {
        let status = response.status;
        let message = wire::server_message(&response.body);
        match status {
            401 => ForgeError::auth(format!(
                "{context} failed: the forge rejected the credentials (401): {message}"
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
            // The other documented rate-limit status (research §2).
            429 => ForgeError::rate_limited(retry_wait(response).unwrap_or(60)),
            // GitHub does not distinguish "missing" from "forbidden" on
            // repo-scoped probes; for this adapter the credentials are
            // the actionable cause (research §5).
            404 => ForgeError::auth(format!(
                "{context} failed: the repository is unknown to these credentials \
                 (404 — GitHub reports missing and forbidden alike)"
            )),
            409 => ForgeError::conflict(format!("{context} failed (409): {message}")),
            422 => {
                if wire::error_codes(&response.body)
                    .iter()
                    .any(|code| code == "already_exists")
                {
                    ForgeError::conflict(format!(
                        "{context} failed: the artifact already exists (422) — the re-run \
                         finds it (I1, never a duplicate): {message}"
                    ))
                } else {
                    ForgeError::request(format!(
                        "{context} failed: the forge rejected the request (422): {message}"
                    ))
                }
            }
            500..=599 => ForgeError::transport(format!(
                "{context} failed: the forge failed server-side ({status}): {message}"
            )),
            _ => ForgeError::request(format!(
                "{context} failed with an unexpected status ({status}): {message}"
            )),
        }
    }

    /// The `find_release_pr` exchange, shared by the machine probe, the
    /// upsert branch-lookup and the close path: conditional GET of the
    /// open-PR list filtered by `head`.
    fn find_pr_exchange(
        &self,
        state: &mut AdapterState,
        branch: &BranchName,
        context: &str,
    ) -> Result<Option<Pr>, ForgeError> {
        let mut headers = self.headers(state)?;
        if let Some((etag, _)) = state.etags.get(branch.as_str()) {
            headers.push(("If-None-Match".to_owned(), etag.clone()));
        }
        let path = format!(
            "{}/pulls?state=open&head={}:{}",
            self.endpoint.repo_prefix(),
            self.endpoint.owner(),
            branch.as_str()
        );
        let request = HttpRequest {
            method: "GET",
            url: self.endpoint.url(&path),
            headers,
            body: None,
        };
        let response = self.call(state, "GET", request)?;
        if response.status == 304 {
            // A 304 only makes sense after a conditional request; the
            // cache holds the answer. A 304 without a cache entry is a
            // protocol violation — fail closed, never fabricate.
            return state
                .etags
                .get(branch.as_str())
                .map(|(_, cached)| cached.clone())
                .ok_or_else(|| {
                    ForgeError::request(format!(
                        "{context} received an unexpected 304 without a prior conditional \
                         request"
                    ))
                });
        }
        if !(200..300).contains(&response.status) {
            return Err(Self::map_status(&response, context));
        }
        let list = wire::parse_pr_list(&response.body)?;
        let found = list
            .into_iter()
            .find(|pr| {
                pr.head
                    .as_ref()
                    .and_then(|head| head.ref_.as_deref())
                    .is_some_and(|head_ref| head_ref == branch.as_str())
            })
            .map(|pr| into_seam_pr(pr, branch.as_str()));
        match response.header("etag") {
            Some(etag) => {
                state
                    .etags
                    .insert(branch.as_str().to_owned(), (etag.to_owned(), found.clone()));
            }
            None => {
                // No ETag on the wire: the next probe is unconditional.
                state.etags.remove(branch.as_str());
            }
        }
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
        let headers = self.headers(state)?;
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
            // (research §4).
            404 => Ok(None),
            status if (200..300).contains(&status) => {
                let release = wire::parse_release(&response.body)?;
                Ok(Some(Release {
                    tag: tag.to_owned(),
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

/// Derives the suggested wait from a rate-limit-shaped response,
/// following the documented decision tree (research §2):
/// `retry-after` wins; else a zero `x-ratelimit-remaining` waits until
/// `x-ratelimit-reset` (clamped to at least one second, 60 s floor
/// without the header); anything else is not a rate-limit shape
/// (`None` — a plain 403 scope rejection).
fn retry_wait(response: &halyard_forge::transport::HttpResponse) -> Option<u64> {
    if let Some(value) = response.header("retry-after") {
        return Some(value.parse::<u64>().unwrap_or(60));
    }
    let exhausted = response
        .header("x-ratelimit-remaining")
        .is_some_and(|value| value == "0");
    if !exhausted {
        return None;
    }
    let reset = response
        .header("x-ratelimit-reset")
        .and_then(|value| value.parse::<u64>().ok());
    Some(match reset {
        Some(reset) => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|since| since.as_secs())
                .unwrap_or_default();
            reset.saturating_sub(now).max(1)
        }
        None => 60,
    })
}

/// The mandatory GitHub headers (research §1) over a resolved bearer —
/// pure: the credential resolution (and the App path's mint) lives on
/// the adapter.
fn mandatory_headers(bearer: &str) -> Vec<(String, String)> {
    vec![
        (
            "Accept".to_owned(),
            "application/vnd.github+json".to_owned(),
        ),
        ("X-GitHub-Api-Version".to_owned(), API_VERSION.to_owned()),
        (
            "User-Agent".to_owned(),
            format!("halyard/{}", env!("CARGO_PKG_VERSION")),
        ),
        ("Authorization".to_owned(), bearer.to_owned()),
    ]
}

/// Maps a parsed wire PR onto the seam vocabulary.
fn into_seam_pr(pr: RestPr, branch: &str) -> Pr {
    Pr {
        number: pr.number,
        branch: branch.to_owned(),
        title: pr.title.unwrap_or_default(),
        body: pr.body.unwrap_or_default(),
        url: pr.html_url.unwrap_or_default(),
    }
}

impl<T: HttpTransport> Forge for GitHubRestForge<T> {
    fn find_release_pr(&self, branch: &BranchName) -> Result<Option<Pr>, ForgeError> {
        let mut state = self.state();
        self.find_pr_exchange(&mut state, branch, "the Release-PR probe")
    }

    fn upsert_pr(&self, spec: &PrSpec) -> Result<Pr, ForgeError> {
        let mut state = self.state();
        // The §11.2 inverse mapping (ADR-0013 decision 6): the base is
        // the release prefix stripped — fail closed before any call on
        // a foreign branch shape.
        let base = spec
            .branch
            .as_str()
            .strip_prefix(RELEASE_BRANCH_PREFIX)
            .filter(|base| !base.is_empty())
            .ok_or_else(|| {
                ForgeError::request(format!(
                    "the Release-PR branch {:?} is not the §11.2 shape \
                     halyard/release/<base>; the PR base is derived from it, so upsert_pr \
                     declines to guess",
                    spec.branch.as_str()
                ))
            })?;
        let existing = self.find_pr_exchange(&mut state, &spec.branch, "the upsert lookup")?;
        if let Some(found) = existing {
            // Same branch ⇒ same PR, rewritten in place (I1).
            let headers = self.headers(&mut state)?;
            let path = format!("{}/pulls/{}", self.endpoint.repo_prefix(), found.number);
            let request = HttpRequest {
                method: "PATCH",
                url: self.endpoint.url(&path),
                headers,
                body: Some(wire::pr_update_body(spec)),
            };
            let response = self.call(&mut state, "PATCH", request)?;
            if !(200..300).contains(&response.status) {
                return Err(Self::map_status(&response, "the Release-PR rewrite"));
            }
            let pr = wire::parse_pr(&response.body)?;
            Ok(into_seam_pr(pr, spec.branch.as_str()))
        } else {
            let headers = self.headers(&mut state)?;
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
            let pr = wire::parse_pr(&response.body)?;
            Ok(into_seam_pr(pr, spec.branch.as_str()))
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
        // release for this tag" — but it may equally mean "repo
        // invisible"; the POST below disambiguates, because a repo the
        // credentials cannot see fails the create with the same 404 and
        // surfaces the auth mapping there.
        if let Some(existing) =
            self.release_by_tag_exchange(&mut state, &spec.tag, "the release probe")?
        {
            let mut found = existing;
            found.created = false;
            return Ok(found);
        }
        let headers = self.headers(&mut state)?;
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
            tag: spec.tag.clone(),
            url: release.html_url.unwrap_or_default(),
            created: true,
        })
    }

    fn close_pr(&self, branch: &BranchName) -> Result<bool, ForgeError> {
        let mut state = self.state();
        let Some(found) = self.find_pr_exchange(&mut state, branch, "the close probe")? else {
            // Not-found is a value (§11.4); the re-close is a no-op (I1).
            return Ok(false);
        };
        let headers = self.headers(&mut state)?;
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
            // The PR vanished between the find and the close (a racing
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
        let headers = self.headers(&mut state)?;
        // 1–2. Read the ref (the release branch the shell's git side
        // pushed) and its commit's root tree. A missing branch is a moved
        // state — the re-run recreates it before the API commit (I2).
        let (base_commit, base_tree) =
            self.read_branch_head(&mut state, &headers, spec.branch.as_str())?;
        // 3–4. Build the entries (blob uploads for binary payloads) and
        // create the tree on top of the base tree.
        let new_tree = self.create_tree(&mut state, &headers, &base_tree, &spec.files)?;
        // 5. Create the commit.
        let prefix = self.endpoint.repo_prefix();
        let commit_request = HttpRequest {
            method: "POST",
            url: self.endpoint.url(&format!("{prefix}/git/commits")),
            headers: headers.clone(),
            body: Some(wire::commit_create_body(
                &spec.message,
                &new_tree,
                &base_commit,
            )),
        };
        let response = self.call(&mut state, "POST", commit_request)?;
        if !(200..300).contains(&response.status) {
            return Err(Self::map_status(&response, "the API commit"));
        }
        let new_commit = wire::parse_created_sha(&response.body)?;
        // 6. Move the ref — never forced (§11.2); the rejections are the
        // CAS conflict.
        let update_ref = HttpRequest {
            method: "PATCH",
            url: self
                .endpoint
                .url(&format!("{prefix}/git/refs/heads/{}", spec.branch.as_str())),
            headers,
            body: Some(wire::ref_update_body(&new_commit)),
        };
        let response = self.call(&mut state, "PATCH", update_ref)?;
        if !(200..300).contains(&response.status) {
            return Err(Self::ref_update_conflict(&response));
        }
        Oid::from_hex(new_commit.as_bytes()).ok_or_else(|| {
            ForgeError::transport(format!(
                "the API commit returned a non-object sha: {new_commit:.200}"
            ))
        })
    }

    fn upsert_pr_comment(&self, spec: &CommentSpec) -> Result<PrComment, ForgeError> {
        // The scan's page size (the GitHub comment list caps at 100).
        const PAGE: usize = 100;
        let mut state = self.state();
        // The shared fail-closed spec contract: PR number, marker,
        // body-with-marker — an invalid spec never reaches the wire.
        spec.validate()?;
        // The marker scan (§11.6, I1): a deterministic ascending walk —
        // pages of 100 until a short page ends the walk (the GitHub
        // comment list is ascending by id, so the scan is complete; a
        // found marker rewrites in place, an exhausted walk creates).
        let mut page = 1_u32;
        loop {
            let path = format!(
                "{}/issues/{}/comments?per_page={PAGE}&page={page}",
                self.endpoint.repo_prefix(),
                spec.pr_number
            );
            let headers = self.headers(&mut state)?;
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
            let comments = wire::parse_comment_list(&response.body)?;
            if let Some(found) = wire::comment_with_marker(&comments, &spec.marker) {
                // `comment_with_marker` only returns id-bearing
                // comments — the PATCH address exists by construction.
                let id = found
                    .id
                    .unwrap_or_else(|| unreachable!("the marker scan yields id-bearing comments"));
                let headers = self.headers(&mut state)?;
                let request = HttpRequest {
                    method: "PATCH",
                    url: self.endpoint.url(&format!(
                        "{}/issues/comments/{id}",
                        self.endpoint.repo_prefix()
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
            if comments.len() < PAGE {
                break;
            }
            page = page.saturating_add(1);
        }
        let headers = self.headers(&mut state)?;
        let request = HttpRequest {
            method: "POST",
            url: self.endpoint.url(&format!(
                "{}/issues/{}/comments",
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

impl<T: HttpTransport> GitHubRestForge<T> {
    /// Reads the branch head: the ref's commit sha and its root tree sha
    /// (the API-commit chain's base). Steps 1–2 of `commit_via_api`.
    fn read_branch_head(
        &self,
        state: &mut AdapterState,
        headers: &[(String, String)],
        branch: &str,
    ) -> Result<(String, String), ForgeError> {
        let prefix = self.endpoint.repo_prefix();
        let read_ref = HttpRequest {
            method: "GET",
            url: self
                .endpoint
                .url(&format!("{prefix}/git/ref/heads/{branch}")),
            headers: headers.to_vec(),
            body: None,
        };
        let response = self.call(state, "GET", read_ref)?;
        if response.status == 404 {
            return Err(ForgeError::conflict(format!(
                "the API commit failed: the branch {branch:?} is missing — the git-side push \
                 creates it; re-run after the push (I2)"
            )));
        }
        if !(200..300).contains(&response.status) {
            return Err(Self::map_status(&response, "the branch read"));
        }
        let base_commit = into_ref_sha(wire::parse_ref(&response.body)?, branch)?;
        let read_commit = HttpRequest {
            method: "GET",
            url: self
                .endpoint
                .url(&format!("{prefix}/git/commits/{base_commit}")),
            headers: headers.to_vec(),
            body: None,
        };
        let response = self.call(state, "GET", read_commit)?;
        if !(200..300).contains(&response.status) {
            return Err(Self::map_status(&response, "the base commit read"));
        }
        let base_tree = into_tree_sha(wire::parse_commit_object(&response.body)?)?;
        Ok((base_commit, base_tree))
    }

    /// Uploads the commit's file payloads and creates the tree: UTF-8
    /// content rides the tree entry, binary payloads go through the
    /// base64 blob endpoint (research §4). Steps 3–4 of `commit_via_api`.
    fn create_tree(
        &self,
        state: &mut AdapterState,
        headers: &[(String, String)],
        base_tree: &str,
        files: &[ApiFile],
    ) -> Result<String, ForgeError> {
        let prefix = self.endpoint.repo_prefix();
        let mut entries = Vec::with_capacity(files.len());
        for file in files {
            if let Ok(text) = std::str::from_utf8(&file.content) {
                entries.push(serde_json::json!({
                    "path": file.path,
                    "mode": "100644",
                    "type": "blob",
                    "content": text,
                }));
            } else {
                let blob_request = HttpRequest {
                    method: "POST",
                    url: self.endpoint.url(&format!("{prefix}/git/blobs")),
                    headers: headers.to_vec(),
                    body: Some(wire::blob_create_body(&file.content)),
                };
                let response = self.call(state, "POST", blob_request)?;
                if !(200..300).contains(&response.status) {
                    return Err(Self::map_status(&response, "the blob upload"));
                }
                let sha = wire::parse_created_sha(&response.body)?;
                entries.push(serde_json::json!({
                    "path": file.path,
                    "mode": "100644",
                    "type": "blob",
                    "sha": sha,
                }));
            }
        }
        let tree_request = HttpRequest {
            method: "POST",
            url: self.endpoint.url(&format!("{prefix}/git/trees")),
            headers: headers.to_vec(),
            body: Some(wire::tree_create_body(base_tree, &entries)),
        };
        let response = self.call(state, "POST", tree_request)?;
        if !(200..300).contains(&response.status) {
            return Err(Self::map_status(&response, "the tree creation"));
        }
        wire::parse_created_sha(&response.body)
    }

    /// A rejected ref update is the §11.2 CAS conflict regardless of
    /// whether GitHub shapes it as 409 or 422 — the branch moved under
    /// us; the re-run re-plans, never forces.
    fn ref_update_conflict(response: &halyard_forge::transport::HttpResponse) -> ForgeError {
        let reason = wire::server_message(&response.body);
        ForgeError::conflict(format!(
            "the branch update failed (the branch moved under us, re-run; never forced): \
             {reason}"
        ))
    }
}

/// Extracts the base commit sha from a parsed ref body.
fn into_ref_sha(reference: RestRef, branch: &str) -> Result<String, ForgeError> {
    reference
        .object
        .and_then(|object| object.sha)
        .filter(|sha| sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| {
            ForgeError::transport(format!(
                "the ref for {branch:?} carried no commit sha (the exchange produced nothing \
                 usable)"
            ))
        })
}

/// Extracts the root tree sha from a parsed commit object.
fn into_tree_sha(commit: RestCommitObject) -> Result<String, ForgeError> {
    commit
        .tree
        .and_then(|tree| tree.sha)
        .filter(|sha| sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| {
            ForgeError::transport(
                "the commit object carried no tree sha (the exchange produced nothing usable)",
            )
        })
}
