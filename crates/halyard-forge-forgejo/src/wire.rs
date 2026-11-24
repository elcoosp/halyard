//! The pure wire layer of the Forgejo REST adapter (M7 step 2):
//! endpoint construction, request builders and response parsers —
//! plain functions over the research-verified shapes
//! (`docs/research/forgejo-rest-forge.md`). No I/O, no clock, no
//! environment: the [`crate::forge::ForgejoRestForge`] assembles the
//! exchanges, this module owns the grammar. Unparseable responses map
//! to [`ForgeError::Transport`] (the exchange produced nothing usable),
//! never to a guessed success.

use std::fmt::Write as _;

use serde::Deserialize;

use halyard_forge::error::ForgeError;
use halyard_forge::spec::{PrSpec, ReleaseSpec};

/// The §11.2 release-branch prefix (`halyard/release/<base>`); the pull
/// request's `base` is the documented inverse — the prefix stripped
/// (the ADR-0013 decision-6 mapping, carried to the Forgejo adapter by
/// ADR-0050), so `upsert_pr` spends no extra probe on it.
pub const RELEASE_BRANCH_PREFIX: &str = "halyard/release/";

/// The Forgejo REST endpoint coordinates: the `owner/repo` path plus
/// the API base (`https://codeberg.org/api/v1` by default — the hosted
/// Forgejo instance the origin grammar recognizes; `gitea.com`,
/// self-hosted instances and the offline lane override it, research
/// §1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgejoEndpoint {
    owner: String,
    repo: String,
    api_base: String,
}

impl ForgejoEndpoint {
    /// Validates and builds the endpoint against the Codeberg API base
    /// (`E-FORGE-REQUEST` on an unusable `owner/repo` path).
    ///
    /// # Errors
    ///
    /// [`ForgeError::Request`] when the path is empty,
    /// whitespace-bearing, or not exactly two non-empty `/`-separated
    /// segments (the `/repos/{owner}/{repo}` grammar — Forgejo has no
    /// nested groups; the request never leaves the process).
    pub fn new(owner_repo: &str) -> Result<Self, ForgeError> {
        Self::with_base(owner_repo, "https://codeberg.org/api/v1")
    }

    /// Validates and builds the endpoint against an explicit API base
    /// (`FORGEJO_API_URL` overrides; the base already carries the
    /// `/api/v1` root, which is never appended implicitly).
    ///
    /// # Errors
    ///
    /// [`ForgeError::Request`] as [`ForgejoEndpoint::new`], plus a base
    /// that is empty, not http(s) or not a bare authority.
    pub fn with_base(owner_repo: &str, api_base: &str) -> Result<Self, ForgeError> {
        let base = api_base.trim_end_matches('/');
        let mut segments = owner_repo.split('/');
        let owner = segments.next().unwrap_or_default();
        let repo = segments.next().unwrap_or_default();
        let overflow = segments.next().is_some();
        if owner.is_empty()
            || repo.is_empty()
            || overflow
            || owner_repo.bytes().any(|b| b.is_ascii_whitespace())
        {
            return Err(ForgeError::request(format!(
                "invalid Forgejo repository path {owner_repo:?}: it must be exactly two \
                 non-empty `/`-separated segments (owner/repo, no nested groups — research §1)"
            )));
        }
        if !(base.starts_with("https://") || base.starts_with("http://")) {
            return Err(ForgeError::request(format!(
                "invalid Forgejo API base {base:?}: it must be an http(s) URL (the `/api/v1` \
                 root included, e.g. https://codeberg.org/api/v1)"
            )));
        }
        Ok(Self {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
            api_base: base.to_owned(),
        })
    }

    /// The repository owner (the path's first segment).
    #[must_use]
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// The repository name (the path's second segment).
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

    /// The `/repos/{owner}/{repo}` prefix every repository-scoped path
    /// starts with (each segment percent-encoded).
    #[must_use]
    pub fn repo_prefix(&self) -> String {
        format!(
            "/repos/{}/{}",
            encode_path_segment(&self.owner),
            encode_path_segment(&self.repo)
        )
    }

    /// Parses a `codeberg.org` / `gitea.com` remote URL (https / `git@`
    /// / `ssh://` forms) into an endpoint against the matched host's
    /// API base — the shell's origin-detection half of the wire-up
    /// (research §1: "one adapter, two names"). A foreign host or an
    /// unusable path is `None` — the caller reports the decline, never
    /// a guess.
    #[must_use]
    pub fn from_remote(remote_url: &str) -> Option<Self> {
        const HOSTS: [&str; 2] = ["codeberg.org", "gitea.com"];
        for host in HOSTS {
            if let Some(path) = strip_host(remote_url, host) {
                let path = path.strip_suffix('/').unwrap_or(path);
                let path = path.strip_suffix(".git").unwrap_or(path);
                if path.is_empty() || path.contains(' ') || path.split('/').any(str::is_empty) {
                    return None;
                }
                return Self::with_base(path, &format!("https://{host}/api/v1")).ok();
            }
        }
        None
    }
}

