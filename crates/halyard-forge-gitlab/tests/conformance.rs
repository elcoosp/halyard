//! The forge conformance matrix over the GitLab REST adapter (M7, §17
//! "same e2e matrix passes per forge"): the shared scenario list runs
//! against [`GitLabRestForge`] driven through a wire-shaped routing
//! transport — a minimal GitLab server modeled on the verified shapes of
//! `docs/research/gitlab-rest-forge.md`, stateful across the one
//! scenario it serves (a fresh transport per scenario keeps the matrix
//! scenarios isolated).

use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;

use halyard_forge::transport::{HttpRequest, HttpResponse, HttpTransport};
use halyard_forge::{Forge, ForgeError, Scenario, Verdict, run_conformance_matrix};
use halyard_forge_gitlab::{GitLabEndpoint, GitLabRestForge};

const PROJECT: &str = "octo/halyard-demo";
const PROJECT_ID: &str = "octo%2Fhalyard-demo";
const BASE: &str = "https://gitlab.com/api/v4";
const BRANCH: &str = "halyard/release/main";

/// The modeled forge state one scenario sees.
#[derive(Debug, Default)]
struct State {
    /// `(iid, source_branch, title, description)` — the open MRs the
    /// list route serves.
    mrs: Vec<(u64, String, String, String)>,
    next_iid: u64,
    /// The tags the release route has created.
    releases: Vec<String>,
    /// `(id, body)` — the MR notes the marker scan walks.
    notes: Vec<(u64, String)>,
    next_note: u64,
    /// The commit counter (deterministic 40-hex-digit shas).
    commits: u64,
}

/// The routing transport: dispatches on method + path and composes the
/// canonical GitLab-shaped response from the request body — the offline
/// twin of a loopback lane, per scenario isolated by construction.
struct Router {
    state: Mutex<State>,
}

