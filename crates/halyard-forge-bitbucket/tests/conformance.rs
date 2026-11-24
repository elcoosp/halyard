//! The shared forge conformance matrix over the Bitbucket REST adapter
//! (M7 step 3, ADR-0051): the §17 gate — "same e2e matrix passes per
//! forge" — run over a routing transport that models the Bitbucket
//! surface (research-verified shapes: the pullrequests flow, the
//! tag-as-release-marker probe, the PR comments' pagination). The one
//! documented decline is `commit_via_api` (the commits resource "does
//! NOT support new commit creation", research §5).

use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;

use halyard_forge::conformance::{Scenario, Verdict, run_conformance_matrix};
use halyard_forge::transport::{HttpRequest, HttpResponse, HttpTransport};
use halyard_forge::{Forge, ForgeError};
use halyard_forge_bitbucket::{BitbucketEndpoint, BitbucketRestForge};

const REPO: &str = "octo/halyard-demo";
const BASE: &str = "https://api.bitbucket.org/2.0";
const BRANCH: &str = "halyard/release/main";
/// The §11.2 base the upsert's `destination` carries (the inverse
/// mapping's stripping result — the router asserts it on create).
const BASE_BRANCH: &str = "main";

/// The routing transport's model: the open pulls (id, source branch,
/// title, description), the tags (the release markers) and the PR
/// comments (id, raw body).
struct State {
    pulls: Vec<(u64, String, String, String)>,
    next_number: u64,
    tags: Vec<String>,
    comments: Vec<(u64, String)>,
    next_comment: u64,
}

/// The Bitbucket-shaped routing transport (statuses are values; the
/// routes mirror the documented endpoints, research §2–§4).
struct Router {
    state: Mutex<State>,
}

impl Router {
    fn new() -> Self {
        Self {
            state: Mutex::new(State {
                pulls: Vec::new(),
                next_number: 40,
                // The release markers the git-side tag phase pushed
                // before the machine's pass (§11.1 order: tags land
                // git-side first — research §3, the probe-hit mapping).
                tags: vec!["conformance-v0.1.0".to_owned()],
                comments: Vec::new(),
                next_comment: 900,
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
            body: r#"{"type":"error","error":{"message":"Resource not found"}}"#.to_owned(),
        }
    }

    fn pull_json(id: u64, source_branch: &str, title: &str, description: &str) -> String {
        serde_json::json!({
            "id": id,
            "title": title,
            "description": description,
            "state": "open",
            "links": { "html": { "href": format!("https://bitbucket.org/{REPO}/pullrequests/{id}") } },
            "source": { "branch": { "name": source_branch } },
        })
        .to_string()
    }

    fn list_json(items: &[String]) -> String {
        format!("[{}]", items.join(","))
    }

    /// One paginated envelope page (research §4: `values` + the
    /// `pagelen` the scan sends; the router serves everything on one
    /// short page).
    fn comment_page_json(items: &[String]) -> String {
        format!(
            r#"{{"size":{},"page":1,"pagelen":50,"values":[{}]}}"#,
            items.len(),
            items.join(",")
        )
    }

    fn comment_json(id: u64, body: &str) -> String {
        serde_json::json!({
            "id": id,
            "content": { "raw": body },
            "links": { "html": { "href": format!("https://bitbucket.org/{REPO}/pullrequests/7/comments/{id}") } },
        })
        .to_string()
    }

    fn tag_json(tag: &str) -> String {
        serde_json::json!({
            "type": "tag",
            "name": tag,
            "links": { "html": { "href": format!("https://bitbucket.org/{REPO}/commits/tag/{tag}") } },
        })
        .to_string()
    }
}

/// The body's string field (the adapter always sends valid JSON).
fn body_field(body: &str, key: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value.get(key).and_then(|v| v.as_str().map(str::to_owned)))
}

/// The body's nested `source.branch.name` field (research §2).
fn source_branch_field(body: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("source")
                .and_then(|source| source.get("branch"))
                .and_then(|branch| branch.get("name"))
                .and_then(|name| name.as_str().map(str::to_owned))
        })
}

/// The path's trailing numeric id.
fn trailing_id(path: &str) -> Option<u64> {
    path.rsplit('/').next().and_then(|id| id.parse().ok())
}

