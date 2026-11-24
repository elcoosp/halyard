//! The pure wire layer of the GitLab REST adapter (M7 step 1):
//! endpoint construction, request builders and response parsers —
//! plain functions over the research-verified shapes
//! (`docs/research/gitlab-rest-forge.md`). No I/O, no clock, no
//! environment: the [`crate::forge::GitLabRestForge`] assembles the
//! exchanges, this module owns the grammar. Unparseable responses map
//! to [`ForgeError::Transport`] (the exchange produced nothing usable),
//! never to a guessed success.

use std::fmt::Write as _;

use base64::Engine as _;
use serde::Deserialize;

use halyard_forge::error::ForgeError;
use halyard_forge::spec::{ApiFile, PrSpec, ReleaseSpec};

/// The §11.2 release-branch prefix (`halyard/release/<base>`); the MR
/// `target_branch` is the documented inverse — the prefix stripped
/// (the ADR-0013 decision 6 mapping, carried to the GitLab adapter by
/// ADR-0049), so `upsert_pr` spends no extra probe on it.
pub const RELEASE_BRANCH_PREFIX: &str = "halyard/release/";

/// The GitLab REST endpoint coordinates: the full project path
/// (`group/repo`, nested groups included) plus the API base
/// (`https://gitlab.com/api/v4` by default; self-hosted instances and
/// the offline lane override it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitLabEndpoint {
    project_path: String,
    api_base: String,
}

impl GitLabEndpoint {
    /// Validates and builds the endpoint against the GitLab.com API
    /// base (`E-FORGE-REQUEST` on an unusable project path).
    ///
    /// # Errors
    ///
    /// [`ForgeError::Request`] when the project path is empty,
    /// whitespace-bearing, or carries an empty segment (a project path
    /// is one to several non-empty `/`-separated segments; the request
    /// never leaves the process).
    pub fn new(project_path: &str) -> Result<Self, ForgeError> {
        Self::with_base(project_path, "https://gitlab.com/api/v4")
    }

    /// Validates and builds the endpoint against an explicit API base
    /// (`GITLAB_API_URL` overrides; the base already carries the
    /// `/api/v4` root, which is never appended implicitly).
    ///
    /// # Errors
    ///
    /// [`ForgeError::Request`] as [`GitLabEndpoint::new`], plus a base
    /// that is empty, not http(s) or not a bare authority.
    pub fn with_base(project_path: &str, api_base: &str) -> Result<Self, ForgeError> {
        let base = api_base.trim_end_matches('/');
        if project_path.is_empty()
            || project_path.bytes().any(|b| b.is_ascii_whitespace())
            || project_path.split('/').any(str::is_empty)
        {
            return Err(ForgeError::request(format!(
                "invalid GitLab project path {project_path:?}: it must be one to several \
                 non-empty `/`-separated path segments without whitespace"
            )));
        }
        if !(base.starts_with("https://") || base.starts_with("http://")) {
            return Err(ForgeError::request(format!(
                "invalid GitLab API base {base:?}: it must be an http(s) URL (the `/api/v4` \
                 root included, e.g. https://gitlab.com/api/v4)"
            )));
        }
        Ok(Self {
            project_path: project_path.to_owned(),
            api_base: base.to_owned(),
        })
    }

    /// The full project path.
    #[must_use]
    pub fn project_path(&self) -> &str {
        &self.project_path
    }

    /// The API base the endpoint resolves against.
    #[must_use]
    pub fn api_base(&self) -> &str {
        &self.api_base
    }

