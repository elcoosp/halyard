//! The pure wire layer of the Bitbucket REST adapter (M7 step 3):
//! endpoint construction, request builders and response parsers —
//! plain functions over the research-verified shapes
//! (`docs/research/bitbucket-rest-forge.md`). No I/O, no clock, no
//! environment: the [`crate::forge::BitbucketRestForge`] assembles the
//! exchanges, this module owns the grammar. Unparseable responses map
//! to [`ForgeError::Transport`] (the exchange produced nothing usable),
//! never to a guessed success.

use std::fmt::Write as _;

use serde::Deserialize;

use halyard_forge::error::ForgeError;
use halyard_forge::spec::PrSpec;

/// The §11.2 release-branch prefix (`halyard/release/<base>`); the pull
/// request's `destination` is the documented inverse — the prefix
/// stripped (the ADR-0013 decision-6 mapping, carried to the Bitbucket
/// adapter by ADR-0051), so `upsert_pr` spends no extra probe on it.
pub const RELEASE_BRANCH_PREFIX: &str = "halyard/release/";

/// The Bitbucket REST endpoint coordinates: the `workspace/repo_slug`
/// path plus the API base (`https://api.bitbucket.org/2.0` by default —
/// "All Bitbucket Cloud requests start with the <https://api.bitbucket.org/2.0>
/// prefix", research §1; the offline lane override replaces the whole
/// base).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BitbucketEndpoint {
    workspace: String,
    repo: String,
    api_base: String,
}

impl BitbucketEndpoint {
    /// Validates and builds the endpoint against the Bitbucket Cloud
    /// API base (`E-FORGE-REQUEST` on an unusable `workspace/repo_slug`
    /// path).
    ///
    /// # Errors
    ///
    /// [`ForgeError::Request`] when the path is empty,
    /// whitespace-bearing, or not exactly two non-empty `/`-separated
    /// segments (the `/repositories/{workspace}/{repo_slug}` grammar —
    /// the request never leaves the process).
    pub fn new(workspace_repo: &str) -> Result<Self, ForgeError> {
        Self::with_base(workspace_repo, "https://api.bitbucket.org/2.0")
    }

    /// Validates and builds the endpoint against an explicit API base
    /// (`BITBUCKET_API_URL` overrides; the base already carries the
    /// `/2.0` root, which is never appended implicitly).
    ///
    /// # Errors
    ///
    /// [`ForgeError::Request`] as [`BitbucketEndpoint::new`], plus a
    /// base that is empty, not http(s) or not a bare authority.
    pub fn with_base(workspace_repo: &str, api_base: &str) -> Result<Self, ForgeError> {
        let base = api_base.trim_end_matches('/');
        let mut segments = workspace_repo.split('/');
        let workspace = segments.next().unwrap_or_default();
        let repo = segments.next().unwrap_or_default();
        let overflow = segments.next().is_some();
        if workspace.is_empty()
            || repo.is_empty()
            || overflow
            || workspace_repo.bytes().any(|b| b.is_ascii_whitespace())
        {
            return Err(ForgeError::request(format!(
                "invalid Bitbucket repository path {workspace_repo:?}: it must be exactly two \
                 non-empty `/`-separated segments (workspace/repo_slug, research §1)"
            )));
        }
        if !(base.starts_with("https://") || base.starts_with("http://")) {
            return Err(ForgeError::request(format!(
                "invalid Bitbucket API base {base:?}: it must be an http(s) URL (the `/2.0` \
                 root included, e.g. https://api.bitbucket.org/2.0)"
            )));
        }
        Ok(Self {
            workspace: workspace.to_owned(),
            repo: repo.to_owned(),
            api_base: base.to_owned(),
        })
    }

    /// The repository workspace (the path's first segment).
    #[must_use]
    pub fn workspace(&self) -> &str {
        &self.workspace
    }

    /// The repository slug (the path's second segment).
    #[must_use]
    pub fn repo(&self) -> &str {
        &self.repo
    }

    /// The API base the endpoint resolves against.
    #[must_use]
    pub fn api_base(&self) -> &str {
        &self.api_base
    }

    /// Joins an endpoint path (leading `/` optional) onto the base —
    /// exactly one separator, never `basepath`.
    #[must_use]
    pub fn url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.api_base.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    /// The `/repositories/{workspace}/{repo_slug}` prefix every
    /// repository-scoped path starts with (each segment
    /// percent-encoded).
    #[must_use]
    pub fn repo_prefix(&self) -> String {
        format!(
            "/repositories/{}/{}",
            encode_path_segment(&self.workspace),
            encode_path_segment(&self.repo)
        )
    }

