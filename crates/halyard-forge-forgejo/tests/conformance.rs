//! The forge conformance matrix over the Forgejo REST adapter (M7, §17
//! "same e2e matrix passes per forge"): the shared scenario list runs
//! against [`ForgejoRestForge`] driven through a wire-shaped routing
//! transport — a minimal Forgejo server modeled on the verified shapes
//! of `docs/research/forgejo-rest-forge.md`, stateful across the one
//! scenario it serves (a fresh transport per scenario keeps the matrix
//! scenarios isolated).
//!
//! The gate this suite pins (the plan's own acceptance line for M7):
//! "forge conformance test-suite shared across adapters" whose gate is
//! the "same e2e matrix passes per forge"
//! (`docs/HALYARD_ARCHITECTURE.md` §17, the M7 row). The adapter's one
//! documented decline — `commit_via_api` (the surface has no atomic
//! multi-file commit, research §5) — is the suite's `Declined` verdict,
//! counted green by design (ADR-0049's verdict model).

use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;

use halyard_forge::transport::{HttpRequest, HttpResponse, HttpTransport};
use halyard_forge::{Forge, ForgeError, Scenario, Verdict, run_conformance_matrix};
use halyard_forge_forgejo::{ForgejoEndpoint, ForgejoRestForge};

const REPO: &str = "octo/halyard-demo";
const BASE: &str = "https://codeberg.org/api/v1";
const BRANCH: &str = "halyard/release/main";

/// The modeled forge state one scenario sees.
#[derive(Debug, Default)]
struct State {
    /// `(number, head_ref, title, body)` — the open pull requests the
    /// list route serves (a Forgejo PR **is** an open pull here; the
    /// close route removes it, so the list stays the open set).
    pulls: Vec<(u64, String, String, String)>,
    next_number: u64,
    /// The tags the release route has created.
    releases: Vec<String>,
    /// `(id, body)` — the issue comments the marker scan walks (a PR is
    /// an issue on this API; the index addresses it).
    comments: Vec<(u64, String)>,
    next_comment: u64,
}

/// The routing transport: dispatches on method + path and composes the
/// canonical Forgejo-shaped response from the request body — the offline
/// twin of a loopback lane, per scenario isolated by construction.
struct Router {
    state: Mutex<State>,
}