    /// The URL-encoded project path — the `:id` path segment the API
    /// accepts as "The ID or URL-encoded path of the project"
    /// (research §2); every `/` becomes `%2F`.
    #[must_use]
    pub fn project_id(&self) -> String {
        self.project_path
            .split('/')
            .map(encode_path_segment)
            .collect::<Vec<_>>()
            .join("%2F")
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

    /// The `/projects/{encoded-id}` prefix every project-scoped path
    /// starts with.
    #[must_use]
    pub fn repo_prefix(&self) -> String {
        format!("/projects/{}", self.project_id())
    }

    /// Parses a `gitlab.com` remote URL (https / `git@host:path` /
    /// `ssh://` forms) into an endpoint against the default API base —
    /// the shell's origin-detection half of the wire-up. Nested group
    /// paths are preserved verbatim (the project path is the URL's
    /// path). A foreign host or an unusable path is `None` — the caller
    /// reports the decline, never a guess.
    #[must_use]
    pub fn from_remote(remote_url: &str) -> Option<Self> {
        const HOST: &str = "gitlab.com";
        let path = if let Some(rest) = remote_url
            .strip_prefix("git@")
            .and_then(|rest| rest.strip_prefix(HOST).and_then(|p| p.strip_prefix(':')))
        {
            rest
        } else if remote_url.starts_with("https://")
            && let Some(rest) = remote_url
                .strip_prefix("https://")
                .and_then(|p| p.strip_prefix(HOST))
        {
            rest.strip_prefix('/').unwrap_or(rest)
        } else if remote_url.starts_with("ssh://")
            && let Some(rest) = remote_url.strip_prefix("ssh://")
            && let Some(path) = rest
                .strip_prefix(&format!("git@{HOST}"))
                .and_then(|p| p.strip_prefix(':').or_else(|| p.strip_prefix('/')))
        {
            path
        } else {
            return None;
        };
        let path = path.strip_suffix('/').unwrap_or(path);
        let path = path.strip_suffix(".git").unwrap_or(path);
        if path.is_empty() || path.contains(' ') || path.split('/').any(str::is_empty) {
            return None;
        }
        Self::new(path).ok()
    }
}

/// A parsed MR body (the fields the seam's
/// [`Pr`](halyard_forge::spec::Pr) carries; `title`/`description`/
/// `web_url` are nullable on the wire).
#[derive(Debug, Deserialize)]
pub struct RestMr {
    /// The project-scoped MR number (`merge_request_iid`).
    pub iid: u64,
    /// Current title (nullable on the wire).
    pub title: Option<String>,
    /// Current description (nullable on the wire).
    pub description: Option<String>,
    /// Web URL (nullable on the wire).
    pub web_url: Option<String>,
    /// The MR's source branch (the find filter's confirmation).
    pub source_branch: Option<String>,
}

/// A parsed release body (`tag_name` + `_links.self`; research §5 —
/// GitLab's release JSON has no `html_url`/`web_url` field).
#[derive(Debug, Deserialize)]
pub struct RestRelease {
    /// The released tag.
    pub tag_name: Option<String>,
    /// The `_links` object (`self` is the release's HTTP URL).
    #[serde(rename = "_links")]
    pub links: Option<RestLinks>,
}

/// The release JSON's `_links` object.
#[derive(Debug, Deserialize)]
pub struct RestLinks {
    /// "HTTP URL of the release." (research §5).
    #[serde(rename = "self")]
    pub self_url: Option<String>,
}

/// A parsed note body (the fields the marker scan needs; the docs'
/// example shape carries no `web_url`, so it is optional — research §4).
#[derive(Debug, Deserialize)]
pub struct RestNote {
    /// The note id (the rewrite address).
    pub id: u64,
    /// The note body (the marker scan's find key).
    pub body: Option<String>,
    /// Web URL when the deployment sends it (never fabricated).
    pub web_url: Option<String>,
}

/// A parsed created-commit body (the commits API's 201 response; the
/// `id` is the created sha, research §6).
#[derive(Debug, Deserialize)]
pub struct RestCommit {
    /// The created commit's sha.
    pub id: Option<String>,
}

/// Parses a PR list (the MR find probe's body) — an unparseable body is
/// a transport failure, never a guessed empty list.
///
/// # Errors
///
/// [`ForgeError::Transport`] when the body is not a JSON array of MRs.
pub fn parse_mr_list(body: &str) -> Result<Vec<RestMr>, ForgeError> {
    serde_json::from_str(body)
        .map_err(|error| ForgeError::transport(format!("the MR list did not parse: {error}")))
}

/// Parses one MR body.
///
/// # Errors
///
/// [`ForgeError::Transport`] when the body is not an MR object.
pub fn parse_mr(body: &str) -> Result<RestMr, ForgeError> {
    serde_json::from_str(body)
        .map_err(|error| ForgeError::transport(format!("the MR body did not parse: {error}")))
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

/// Parses a notes list.
///
/// # Errors
///
/// [`ForgeError::Transport`] when the body is not a JSON array of notes.
pub fn parse_note_list(body: &str) -> Result<Vec<RestNote>, ForgeError> {
    serde_json::from_str(body)
        .map_err(|error| ForgeError::transport(format!("the notes list did not parse: {error}")))
}

/// Parses one note body (the rewrite/create responses are single
/// objects, unlike the scan's list).
///
/// # Errors
///
/// [`ForgeError::Transport`] when the body is not a note object.
pub fn parse_note(body: &str) -> Result<RestNote, ForgeError> {
    serde_json::from_str(body)
        .map_err(|error| ForgeError::transport(format!("the note body did not parse: {error}")))
}

/// Parses a created-commit body.
///
/// # Errors
///
/// [`ForgeError::Transport`] when the body is not a commit object.
pub fn parse_commit(body: &str) -> Result<RestCommit, ForgeError> {
    serde_json::from_str(body)
        .map_err(|error| ForgeError::transport(format!("the commit body did not parse: {error}")))
}

/// The server's own diagnosis: GitLab shapes errors as
/// `{"message": "…"}` or `{"message": {"field": ["…"]}}` (a 400's field
/// map) or `{"error": "…"}` — rendered compactly for the taxonomy's
/// reasons (§15.4 errors teach).
#[must_use]
pub fn server_message(body: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return body.chars().take(200).collect();
    };
    if let Some(message) = value.get("message") {
        return compact(message);
    }
    if let Some(error) = value.get("error") {
        return compact(error);
    }
    body.chars().take(200).collect()
}

/// Renders one JSON value compactly (strings verbatim, objects/arrays
/// as compact JSON, everything else via its display).
fn compact(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
            serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
        }
        other => other.to_string(),
    }
}