impl Router {
    fn new() -> Self {
        Self {
            state: Mutex::new(State {
                next_iid: 41,
                next_note: 900,
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
            body: r#"{"message":"404 Not Found"}"#.to_owned(),
        }
    }

    fn mr_json(iid: u64, source: &str, title: &str, description: &str) -> String {
        format!(
            r#"{{"iid":{iid},"title":{title},"description":{description},"state":"opened","source_branch":{source},"target_branch":"main","web_url":"https://gitlab.com/{PROJECT}/-/merge_requests/{iid}"}}"#,
            title = json(title),
            description = json(description),
            source = json(source),
        )
    }

    fn list_json(items: &[String]) -> String {
        format!("[{}]", items.join(","))
    }

    fn note_json(id: u64, body: &str) -> String {
        format!(r#"{{"id":{id},"body":{},"system":false}}"#, json(body))
    }

    fn release_json(tag: &str) -> String {
        format!(
            r#"{{"tag_name":{},"_links":{{"self":"https://gitlab.com/{PROJECT}/-/releases/{tag}"}}}}"#,
            json(tag)
        )
    }

    /// The suffix after the project's `merge_requests` segment (`""` for
    /// the list, `"/42"` for one MR, `"/42/notes"` for its notes,
    /// `"/42/notes/9"` for one note) — the route table's key shape.
    fn mr_suffix(clean: &str) -> Option<String> {
        let rest = clean.strip_prefix(&format!("/projects/{PROJECT_ID}/merge_requests"))?;
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
    /// The MR surface's arm: list, one MR (update/close), its notes
    /// (list, create) and one note (update) — the suffix after the
    /// project's `merge_requests` segment decides. `Some` when the arm
    /// matched.
    fn route_mr(
        state: &mut State,
        method: &str,
        suffix: &str,
        body_json: &str,
    ) -> Option<Result<HttpResponse, ForgeError>> {
        Some(match (method, suffix) {
            ("GET", "") => {
                let items: Vec<String> = state
                    .mrs
                    .iter()
                    .map(|(iid, source, title, description)| {
                        Self::mr_json(*iid, source, title, description)
                    })
                    .collect();
                Ok(Self::ok(Self::list_json(&items)))
            }
            ("GET", s) if s.starts_with('/') && s.ends_with("/notes") => {
                let items: Vec<String> = state
                    .notes
                    .iter()
                    .map(|(id, body)| Self::note_json(*id, body))
                    .collect();
                Ok(Self::ok(Self::list_json(&items)))
            }
            ("POST", "") => {
                let source = body_field(body_json, "source_branch").unwrap_or_default();
                let title = body_field(body_json, "title").unwrap_or_default();
                let description = body_field(body_json, "description").unwrap_or_default();
                state.next_iid += 1;
                let iid = state.next_iid;
                state
                    .mrs
                    .push((iid, source.clone(), title.clone(), description.clone()));
                Ok(Self::created(Self::mr_json(
                    iid,
                    &source,
                    &title,
                    &description,
                )))
            }
            ("POST", s) if s.starts_with('/') && s.ends_with("/notes") => {
                let body = body_field(body_json, "body").unwrap_or_default();
                state.next_note += 1;
                let id = state.next_note;
                state.notes.push((id, body.clone()));
                Ok(Self::created(Self::note_json(id, &body)))
            }
            ("PUT", s)
                if s.starts_with('/') && s.split('/').count() == 2 && trailing_id(s).is_some() =>
            {
                let iid = trailing_id(s)?;
                Ok(Self::put_mr(state, iid, body_json))
            }
            ("PUT", s) if s.contains("/notes/") => {
                let id = trailing_id(s)?;
                let new_body = body_field(body_json, "body").unwrap_or_default();
                for (known, body) in &mut state.notes {
                    if *known == id {
                        body.clone_from(&new_body);
                    }
                }
                Ok(Self::ok(Self::note_json(id, &new_body)))
            }
            _ => return None,
        })
    }

    /// The `PUT …/merge_requests/:iid` arm: a rewrite carries the
    /// title/description; the documented close (`state_event`) removes
    /// the MR from the open list (the re-run's find sees none — I1).
    fn put_mr(state: &mut State, iid: u64, body_json: &str) -> HttpResponse {
        let title = body_field(body_json, "title");
        let description = body_field(body_json, "description");
        if body_field(body_json, "state_event").as_deref() == Some("close") {
            state.mrs.retain(|(known, _, _, _)| *known != iid);
            return Self::ok(Self::mr_json(
                iid,
                BRANCH,
                &title.unwrap_or_default(),
                &description.unwrap_or_default(),
            ));
        }
        for (known, _, known_title, known_description) in &mut state.mrs {
            if *known == iid {
                if let Some(new) = &title {
                    known_title.clone_from(new);
                }
                if let Some(new) = &description {
                    known_description.clone_from(new);
                }
            }
        }
        let (iid, source, title, description) = state
            .mrs
            .iter()
            .find(|(known, _, _, _)| *known == iid)
            .map(|(n, s, t, d)| (*n, s.clone(), t.clone(), d.clone()))
            .unwrap_or_default();
        Self::ok(Self::mr_json(iid, &source, &title, &description))
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
        // The route table keys on the path without its query string
        // (the MR filter, the note sort and the file probe all ride
        // `?…=` parameters).
        let clean = path.split('?').next().unwrap_or(&path).to_owned();
        let mut state = self.lock();
        if let Some(suffix) = Self::mr_suffix(&clean)
            && let Some(routed) = Self::route_mr(&mut state, request.method, &suffix, &body_json)
        {
            return routed;
        }
        // The release surface: GET-by-tag probe + create.
        if clean.starts_with(&format!("/projects/{PROJECT_ID}/releases/"))
            && request.method == "GET"
        {
            let tag = clean.rsplit('/').next().unwrap_or_default();
            return Ok(if state.releases.iter().any(|known| known == tag) {
                Self::ok(Self::release_json(tag))
            } else {
                Self::not_found()
            });
        }
        if request.method == "POST" && clean == format!("/projects/{PROJECT_ID}/releases") {
            let tag = body_field(&body_json, "tag_name").unwrap_or_default();
            state.releases.push(tag.clone());
            return Ok(Self::created(Self::release_json(&tag)));
        }
        // The commit surface: the per-file existence probe (the router
        // models a branch where the suite's file does not exist yet —
        // the content commit creates it) and the commits create.
        if request.method == "GET" && clean.contains("/repository/files/") {
            return Ok(Self::not_found());
        }
        if request.method == "POST" && clean.contains("/repository/commits") {
            return Ok(Self::commit_route(&mut state, &body_json));
        }
        Err(ForgeError::transport(format!(
            "the routing transport has no route for {} {path}",
            request.method
        )))
    }
}

impl Router {
    /// The commits-create arm: the response carries the created sha
    /// (research §6) plus the fields the JSON shape pins.
    fn commit_route(state: &mut State, body_json: &str) -> HttpResponse {
        let value: serde_json::Value = serde_json::from_str(body_json)
            .unwrap_or_else(|error| unreachable!("the adapter sends valid JSON: {error}"));
        let message = value
            .get("commit_message")
            .and_then(|m| m.as_str())
            .unwrap_or_default()
            .to_owned();
        state.commits += 1;
        Self::created(format!(
            r#"{{"id":"{:040}","short_id":"a{:06}","title":{},"web_url":"https://gitlab.com/{PROJECT}/-/commit/{:040}"}}"#,
            state.commits,
            state.commits,
            json(&message),
            state.commits
        ))
    }
}

/// The GitLab factory: a fresh adapter over a fresh routing transport
/// per scenario — the scripted-conversation construction the contract
/// tests use.
fn gitlab_factory(scenario: Scenario) -> Box<dyn Forge> {
    let _ = scenario;
    let forge = GitLabRestForge::with_transport(
        GitLabEndpoint::with_base(PROJECT, BASE)
            .unwrap_or_else(|error| unreachable!("static endpoint is valid: {error}")),
        "conformance-token",
        Router::new(),
    );
    Box::new(forge)
}

#[test]
fn matrix_is_green_over_gitlab_rest() {
    let report = run_conformance_matrix(&mut gitlab_factory);
    assert!(
        report.is_green(),
        "the GitLab REST adapter fails the shared matrix: {report:?}"
    );
}

#[test]
fn gitlab_declines_nothing() {
    // The GitLab adapter implements every optional capability (the MR
    // close, the commits-API commit, the notes comment) — the matrix
    // shows nine passes, zero declines.
    let report = run_conformance_matrix(&mut gitlab_factory);
    let declined = report
        .outcomes()
        .iter()
        .filter(|outcome| matches!(outcome.verdict, Verdict::Declined { .. }))
        .count();
    assert_eq!(declined, 0, "all capabilities are implemented: {report:?}");
}
