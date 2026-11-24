//! The forge conformance matrix over the GitHub REST adapter (M7, §17
//! "same e2e matrix passes per forge"): the same scenario list the
//! reference adapters run in `halyard-forge` runs here against
//! [`GitHubRestForge`] driven through a wire-shaped routing transport —
//! a minimal GitHub server modeled on the verified shapes of
//! `docs/research/github-rest-forge.md`, stateful across the one
//! scenario it serves (a fresh transport per scenario keeps the matrix
//! scenarios isolated).

use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;

use halyard_forge::transport::{HttpRequest, HttpResponse, HttpTransport};
use halyard_forge::{Forge, ForgeError, Scenario, Verdict, run_conformance_matrix};
use halyard_forge_github::{ForgeEndpoint, GitHubRestForge};

const OWNER: &str = "octo";
const REPO: &str = "halyard-demo";
const BASE: &str = "https://api.github.com";

/// The modeled forge state one scenario sees.
#[derive(Debug, Default)]
struct State {
    /// `(number, head, title, body)` — the open PRs the list route serves.
    prs: Vec<(u64, String, String, String)>,
    next_pr: u64,
    /// The tags the release route has created.
    releases: Vec<String>,
    /// `(id, body)` — the issue comments the marker scan walks.
    comments: Vec<(u64, String)>,
    next_comment: u64,
    /// The commit counter (deterministic 40-hex-digit shas).
    commits: u64,
}

/// The routing transport: dispatches on method + path and composes the
/// canonical GitHub-shaped response from the request body — the offline
/// twin of the loopback lane, per scenario isolated by construction.
struct Router {
    state: Mutex<State>,
}

