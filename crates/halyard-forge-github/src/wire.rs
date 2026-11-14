//! The pure wire layer of the GitHub REST adapter (M4.1b): endpoint
//! construction, request builders and response parsers — plain
//! functions over the research-verified shapes
//! (`docs/research/github-rest-forge.md`). No I/O, no clock, no
//! environment: the [`crate::forge::GitHubRestForge`] assembles the
//! exchanges, this module owns the grammar. Unparseable responses map
//! to [`ForgeError::Transport`] (the exchange produced nothing usable),
//! never to a guessed success.

use std::fmt::Write as _;

use base64::Engine as _;
use serde::Deserialize;

use halyard_forge::error::ForgeError;
use halyard_forge::spec::{PrSpec, ReleaseSpec};

/// The API version the adapter pins on every request (research §1:
/// `X-GitHub-Api-Version: 2022-11-28`).
pub const API_VERSION: &str = "2022-11-28";

/// The §11.2 release-branch prefix (`halyard/release/<base>`); the PR
/// `base` is the documented inverse — the prefix stripped (ADR-0013
/// decision 6), so `upsert_pr` spends no extra probe on it.
pub const RELEASE_BRANCH_PREFIX: &str = "halyard/release/";

/// The GitHub REST endpoint coordinates: `owner/repo` plus the API
/// base (overridable for GitHub Enterprise and the loopback mock lane;
/// the default is `https://api.github.com`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgeEndpoint {
    owner: String,
    repo: String,
    api_base: String,
}

impl ForgeEndpoint {
    /// Validates and builds the endpoint against the default GitHub
    /// API base (`E-FORGE-REQUEST` on an unusable slug).
    ///
    /// # Errors
    ///
    /// [`ForgeError::Request`] when the owner or repo is empty,
    /// whitespace-bearing or carries a `/` (a slug is exactly two
    /// segments; the request never leaves the process).
    pub fn new(owner: &str, repo: &str) -> Result<Self, ForgeError> {
        Self::with_base(owner, repo, "https://api.github.com")
    }

    /// Validates and builds the endpoint against an explicit API base
    /// (`GITHUB_API_URL` overrides; trailing slashes are trimmed).
    ///
    /// # Errors
    ///
    /// [`ForgeError::Request`] as [`ForgeEndpoint::new`], plus a base
    /// that is empty, not http(s) or not a bare authority.
    pub fn with_base(owner: &str, repo: &str, api_base: &str) -> Result<Self, ForgeError> {
        let base = api_base.trim_end_matches('/');
        if owner.is_empty()
            || repo.is_empty()
            || owner.split('/').count() != 1
            || repo.split('/').count() != 1
        {
            return Err(ForgeError::request(format!(
                "invalid GitHub repository slug {owner:?}/{repo:?}: both halves must be \
                 non-empty single path segments"
            )));
        }
        if !(base.starts_with("https://") || base.starts_with("http://")) {
            return Err(ForgeError::request(format!(
                "invalid GitHub API base {base:?}: it must be an http(s) URL"
            )));
        }
        Ok(Self {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
            api_base: base.to_owned(),
        })
    }

    /// The repository owner.
    #[must_use]
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// The repository name.
    #[must_use]
    pub fn repo(&self) -> &str {
        &self.repo
    }

    /// The API base the endpoint resolves against.
    #[must_use]
    pub fn api_base(&self) -> &str {
        &self.api_base
    }

    /// The `owner/repo` slug (report surfaces).
    #[must_use]
    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
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

    /// The `/repos/{owner}/{repo}` prefix every repo-scoped path starts
    /// with.
    #[must_use]
    pub fn repo_prefix(&self) -> String {
        format!("/repos/{}/{}", self.owner, self.repo)
    }
}

/// A parsed PR body (the fields the seam's [`Pr`](halyard_forge::spec::Pr)
/// carries; `title`/`body`/`html_url` are nullable on the wire).
#[derive(Debug, Deserialize)]
pub struct RestPr {
    /// The forge's PR number.
    pub number: u64,
    /// Current title (nullable on the wire).
    #[serde(default)]
    pub title: Option<String>,
    /// Current body (nullable on the wire).
    #[serde(default)]
    pub body: Option<String>,
    /// Web URL (nullable on the wire).
    #[serde(default)]
    pub html_url: Option<String>,
    /// The head branch object (`head.ref` is the match key).
    #[serde(default)]
    pub head: Option<RestHead>,
}