    /// Parses a `bitbucket.org` remote URL (https / `git@` / `ssh://`
    /// forms) into an endpoint against the hosted API base — the
    /// shell's origin-detection half of the wire-up (research §1). A
    /// foreign host or an unusable path is `None` — the caller reports
    /// the decline, never a guess.
    #[must_use]
    pub fn from_remote(remote_url: &str) -> Option<Self> {
        let path = strip_host(remote_url, "bitbucket.org")?;
        let path = path.strip_suffix('/').unwrap_or(path);
        let path = path.strip_suffix(".git").unwrap_or(path);
        if path.is_empty() || path.contains(' ') || path.split('/').any(str::is_empty) {
            return None;
        }
        Self::with_base(path, "https://api.bitbucket.org/2.0").ok()
    }
}

/// Strips the scheme + host prefix for the recognized host, returning
/// the repository path. The https form requires the exact host (the
/// next character is `/`) — port-bearing and look-alike hosts are the
/// `BITBUCKET_API_URL` override's territory, never this grammar's.
fn strip_host<'a>(remote_url: &'a str, host: &str) -> Option<&'a str> {
    // git@host:path (the scp form).
    if let Some(rest) = remote_url
        .strip_prefix("git@")
        .and_then(|rest| rest.strip_prefix(host))
        && let Some(path) = rest.strip_prefix(':')
    {
        return Some(path);
    }
    // https://host/path.
    if let Some(path) = remote_url
        .strip_prefix("https://")
        .and_then(|rest| rest.strip_prefix(host))
        .and_then(|rest| rest.strip_prefix('/'))
    {
        return Some(path);
    }
    // ssh://git@host[:/]path.
    if let Some(tail) = remote_url
        .strip_prefix("ssh://")
        .and_then(|rest| rest.strip_prefix(&format!("git@{host}")))
        && let Some(path) = tail.strip_prefix(':').or_else(|| tail.strip_prefix('/'))
    {
        return Some(path);
    }
    None
}

/// A parsed pull-request body (the fields the seam's
/// [`Pr`](halyard_forge::spec::Pr) carries; `title`/`description` are
/// nullable on the wire).
#[derive(Debug, Deserialize)]
pub struct RestPullRequest {
    /// The repo-scoped pull index (`PullRequest.id`).
    pub id: u64,
    /// Current title (nullable on the wire).
    pub title: Option<String>,
    /// Current body (nullable on the wire; the wire field is
    /// `description`).
    pub description: Option<String>,
    /// The pull's web links (the `html.href` URL the seam reports).
    pub links: Option<RestLinks>,
    /// The pull's source branch info (the find filter's confirmation).
    pub source: Option<RestSource>,
}

/// A pull request's links (`PullRequest.links`; only the `html` page
/// URL the seam vocabulary carries).
#[derive(Clone, Debug, Deserialize)]
pub struct RestLinks {
    /// The web page link.
    pub html: Option<RestHref>,
}

/// One link target (`{"href": …}`).
#[derive(Clone, Debug, Deserialize)]
pub struct RestHref {
    /// The URL (nullable on the wire).
    pub href: Option<String>,
}

/// The pull request's branch-info side (`source`/`destination`; the
/// find probe confirms `branch.name` client-side, research §2).
#[derive(Clone, Debug, Deserialize)]
pub struct RestSource {
    /// The branch reference.
    pub branch: Option<RestBranchName>,
}

/// The branch name object (`{"name": …}`).
#[derive(Clone, Debug, Deserialize)]
pub struct RestBranchName {
    /// The branch name (nullable on the wire).
    pub name: Option<String>,
}

/// A parsed tag body (`refs/tags/{name}`; research §3 — the release
/// marker on this surface is the tag: `name` + `target.hash` +
/// `links.html`).
#[derive(Clone, Debug, Deserialize)]
pub struct RestTag {
    /// The tag name (verbatim from the wire).
    pub name: Option<String>,
    /// The tag's web links (the `html.href` URL the seam reports).
    pub links: Option<RestLinks>,
}

/// A parsed comment body (the fields the marker scan needs — the wire
/// carries the markdown in `content.raw`, research §4).
#[derive(Clone, Debug, Deserialize)]
pub struct RestComment {
    /// The comment id (the rewrite address).
    pub id: u64,
    /// The comment's rendered content (the marker scan's find key).
    pub content: Option<RestContent>,
    /// Web URL when the deployment sends it (never fabricated).
    pub links: Option<RestLinks>,
}

/// The comment content object (`{"raw": …}`; research §4).
#[derive(Clone, Debug, Deserialize)]
pub struct RestContent {
    /// The raw markdown body (nullable on the wire).
    pub raw: Option<String>,
}

/// One page of a paginated envelope (`size`, `page`, `pagelen`,
/// `next`, `previous`, `values` — research §2/§4); the marker scan
/// follows `next` until a page carries none.
#[derive(Debug, Deserialize)]
pub struct RestCommentPage {
    /// The page's comments.
    pub values: Vec<RestComment>,
    /// The next page's URL (absent on the last page).
    pub next: Option<String>,
}