impl Router {
    fn new() -> Self {
        Self {
            state: Mutex::new(State {
                next_pr: 41,
                next_comment: 900,
                ..State::default()
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn ok(body: String) -> HttpResponse {
        HttpResponse {
            status: 200,
            headers: Vec::new(),
            body,
        }
    }

    fn created(body: String) -> HttpResponse {
        HttpResponse {
            status: 201,
            headers: Vec::new(),
            body,
        }
    }

    fn not_found() -> HttpResponse {
        HttpResponse {
            status: 404,
            headers: Vec::new(),
            body: r#"{"message":"Not Found"}"#.to_owned(),
        }
    }

    fn pr_json(number: u64, head: &str, title: &str, body: &str) -> String {
        format!(
            r#"{{"number":{number},"title":{title},"body":{body},"state":"open","head":{{"ref":"{head}"}},"html_url":"https://github.com/{OWNER}/{REPO}/pull/{number}"}}"#,
            title = json(title),
            body = json(body),
        )
    }

    fn list_json(items: &[String]) -> String {
        format!("[{}]", items.join(","))
    }

    fn comment_json(id: u64, body: &str) -> String {
        format!(
            r#"{{"id":{id},"body":{},"html_url":"https://github.com/{OWNER}/{REPO}/issues/comments/{id}"}}"#,
            json(body)
        )
    }

    fn release_json(tag: &str) -> String {
        format!(
            r#"{{"id":7,"tag_name":{},"html_url":"https://github.com/{OWNER}/{REPO}/releases/tag/{tag}"}}"#,
            json(tag)
        )
    }
}

/// A JSON string literal (the router's one text helper).
fn json(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
}

/// The body's string field (the adapter always sends valid JSON).
fn body_field(body: &str, key: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value.get(key).and_then(|v| v.as_str().map(str::to_owned)))
}

/// The path's trailing numeric id.
fn trailing_id(path: &str) -> Option<u64> {
    path.rsplit('/').next().and_then(|id| id.parse().ok())
}

impl HttpTransport for Router {
    fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ForgeError> {
        let path = request
            .url
            .strip_prefix(BASE)
            .unwrap_or(&request.url)
            .to_owned();
        let body_json = request.body.clone().unwrap_or_default();
        // The route table keys on the path without its query string
        // (the comment scan rides `?per_page=100&page=N`).
        let clean = path.split('?').next().unwrap_or(&path).to_owned();
        let mut state = self.lock();
        let repo = format!("/repos/{OWNER}/{REPO}");
        if request.method == "GET" {
            return Self::route_read(&mut state, &clean, &repo);
        }
        Self::route_write(&mut state, request.method, &clean, &repo, &body_json)
    }
}

impl Router {
    /// The read routes: the PR list, the release-by-tag probe, the
    /// comment list (the marker scan's walk), the branch ref and the
    /// commit object (the API-commit chain's base reads).
    fn route_read(state: &mut State, clean: &str, repo: &str) -> Result<HttpResponse, ForgeError> {
        if clean.starts_with(&format!("{repo}/pulls")) {
            let items: Vec<String> = state
                .prs
                .iter()
                .map(|(number, head, title, body)| Self::pr_json(*number, head, title, body))
                .collect();
            return Ok(Self::ok(Self::list_json(&items)));
        }
        if clean.starts_with(&format!("{repo}/releases/tags/")) {
            let tag = clean.rsplit('/').next().unwrap_or_default();
            return Ok(if state.releases.iter().any(|known| known == tag) {
                Self::ok(Self::release_json(tag))
            } else {
                Self::not_found()
            });
        }
        if clean.starts_with(&format!("{repo}/issues/")) && clean.ends_with("/comments") {
            let items: Vec<String> = state
                .comments
                .iter()
                .map(|(id, body)| Self::comment_json(*id, body))
                .collect();
            return Ok(Self::ok(Self::list_json(&items)));
        }
        if clean.starts_with(&format!("{repo}/git/ref/heads/")) {
            let sha = format!("{:040}", state.commits);
            return Ok(Self::ok(format!(
                r#"{{"ref":"refs/heads/x","object":{{"sha":"{sha}","type":"commit"}}}}"#
            )));
        }
        if clean.contains("/git/commits/") {
            let sha = format!("{:040}", state.commits);
            return Ok(Self::ok(format!(
                r#"{{"sha":"{sha}","tree":{{"sha":"{sha}"}}}}"#
            )));
        }
        Err(ForgeError::transport(format!(
            "the routing transport has no read route for {clean}"
        )))
    }

    /// The write routes: the PR create/rewrite, the release create, the
    /// comment create/rewrite and the API-commit chain (blob → tree →
    /// commit → ref, UTF-8 payloads riding the tree entry).
    fn route_write(
        state: &mut State,
        method: &str,
        clean: &str,
        repo: &str,
        body_json: &str,
    ) -> Result<HttpResponse, ForgeError> {
        if method == "POST" && clean == format!("{repo}/pulls") {
            let head = body_field(body_json, "head").unwrap_or_default();
            let title = body_field(body_json, "title").unwrap_or_default();
            let body = body_field(body_json, "body").unwrap_or_default();
            state.next_pr += 1;
            let number = state.next_pr;
            state
                .prs
                .push((number, head.clone(), title.clone(), body.clone()));
            return Ok(Self::created(Self::pr_json(number, &head, &title, &body)));
        }
        if method == "PATCH" && clean.starts_with(&format!("{repo}/pulls/")) {
            let Some(number) = trailing_id(clean) else {
                return Err(ForgeError::transport("unroutable PATCH (no PR number)"));
            };
            for (known, _, title, body) in &mut state.prs {
                if *known == number {
                    if let Some(new) = body_field(body_json, "title") {
                        title.clone_from(&new);
                    }
                    if let Some(new) = body_field(body_json, "body") {
                        body.clone_from(&new);
                    }
                }
            }
            let (_, head, title, body) = state
                .prs
                .iter()
                .find(|(known, _, _, _)| *known == number)
                .map(|(n, h, t, b)| (*n, h.clone(), t.clone(), b.clone()))
                .unwrap_or_default();
            return Ok(Self::ok(Self::pr_json(number, &head, &title, &body)));
        }
        if method == "POST" && clean == format!("{repo}/releases") {
            let tag = body_field(body_json, "tag_name").unwrap_or_default();
            state.releases.push(tag.clone());
            return Ok(Self::created(Self::release_json(&tag)));
        }
        if method == "POST"
            && clean.starts_with(&format!("{repo}/issues/"))
            && clean.ends_with("/comments")
        {
            let body = body_field(body_json, "body").unwrap_or_default();
            state.next_comment += 1;
            let id = state.next_comment;
            state.comments.push((id, body.clone()));
            return Ok(Self::created(Self::comment_json(id, &body)));
        }
        if method == "PATCH"
            && clean.starts_with(&format!("{repo}/issues/"))
            && clean.contains("/comments/")
        {
            let Some(id) = trailing_id(clean) else {
                return Err(ForgeError::transport("unroutable PATCH (no comment id)"));
            };
            let new_body = body_field(body_json, "body").unwrap_or_default();
            for (known, body) in &mut state.comments {
                if *known == id {
                    body.clone_from(&new_body);
                }
            }
            return Ok(Self::ok(Self::comment_json(id, &new_body)));
        }
        if method == "POST"
            && (clean == format!("{repo}/git/blobs")
                || clean == format!("{repo}/git/trees")
                || clean == format!("{repo}/git/commits"))
        {
            state.commits += 1;
            return Ok(Self::created(format!(
                r#"{{"sha":"{:040}"}}"#,
                state.commits
            )));
        }
        if method == "PATCH" && clean.starts_with(&format!("{repo}/git/refs/")) {
            let sha = body_field(body_json, "sha").unwrap_or_default();
            return Ok(Self::ok(format!(
                r#"{{"ref":"refs/heads/x","object":{{"sha":"{sha}","type":"commit"}}}}"#
            )));
        }
        Err(ForgeError::transport(format!(
            "the routing transport has no write route for {method} {clean}"
        )))
    }
}

/// The GitHub factory: a fresh adapter over a fresh routing transport
/// per scenario — the same construction the loopback lane uses, offline.
fn github_factory(scenario: Scenario) -> Box<dyn Forge> {
    let _ = scenario;
    let forge = GitHubRestForge::with_transport(
        ForgeEndpoint::with_base(OWNER, REPO, BASE)
            .unwrap_or_else(|error| unreachable!("static endpoint is valid: {error}")),
        "conformance-token",
        Router::new(),
    );
    Box::new(forge)
}

#[test]
fn matrix_is_green_over_github_rest() {
    let report = run_conformance_matrix(&mut github_factory);
    assert!(
        report.is_green(),
        "the GitHub REST adapter fails the shared matrix: {report:?}"
    );
}

#[test]
fn github_declines_nothing() {
    // The GitHub adapter implements every optional capability — the
    // matrix shows nine passes, zero declines.
    let report = run_conformance_matrix(&mut github_factory);
    let declined = report
        .outcomes()
        .iter()
        .filter(|outcome| matches!(outcome.verdict, Verdict::Declined { .. }))
        .count();
    assert_eq!(declined, 0, "all capabilities are implemented: {report:?}");
}