/// The `head` object of a PR body.
#[derive(Debug, Deserialize)]
pub struct RestHead {
    /// The branch name the PR is opened from.
    #[serde(rename = "ref", default)]
    pub ref_: Option<String>,
}

/// A parsed release body (the GET-by-tag-first idempotence payload).
#[derive(Debug, Deserialize)]
pub struct RestRelease {
    /// Web URL of the release.
    #[serde(default)]
    pub html_url: Option<String>,
}

/// A parsed ref body (`GET /git/ref/heads/…`).
#[derive(Debug, Deserialize)]
pub struct RestRef {
    /// The object the ref points at.
    #[serde(default)]
    pub object: Option<RestRefObject>,
}

/// The object a ref points at.
#[derive(Debug, Deserialize)]
pub struct RestRefObject {
    /// The commit sha the ref resolves to.
    #[serde(default)]
    pub sha: Option<String>,
}

/// A parsed commit object (`GET /git/commits/{sha}`) — the tree sha the
/// API-commit chain needs.
#[derive(Debug, Deserialize)]
pub struct RestCommitObject {
    /// The commit's root tree.
    #[serde(default)]
    pub tree: Option<RestShaRef>,
}

/// A `{ "sha": … }` shape (nested tree fields, POST responses).
#[derive(Debug, Deserialize)]
pub struct RestShaRef {
    /// The object sha.
    #[serde(default)]
    pub sha: Option<String>,
}

/// Parses a PR-list body (`E-FORGE-TRANSPORT` when unreadable).
///
/// # Errors
///
/// [`ForgeError::Transport`] when the body is not the documented JSON
/// array.
pub fn parse_pr_list(body: &str) -> Result<Vec<RestPr>, ForgeError> {
    serde_json::from_str(body)
        .map_err(|error| ForgeError::transport(format!("unparseable PR list body: {error}")))
}

/// Parses a single PR body.
///
/// # Errors
///
/// [`ForgeError::Transport`] when the body is not the documented JSON
/// object.
pub fn parse_pr(body: &str) -> Result<RestPr, ForgeError> {
    serde_json::from_str(body)
        .map_err(|error| ForgeError::transport(format!("unparseable PR body: {error}")))
}

/// Parses a release body.
///
/// # Errors
///
/// [`ForgeError::Transport`] when the body is not the documented JSON
/// object.
pub fn parse_release(body: &str) -> Result<RestRelease, ForgeError> {
    serde_json::from_str(body)
        .map_err(|error| ForgeError::transport(format!("unparseable release body: {error}")))
}

/// Parses a ref body.
///
/// # Errors
///
/// [`ForgeError::Transport`] when the body is not the documented JSON
/// object.
pub fn parse_ref(body: &str) -> Result<RestRef, ForgeError> {
    serde_json::from_str(body)
        .map_err(|error| ForgeError::transport(format!("unparseable ref body: {error}")))
}

/// Parses a commit object body.
///
/// # Errors
///
/// [`ForgeError::Transport`] when the body is not the documented JSON
/// object.
pub fn parse_commit_object(body: &str) -> Result<RestCommitObject, ForgeError> {
    serde_json::from_str(body)
        .map_err(|error| ForgeError::transport(format!("unparseable commit object body: {error}")))
}

/// Parses a created-object sha (`{"sha": …}`).
///
/// # Errors
///
/// [`ForgeError::Transport`] when the body carries no sha.
pub fn parse_created_sha(body: &str) -> Result<String, ForgeError> {
    let parsed: RestShaRef = serde_json::from_str(body)
        .map_err(|error| ForgeError::transport(format!("unparseable object body: {error}")))?;
    parsed
        .sha
        .ok_or_else(|| ForgeError::transport(format!("object body carries no sha: {body:.200}")))
}

/// The `errors[].code` values of a validation-failure body — the
/// `already_exists` discriminator the I1 race mapping needs (research
/// §4).
#[must_use]
pub fn error_codes(body: &str) -> Vec<String> {
    #[derive(Deserialize)]
    struct ErrorBody {
        #[serde(default)]
        errors: Vec<RestErrorEntry>,
    }
    #[derive(Deserialize)]
    struct RestErrorEntry {
        #[serde(default)]
        code: Option<String>,
    }
    serde_json::from_str::<ErrorBody>(body)
        .map(|parsed| {
            parsed
                .errors
                .into_iter()
                .filter_map(|entry| entry.code)
                .collect()
        })
        .unwrap_or_default()
}