impl Router {
    fn new() -> Self {
        Self {
            state: Mutex::new(State {
                next_number: 40,
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

    fn pull_json(number: u64, head_ref: &str, title: &str, body: &str) -> String {
        let head = json(head_ref);
        let title = json(title);
        let body = json(body);
        format!(
            r#"{{"number":{number},"title":{title},"body":{body},"state":"open","html_url":"https://codeberg.org/{REPO}/pulls/{number}","head":{{"ref":{head},"label":{head},"repo":{{"full_name":"{REPO}"}}}},"base":{{"ref":"main"}}}}"#
        )
    }

    fn list_json(items: &[String]) -> String {
        format!("[{}]", items.join(","))
    }

    fn comment_json(id: u64, body: &str) -> String {
        format!(
            r#"{{"id":{id},"body":{},"html_url":"https://codeberg.org/{REPO}/issues/comments/{id}"}}"#,
            json(body)
        )
    }

    fn release_json(tag: &str) -> String {
        format!(
            r#"{{"tag_name":{},"html_url":"https://codeberg.org/{REPO}/releases/tag/{tag}","id":7}}"#,
            json(tag)
        )
    }

    /// The suffix after the repo's `pulls` segment (`""` for the list,
    /// `"/42"` for one pull) — the route table's key shape.
    fn pull_suffix(clean: &str) -> Option<String> {
        let rest = clean.strip_prefix(&format!("/repos/{REPO}/pulls"))?;
        Some(rest.to_owned())
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

impl Router {
    /// The pull surface's arm: the list, the create and the one-pull
    /// update/close — the suffix after the repo's `pulls` segment
    /// decides. `Some` when the arm matched.
    fn route_pull(
        state: &mut State,
        method: &str,
        suffix: &str,
        body_json: &str,
    ) -> Option<Result<HttpResponse, ForgeError>> {
        Some(match (method, suffix) {
            ("GET", "") => {
                let items: Vec<String> = state
                    .pulls
                    .iter()
                    .map(|(number, head, title, body)| Self::pull_json(*number, head, title, body))
                    .collect();
                Ok(Self::ok(Self::list_json(&items)))
            }
            ("POST", "") => {
                let head = body_field(body_json, "head").unwrap_or_default();
                let title = body_field(body_json, "title").unwrap_or_default();
                let body = body_field(body_json, "body").unwrap_or_default();
                state.next_number += 1;
                let number = state.next_number;
                state
                    .pulls
                    .push((number, head.clone(), title.clone(), body.clone()));
                Ok(Self::created(Self::pull_json(number, &head, &title, &body)))
            }
            ("PATCH", s) if s.starts_with('/') && trailing_id(s).is_some() => {
                let number = trailing_id(s)?;
                Ok(Self::patch_pull(state, number, body_json))
            }
            _ => return None,
        })
    }

    /// The `PATCH …/pulls/:number` arm: a rewrite carries the
    /// title/body; the verified close (`state: "closed"`, research §8)
    /// removes the pull from the open list (the re-run's find sees none
    /// — I1).
    fn patch_pull(state: &mut State, number: u64, body_json: &str) -> HttpResponse {
        let title = body_field(body_json, "title");
        let body = body_field(body_json, "body");
        if body_field(body_json, "state").as_deref() == Some("closed") {
            state.pulls.retain(|(known, _, _, _)| *known != number);
            return Self::ok(Self::pull_json(
                number,
                BRANCH,
                &title.unwrap_or_default(),
                &body.unwrap_or_default(),
            ));
        }
        for (known, _, known_title, known_body) in &mut state.pulls {
            if *known == number {
                if let Some(new) = &title {
                    known_title.clone_from(new);
                }
                if let Some(new) = &body {
                    known_body.clone_from(new);
                }
            }
        }
        let (number, head, title, body) = state
            .pulls
            .iter()
            .find(|(known, _, _, _)| *known == number)
            .map(|(n, h, t, b)| (*n, h.clone(), t.clone(), b.clone()))
            .unwrap_or_default();
        Self::ok(Self::pull_json(number, &head, &title, &body))
    }
}

impl HttpTransport for Router {
    fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ForgeError> {
        let path = request
            .url
            .strip_prefix(BASE)
            .unwrap_or(&request.url)
            .to_owned();
        let body_json = request.body.clone().unwrap_or_default();
        // The route table keys on the path without its query string (the
        // find probe's `state`/`head` filters ride `?…=` parameters).
        let clean = path.split('?').next().unwrap_or(&path).to_owned();
        let mut state = self.lock();
        if let Some(suffix) = Self::pull_suffix(&clean)
            && let Some(routed) = Self::route_pull(&mut state, request.method, &suffix, &body_json)
        {
            return routed;
        }
        // The release surface: the GET-by-tag probe + the create.
        if clean.starts_with(&format!("/repos/{REPO}/releases/tags/")) && request.method == "GET" {
            let tag = clean.rsplit('/').next().unwrap_or_default();
            return Ok(if state.releases.iter().any(|known| known == tag) {
                Self::ok(Self::release_json(tag))
            } else {
                Self::not_found()
            });
        }
        if request.method == "POST" && clean == format!("/repos/{REPO}/releases") {
            let tag = body_field(&body_json, "tag_name").unwrap_or_default();
            state.releases.push(tag.clone());
            return Ok(Self::created(Self::release_json(&tag)));
        }
        // The comment surface (a PR is an issue here): the scan, the
        // create and the in-place rewrite.
        if clean.starts_with(&format!("/repos/{REPO}/issues/"))
            && clean.ends_with("/comments")
            && request.method == "GET"
        {
            let items: Vec<String> = state
                .comments
                .iter()
                .map(|(id, body)| Self::comment_json(*id, body))
                .collect();
            return Ok(Self::ok(Self::list_json(&items)));
        }
        if request.method == "POST"
            && clean.starts_with(&format!("/repos/{REPO}/issues/"))
            && clean.ends_with("/comments")
        {
            let body = body_field(&body_json, "body").unwrap_or_default();
            state.next_comment += 1;
            let id = state.next_comment;
            state.comments.push((id, body.clone()));
            return Ok(Self::created(Self::comment_json(id, &body)));
        }
        if request.method == "PATCH" && clean.contains("/issues/comments/") {
            let id = trailing_id(&clean).unwrap_or_default();
            let new_body = body_field(&body_json, "body").unwrap_or_default();
            for (known, body) in &mut state.comments {
                if *known == id {
                    body.clone_from(&new_body);
                }
            }
            return Ok(Self::ok(Self::comment_json(id, &new_body)));
        }
        Err(ForgeError::transport(format!(
            "the routing transport has no route for {} {path}",
            request.method
        )))
    }
}

/// The Forgejo factory: a fresh adapter over a fresh routing transport
/// per scenario — the scripted-conversation construction the contract
/// tests use.
fn forgejo_factory(scenario: Scenario) -> Box<dyn Forge> {
    let _ = scenario;
    let forge = ForgejoRestForge::with_transport(
        ForgejoEndpoint::with_base(REPO, BASE)
            .unwrap_or_else(|error| unreachable!("static endpoint is valid: {error}")),
        "conformance-token",
        Router::new(),
    );
    Box::new(forge)
}

#[test]
fn matrix_is_green_over_forgejo_rest() {
    let report = run_conformance_matrix(&mut forgejo_factory);
    assert!(
        report.is_green(),
        "the Forgejo REST adapter fails the shared matrix: {report:?}"
    );
}

#[test]
fn forgejo_declines_exactly_the_commit_capability() {
    // The one documented decline (research §5: the surface has no atomic
    // multi-file commit, so `commit_via_api` rides the trait's honest
    // decline naming the adapter) — every other capability is
    // implemented, so the matrix shows eight passes and one decline.
    let report = run_conformance_matrix(&mut forgejo_factory);
    let declined: Vec<&str> = report
        .outcomes()
        .iter()
        .filter_map(|outcome| match &outcome.verdict {
            Verdict::Declined { .. } => Some(outcome.scenario.name()),
            _ => None,
        })
        .collect();
    assert_eq!(
        declined,
        vec!["commit_via_api_capability"],
        "exactly the commit capability declines: {report:?}"
    );
    let failed = report.failures();
    assert!(
        failed.is_empty(),
        "no scenario fails on a green adapter: {failed:?}"
    );
}