/// Parses a pull-request list (the find probe's body) — an unparseable
/// body is a transport failure, never a guessed empty list.
///
/// # Errors
///
/// [`ForgeError::Transport`] when the body is not a JSON array of
/// pulls.
pub fn parse_pull_list(body: &str) -> Result<Vec<RestPullRequest>, ForgeError> {
    serde_json::from_str(body)
        .map_err(|error| ForgeError::transport(format!("the pull list did not parse: {error}")))
}

/// Parses one pull-request body.
///
/// # Errors
///
/// [`ForgeError::Transport`] when the body is not a pull object.
pub fn parse_pull(body: &str) -> Result<RestPullRequest, ForgeError> {
    serde_json::from_str(body)
        .map_err(|error| ForgeError::transport(format!("the pull body did not parse: {error}")))
}

/// Parses a tag body (the release-marker probe's response).
///
/// # Errors
///
/// [`ForgeError::Transport`] when the body is not a tag object.
pub fn parse_tag(body: &str) -> Result<RestTag, ForgeError> {
    serde_json::from_str(body)
        .map_err(|error| ForgeError::transport(format!("the tag body did not parse: {error}")))
}

/// Parses one page of the comment scan's envelope.
///
/// # Errors
///
/// [`ForgeError::Transport`] when the body is not a paginated envelope.
pub fn parse_comment_page(body: &str) -> Result<RestCommentPage, ForgeError> {
    serde_json::from_str(body)
        .map_err(|error| ForgeError::transport(format!("the comment page did not parse: {error}")))
}

/// Parses one comment body (the rewrite/create responses are single
/// objects, unlike the scan's envelope).
///
/// # Errors
///
/// [`ForgeError::Transport`] when the body is not a comment object.
pub fn parse_comment(body: &str) -> Result<RestComment, ForgeError> {
    serde_json::from_str(body)
        .map_err(|error| ForgeError::transport(format!("the comment body did not parse: {error}")))
}

/// The server's own diagnosis (research §6 of the error layout): the
/// 2.0 API's error body is `{"type": "error", "error": {"message": …}}`
/// and "only `error.message` is guaranteed". Anything else is truncated
/// raw text (§15.4 errors teach).
#[must_use]
pub fn server_message(body: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return body.chars().take(200).collect();
    };
    if let Some(message) = value.get("error").and_then(|error| error.get("message")) {
        if let Some(text) = message.as_str() {
            return text.to_owned();
        }
        return serde_json::to_string(message).unwrap_or_else(|_| message.to_string());
    }
    body.chars().take(200).collect()
}

/// Percent-encodes one path or query-value segment (the RFC 3986
/// unreserved set rides verbatim; everything else is `%XX`-escaped —
/// the repository path's segments are plain names, the `q` filter's
/// spaces, quotes and `=` are escaped whole, research §2).
#[must_use]
pub fn encode_path_segment(segment: &str) -> String {
    let mut encoded = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

/// The find probe's `q` filter (research §2): the documented grammar's
/// equality over the wire fields — `state = "OPEN" AND
/// source.branch.name = "<branch>"` — URL-encoded whole (the value is
/// one query parameter; the client-side confirmation on the parsed
/// `source.branch.name` stays the truth).
#[must_use]
pub fn find_query(branch: &str) -> String {
    format!(
        "q={}&pagelen=50",
        encode_path_segment(&format!(
            "state = \"OPEN\" AND source.branch.name = \"{branch}\""
        ))
    )
}

/// The pull-create body (`POST /repositories/{workspace}/{repo_slug}/pullrequests`;
/// research §2: the minimum documented body is "title and source,
/// specified by a branch name", `destination` rides the §11.2 inverse
/// mapping explicitly — the default would be `repository.mainbranch` —
/// and the notes ride `description`).
#[must_use]
pub fn pr_create_body(spec: &PrSpec, base: &str) -> String {
    serde_json::json!({
        "title": spec.title,
        "description": spec.body,
        "source": { "branch": { "name": spec.branch.as_str() } },
        "destination": { "branch": { "name": base } },
    })
    .to_string()
}

/// The pull-rewrite body (`PUT …/pullrequests/{id}`; research §2: the
/// update "can be used to change the pull request's branches or
/// description" — the adapter sends the title and description only,
/// never a branch or state change).
#[must_use]
pub fn pr_update_body(spec: &PrSpec) -> String {
    serde_json::json!({
        "title": spec.title,
        "description": spec.body,
    })
    .to_string()
}

/// The comment body (`content.raw`, research §4 — the documented
/// create/update field: "body fields include `content` (with `raw`,
/// `markup`, `html`)").
#[must_use]
pub fn comment_body(body: &str) -> String {
    serde_json::json!({ "content": { "raw": body } }).to_string()
}