impl Router {
    /// The pull surface's arm: the list, the create and the one-pull
    /// update/close — the suffix after the repo's `pullrequests` segment
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
                    .map(|(id, source, title, description)| {
                        Self::pull_json(*id, source, title, description)
                    })
                    .collect();
                Ok(Self::ok(Self::list_json(&items)))
            }
            ("POST", "") => {
                let source = source_branch_field(body_json).unwrap_or_default();
                let title = body_field(body_json, "title").unwrap_or_default();
                let description = body_field(body_json, "description").unwrap_or_default();
                let destination = serde_json::from_str::<serde_json::Value>(body_json)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("destination")
                            .and_then(|d| d.get("branch"))
                            .and_then(|b| b.get("name"))
                            .and_then(|name| name.as_str().map(str::to_owned))
                    })
                    .unwrap_or_default();
                assert_eq!(
                    destination, BASE_BRANCH,
                    "the create always carries the §11.2 destination explicitly"
                );
                state.next_number += 1;
                let id = state.next_number;
                state
                    .pulls
                    .push((id, source.clone(), title.clone(), description.clone()));
                Ok(Self::created(Self::pull_json(
                    id,
                    &source,
                    &title,
                    &description,
                )))
            }
            ("PUT", s) if s.starts_with('/') && !s.ends_with("/decline") => {
                let id = trailing_id(s).unwrap_or_default();
                let title = body_field(body_json, "title").unwrap_or_default();
                let description = body_field(body_json, "description").unwrap_or_default();
                for (known, _, known_title, known_description) in &mut state.pulls {
                    if *known == id {
                        known_title.clone_from(&title);
                        known_description.clone_from(&description);
                    }
                }
                let (id, source, title, description) = state
                    .pulls
                    .iter()
                    .find(|(known, _, _, _)| *known == id)
                    .map(|(i, s, t, d)| (*i, s.clone(), t.clone(), d.clone()))
                    .unwrap_or_default();
                Ok(Self::ok(Self::pull_json(id, &source, &title, &description)))
            }
            ("POST", s) if s.ends_with("/decline") => {
                let id = s
                    .strip_suffix("/decline")
                    .and_then(trailing_id)
                    .unwrap_or_default();
                state.pulls.retain(|(known, _, _, _)| *known != id);
                Ok(Self::ok(Self::pull_json(id, BRANCH, "t", "b")))
            }
            _ => return None,
        })
    }

    /// The comment surface's arm: the paginated scan, the create and
    /// the in-place rewrite (research §4). `Some` when the arm matched.
    fn route_comments(
        state: &mut State,
        method: &str,
        suffix: &str,
        body_json: &str,
    ) -> Option<Result<HttpResponse, ForgeError>> {
        let raw_of = |body_json: &str| {
            serde_json::from_str::<serde_json::Value>(body_json)
                .ok()
                .and_then(|value| {
                    value
                        .get("content")
                        .and_then(|content| content.get("raw"))
                        .and_then(|raw| raw.as_str().map(str::to_owned))
                })
                .unwrap_or_default()
        };
        Some(match (method, suffix.contains("/comments/")) {
            ("GET", false) => {
                let items: Vec<String> = state
                    .comments
                    .iter()
                    .map(|(id, body)| Self::comment_json(*id, body))
                    .collect();
                Ok(Self::ok(Self::comment_page_json(&items)))
            }
            ("POST", false) => {
                let raw = raw_of(body_json);
                state.next_comment += 1;
                let id = state.next_comment;
                state.comments.push((id, raw.clone()));
                Ok(Self::created(Self::comment_json(id, &raw)))
            }
            ("PUT", true) => {
                let id = trailing_id(suffix).unwrap_or_default();
                let raw = raw_of(body_json);
                for (known, body) in &mut state.comments {
                    if *known == id {
                        body.clone_from(&raw);
                    }
                }
                Ok(Self::ok(Self::comment_json(id, &raw)))
            }
            _ => return None,
        })
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
        // find probe's `q` filter rides the `?…` parameter).
        let clean = path.split('?').next().unwrap_or(&path).to_owned();
        let prefix = format!("/repositories/{REPO}");
        let mut state = self.lock();
        if let Some(suffix) = clean
            .strip_prefix(&format!("{prefix}/pullrequests"))
            .filter(|rest| !rest.contains("/comments"))
            && let Some(routed) = Self::route_pull(&mut state, request.method, suffix, &body_json)
        {
            return routed;
        }
        // The release surface: the tag probe (the release marker,
        // research §3) — a GET never creates.
        if let Some(tag) = clean.strip_prefix(&format!("{prefix}/refs/tags/"))
            && request.method == "GET"
        {
            return Ok(if state.tags.iter().any(|known| known == tag) {
                Self::ok(Self::tag_json(tag))
            } else {
                Self::not_found()
            });
        }
        if let Some(suffix) = clean
            .strip_prefix(&format!("{prefix}/pullrequests"))
            .filter(|rest| rest.contains("/comments"))
            && let Some(routed) =
                Self::route_comments(&mut state, request.method, suffix, &body_json)
        {
            return routed;
        }
        Err(ForgeError::transport(format!(
            "the routing transport has no route for {} {path}",
            request.method
        )))
    }
}

/// The Bitbucket factory: a fresh adapter over a fresh routing
/// transport per scenario — the scripted-conversation construction the
/// contract tests use.
fn bitbucket_factory(scenario: Scenario) -> Box<dyn Forge> {
    let _ = scenario;
    let forge = BitbucketRestForge::with_transport(
        BitbucketEndpoint::with_base(REPO, BASE)
            .unwrap_or_else(|error| unreachable!("static endpoint is valid: {error}")),
        "conformance-token",
        Router::new(),
    );
    Box::new(forge)
}

#[test]
fn matrix_is_green_over_bitbucket_rest() {
    let report = run_conformance_matrix(&mut bitbucket_factory);
    assert!(
        report.is_green(),
        "the Bitbucket REST adapter fails the shared matrix: {report:?}"
    );
}

#[test]
fn bitbucket_declines_exactly_the_commit_capability() {
    // The one documented decline (research §5: the commits resource
    // "does NOT support new commit creation", so `commit_via_api` rides
    // the trait's honest decline naming the adapter) — every other
    // capability is implemented: the release act is the tag probe's
    // existed shape (research §3), which the MachinePropagate scenario
    // accepts.
    let report = run_conformance_matrix(&mut bitbucket_factory);
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