/// Strips the scheme + host prefix for one recognized host, returning
/// the repository path. The https form requires the exact host (the
/// next character is `/`) — port-bearing and look-alike hosts are the
/// `FORGEJO_API_URL` override's territory, never this grammar's.
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
/// [`Pr`](halyard_forge::spec::Pr) carries; `title`/`body`/`html_url`
/// are nullable on the wire).
#[derive(Debug, Deserialize)]
pub struct RestPullRequest {
    /// The repo-scoped pull index (`PullRequest.number`).
    pub number: u64,
    /// Current title (nullable on the wire).
    pub title: Option<String>,
    /// Current body (nullable on the wire).
    pub body: Option<String>,
    /// Web URL (nullable on the wire).
    pub html_url: Option<String>,
    /// The pull's head branch info (the find filter's confirmation).
    pub head: Option<RestHead>,
}

/// The pull request's head branch (`PRBranchInfo`; `ref` is the branch
/// name the find probe confirms client-side, research §2).
#[derive(Debug, Deserialize)]
pub struct RestHead {
    /// The head branch name (renamed from the wire's `ref`).
    #[serde(rename = "ref")]
    pub ref_field: Option<String>,
}

/// A parsed release body (`tag_name` + `html_url`; research §3).
#[derive(Debug, Deserialize)]
pub struct RestRelease {
    /// The released tag.
    pub tag_name: Option<String>,
    /// The release's web URL (mapped onto the seam's `Release::url`;
    /// absent is the empty string, never composed).
    pub html_url: Option<String>,
}

/// A parsed comment body (the fields the marker scan needs; a PR **is**
/// an issue on this API, research §4).
#[derive(Clone, Debug, Deserialize)]
pub struct RestComment {
    /// The comment id (the rewrite address).
    pub id: u64,
    /// The comment body (the marker scan's find key).
    pub body: Option<String>,
    /// Web URL when the deployment sends it (never fabricated).
    pub html_url: Option<String>,
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

/// Parses a release body.
///
/// # Errors
///
/// [`ForgeError::Transport`] when the body is not a release object.
pub fn parse_release(body: &str) -> Result<RestRelease, ForgeError> {
    serde_json::from_str(body)
        .map_err(|error| ForgeError::transport(format!("the release body did not parse: {error}")))
}

/// Parses a comment list (the marker scan's body).
///
/// # Errors
///
/// [`ForgeError::Transport`] when the body is not a JSON array of
/// comments.
pub fn parse_comment_list(body: &str) -> Result<Vec<RestComment>, ForgeError> {
    serde_json::from_str(body)
        .map_err(|error| ForgeError::transport(format!("the comment list did not parse: {error}")))
}

/// Parses one comment body (the rewrite/create responses are single
/// objects, unlike the scan's list).
///
/// # Errors
///
/// [`ForgeError::Transport`] when the body is not a comment object.
pub fn parse_comment(body: &str) -> Result<RestComment, ForgeError> {
    serde_json::from_str(body)
        .map_err(|error| ForgeError::transport(format!("the comment body did not parse: {error}")))
}

/// The server's own diagnosis: the spec's `APIError` is
/// `{"message": "…"}` (the `url` field is the doc link, not a
/// diagnosis); validation failures ride `422` with a body the same
/// reader renders. Anything else is truncated raw text (§15.4 errors
/// teach).
#[must_use]
pub fn server_message(body: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return body.chars().take(200).collect();
    };
    if let Some(message) = value.get("message") {
        if let Some(text) = message.as_str() {
            return text.to_owned();
        }
        return serde_json::to_string(message).unwrap_or_else(|_| message.to_string());
    }
    body.chars().take(200).collect()
}

/// Percent-encodes one path segment (the RFC 3986 unreserved set rides
/// verbatim; everything else is `%XX`-escaped — the repository path's
/// segments are plain `owner`/`repo` names, the encoder is the same
/// belt-and-braces the GitLab adapter ships).
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

/// The pull-create body (`POST /repos/{owner}/{repo}/pulls`; research
/// §2: `head`, `base`, `title`, `body` — the `CreatePullRequestOption`
/// fields the adapter sends).
#[must_use]
pub fn pr_create_body(spec: &PrSpec, base: &str) -> String {
    serde_json::json!({
        "head": spec.branch.as_str(),
        "base": base,
        "title": spec.title,
        "body": spec.body,
    })
    .to_string()
}

/// The pull-rewrite body (`PATCH /repos/{owner}/{repo}/pulls/{index}`;
/// the `EditPullRequestOption` fields the adapter sends — a rewrite
/// never carries `state`, research §2).
#[must_use]
pub fn pr_update_body(spec: &PrSpec) -> String {
    serde_json::json!({
        "title": spec.title,
        "body": spec.body,
    })
    .to_string()
}

/// The pull-close body (`PATCH …/pulls/{index}` with
/// `state: "closed"` — the verified close, research §8).
#[must_use]
pub fn pr_close_body() -> String {
    serde_json::json!({ "state": "closed" }).to_string()
}

/// The release-create body (`POST /repos/{owner}/{repo}/releases`;
/// research §3: `tag_name` is the definition's only required field, the
/// notes ride `body`, and the git tag exists before the call — the
/// §11.1 tag phase creates it git-side — so `target_commitish` is
/// omitted).
#[must_use]
pub fn release_create_body(spec: &ReleaseSpec) -> String {
    serde_json::json!({
        "tag_name": spec.tag,
        "body": spec.notes,
    })
    .to_string()
}

/// The comment body (`body`, research §4 — the only required field of
/// `CreateIssueCommentOption` / `EditIssueCommentOption`).
#[must_use]
pub fn comment_body(body: &str) -> String {
    serde_json::json!({ "body": body }).to_string()
}