/// The server's human message, truncated for error reasons — surfaced,
/// never synthesized (and never a credential).
#[must_use]
pub fn server_message(body: &str) -> String {
    #[derive(Deserialize)]
    struct MessageBody {
        #[serde(default)]
        message: Option<String>,
    }
    serde_json::from_str::<MessageBody>(body)
        .ok()
        .and_then(|parsed| parsed.message)
        .unwrap_or_else(|| body.chars().take(160).collect())
}

/// Builds the `POST /pulls` JSON body (research §4: `head`, `base`,
/// `title`, `body`).
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

/// Builds the `PATCH /pulls/{number}` rewrite body (title + body only —
/// the §11.1 machine never touches base/head on an update).
#[must_use]
pub fn pr_update_body(spec: &PrSpec) -> String {
    serde_json::json!({
        "title": spec.title,
        "body": spec.body,
    })
    .to_string()
}

/// Builds the `PATCH /pulls/{number}` close body (§11.1 step 3).
#[must_use]
pub fn pr_close_body() -> String {
    serde_json::json!({ "state": "closed" }).to_string()
}

/// Builds the `POST /releases` body (`tag_name`, `name`, `body` — the
/// notes; research §4).
#[must_use]
pub fn release_create_body(spec: &ReleaseSpec) -> String {
    serde_json::json!({
        "tag_name": spec.tag,
        "name": spec.tag,
        "body": spec.notes,
    })
    .to_string()
}

/// Builds the `POST /git/trees` body: `base_tree` plus entries (sha-
/// or content-bearing; the two are mutually exclusive, research §4).
#[must_use]
pub fn tree_create_body(base_tree: &str, entries: &[serde_json::Value]) -> String {
    serde_json::json!({
        "base_tree": base_tree,
        "tree": entries,
    })
    .to_string()
}

/// Builds the `POST /git/blobs` body for a binary payload: base64
/// content with the documented `encoding` (research §4).
#[must_use]
pub fn blob_create_body(content: &[u8]) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(content);
    serde_json::json!({
        "content": encoded,
        "encoding": "base64",
    })
    .to_string()
}

/// Builds the `POST /git/commits` body (message, tree, single parent).
#[must_use]
pub fn commit_create_body(message: &str, tree: &str, parent: &str) -> String {
    serde_json::json!({
        "message": message,
        "tree": tree,
        "parents": [parent],
    })
    .to_string()
}

/// Builds the `PATCH /git/refs/heads/…` body: the new sha and
/// `force: false` — explicit (§11.2: never forced).
#[must_use]
pub fn ref_update_body(sha: &str) -> String {
    serde_json::json!({
        "sha": sha,
        "force": false,
    })
    .to_string()
}

/// Percent-encodes one path segment for tag/branch-bearing URLs —
/// keeps the RFC 3986 unreserved set (`A-Z a-z 0-9 - _ . ~`), encodes
/// everything else (a `/` in a tag name must not split the path).
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

#[cfg(test)]
mod tests {
    use super::{encode_path_segment, error_codes, server_message};

    #[test]
    fn segments_keep_the_unreserved_set() {
        assert_eq!(
            encode_path_segment("core-v0.5.0"),
            "core-v0.5.0",
            "tags need no escaping in the common shape"
        );
        assert_eq!(
            encode_path_segment("feature/v1.0"),
            "feature%2Fv1.0",
            "a slash inside a tag must not split the path"
        );
        assert_eq!(encode_path_segment("a b"), "a%20b");
    }

    #[test]
    fn error_codes_read_the_validation_shape() {
        let codes = error_codes(
            r#"{"message":"Validation Failed","errors":[{"code":"already_exists"},{"field":"x"}]}"#,
        );
        assert_eq!(codes, vec!["already_exists".to_owned()]);
        assert_eq!(error_codes("not json"), Vec::<String>::new());
    }

    #[test]
    fn server_message_surfaces_the_server_text() {
        assert_eq!(
            server_message(r#"{"message":"Bad credentials"}"#),
            "Bad credentials"
        );
        assert_eq!(server_message("plain text"), "plain text");
    }
}