/// Percent-encodes one path segment (the RFC 3986 unreserved set rides
/// verbatim; everything else — `/` included, which is what the project
/// path's encoding needs — is `%XX`-escaped).
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

/// The MR-create body (`POST /projects/:id/merge_requests`; research
/// §3: `source_branch`, `target_branch`, `title`, `description`).
#[must_use]
pub fn mr_create_body(spec: &PrSpec, base: &str) -> String {
    serde_json::json!({
        "source_branch": spec.branch.as_str(),
        "target_branch": base,
        "title": spec.title,
        "description": spec.body,
    })
    .to_string()
}

/// The MR-update body (`PUT /projects/:id/merge_requests/:iid`; the
/// "at least one non-required attribute" rule — the adapter always sends
/// both `title` and `description`).
#[must_use]
pub fn mr_update_body(spec: &PrSpec) -> String {
    serde_json::json!({
        "title": spec.title,
        "description": spec.body,
    })
    .to_string()
}

/// The MR-close body (`state_event: "close"`, research §3).
#[must_use]
pub fn mr_close_body() -> String {
    serde_json::json!({ "state_event": "close" }).to_string()
}

/// The release-create body (`POST /projects/:id/releases`; the tag
/// exists — the §11.1 tag phase creates it git-side — so `ref` is
/// omitted, research §5).
#[must_use]
pub fn release_create_body(spec: &ReleaseSpec) -> String {
    serde_json::json!({
        "tag_name": spec.tag,
        "description": spec.notes,
    })
    .to_string()
}

/// The note body (`body`, research §4).
#[must_use]
pub fn note_body(body: &str) -> String {
    serde_json::json!({ "body": body }).to_string()
}

/// One commits-API action (`POST /projects/:id/repository/commits`;
/// research §6: `action` create-or-update, `file_path`, `content`,
/// `encoding: "base64"` — byte-exact round-trips).
#[must_use]
pub fn commit_action(file: &ApiFile, exists: bool) -> String {
    let content = base64::engine::general_purpose::STANDARD.encode(&file.content);
    serde_json::json!({
        "action": if exists { "update" } else { "create" },
        "file_path": file.path,
        "content": content,
        "encoding": "base64",
    })
    .to_string()
}

/// The commit-create body (`branch`, `commit_message`, `actions[]`).
#[must_use]
pub fn commit_create_body(branch: &str, message: &str, actions: &[String]) -> String {
    let parsed: Vec<serde_json::Value> = actions
        .iter()
        .map(|action| {
            serde_json::from_str(action)
                .unwrap_or_else(|error| unreachable!("a built action is valid JSON: {error}"))
        })
        .collect();
    serde_json::json!({
        "branch": branch,
        "commit_message": message,
        "actions": parsed,
    })
    .to_string()
}

/// Extracts the created sha from a `{ "id": "…" }` / `{ "sha": "…" }`
/// body (the commits API and the release route's shared read).
///
/// # Errors
///
/// [`ForgeError::Transport`] when the sha is missing or not 40-hex.
pub fn parse_created_sha(body: &str) -> Result<String, ForgeError> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|error| ForgeError::transport(format!("the sha body did not parse: {error}")))?;
    let sha = value
        .get("id")
        .or_else(|| value.get("sha"))
        .and_then(|sha| sha.as_str())
        .filter(|sha| sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| {
            ForgeError::transport(format!(
                "the exchange returned no usable object sha: {body:.200}"
            ))
        })?;
    Ok(sha.to_owned())
}
