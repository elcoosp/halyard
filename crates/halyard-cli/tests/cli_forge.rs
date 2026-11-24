//! CLI `forge-smoke` + live `pr` over a loopback mock forge (M4.1b,
//! §15.1 "mock forge server + real git" lane, ADR-0013): the compiled
//! binary runs against a `std::net` HTTP/1.1 fake forge (no TLS on the
//! loopback, no new dependencies) and the assertions cover both sides —
//! the report the binary prints and the requests the fake server
//! actually received (headers, paths, JSON bodies).

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use halyard_test_support::{CommitSpec, GitFixture};

type BoxError = Box<dyn std::error::Error>;

/// A string field of a captured request/echo document (missing fields
/// render empty — the assertions make the difference visible).
fn field(value: &serde_json::Value, name: &str) -> String {
    value
        .get(name)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// One scripted response: status + body (the fake forge is a
/// deterministic FIFO — §11.5 accounting rides the script order).
struct FakeForge {
    script: Mutex<VecDeque<(u16, String)>>,
    received: Mutex<Vec<serde_json::Value>>,
}

impl FakeForge {
    /// Builds the fake forge with a response script; `serve` consumes
    /// one request per scripted entry, echoing the received request's
    /// salient parts into `received`.
    fn new(
        script: Vec<(u16, String)>,
    ) -> Result<(Arc<Self>, String, thread::JoinHandle<()>), BoxError> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let forge = Arc::new(Self {
            script: Mutex::new(script.into_iter().collect()),
            received: Mutex::new(Vec::new()),
        });
        let server = Arc::clone(&forge);
        let handle = thread::spawn(move || {
            while let Ok((mut stream, _addr)) = listener.accept() {
                if server.serve_one(&mut stream).is_err() {
                    return;
                }
                if server
                    .script
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .is_empty()
                {
                    return;
                }
            }
        });
        Ok((forge, format!("http://127.0.0.1:{port}"), handle))
    }

    /// Reads one HTTP/1.1 exchange, records it, answers with the next
    /// scripted response.
    fn serve_one(&self, stream: &mut TcpStream) -> Result<(), BoxError> {
        let mut buf = Vec::new();
        let mut chunk = [0_u8; 4096];
        let header_end;
        loop {
            let read = stream.read(&mut chunk)?;
            if read == 0 {
                return Ok(());
            }
            if let Some(slice) = chunk.get(..read) {
                buf.extend_from_slice(slice);
            }
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                header_end = pos;
                break;
            }
        }
        let head = buf.get(..header_end).map_or(String::new(), |bytes| {
            String::from_utf8_lossy(bytes).into_owned()
        });
        let mut lines = head.lines();
        let request_line = lines.next().unwrap_or_default().to_string();
        let mut parts = request_line.split(' ');
        let method = parts.next().unwrap_or_default().to_owned();
        let path = parts.next().unwrap_or_default().to_owned();
        let mut content_length = 0_usize;
        let mut authorization = String::new();
        let mut accept = String::new();
        let mut api_version = String::new();
        let mut user_agent = String::new();
        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_owned();
            match name.as_str() {
                "content-length" => content_length = value.parse().unwrap_or(0),
                "authorization" => authorization = value,
                "accept" => accept = value,
                "x-github-api-version" => api_version = value,
                "user-agent" => user_agent = value,
                _ => {}
            }
        }
        let body_start = header_end + 4;
        while buf.len() < body_start + content_length {
            let read = stream.read(&mut chunk)?;
            if read == 0 {
                break;
            }
            if let Some(slice) = chunk.get(..read) {
                buf.extend_from_slice(slice);
            }
        }
        let body = buf
            .get(body_start..body_start + content_length)
            .map_or(String::new(), |bytes| {
                String::from_utf8_lossy(bytes).into_owned()
            });
        self.received
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(serde_json::json!({
                "method": method,
                "path": path,
                "body": body,
                "authorization": authorization,
                "accept": accept,
                "api_version": api_version,
                "user_agent": user_agent,
            }));
        let Some((status, payload)) = self
            .script
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
        else {
            return Ok(());
        };
        let reason = match status {
            201 => "Created",
            304 => "Not Modified",
            401 => "Unauthorized",
            404 => "Not Found",
            _ => "OK",
        };
        let response = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            payload.len(),
            payload
        );
        stream.write_all(response.as_bytes())?;
        stream.flush()?;
        Ok(())
    }

    /// The received requests, oldest first.
    fn received(&self) -> Vec<serde_json::Value> {
        self.received
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Runs the binary with the smoke environment; returns (exit code,
/// combined output).
fn halyard_with_env(
    root: &std::path::Path,
    args: &[&str],
    envs: &[(&str, &str)],
) -> Result<(u8, String), BoxError> {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_halyard"))
        .arg("-C")
        .arg(root)
        .args(args)
        .env("NO_COLOR", "1")
        .envs(envs.iter().copied())
        .output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let code = u8::try_from(out.status.code().unwrap_or_default()).unwrap_or_default();
    Ok((code, text))
}

fn fixture_dir(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir =
        std::env::temp_dir().join(format!("halyard-cli-forge-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn build_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let dir = fixture_dir(label)?;
    let mut fx = GitFixture::init_in(&dir)?;
    fx.commit_with(&CommitSpec::files(
        vec![
            (
                "Cargo.toml".to_owned(),
                "[package]\nname = \"solo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n".to_owned(),
            ),
            (
                "src/lib.rs".to_owned(),
                "pub const C: usize = 1;\n".to_owned(),
            ),
        ],
        "feat: scaffold the crate",
    ))?;
    Ok(fx)
}

/// A PR body shaped like the REST response the adapter parses.
fn pr_body(number: u64, branch: &str) -> String {
    serde_json::json!({
        "number": number,
        "title": "chore: release v0.1.1",
        "body": "## 🚀 Release plan",
        "html_url": format!("https://github.com/octo/halyard-demo/pull/{number}"),
        "head": { "ref": branch },
    })
    .to_string()
}

/// `forge-smoke` over the loopback forge: the probe rides the
/// documented 404→`false` shape, the report carries the rate snapshot
/// and the call log, and the fake server saw the mandatory headers.
#[test]
fn forge_smoke_probes_read_only_over_loopback() -> Result<(), BoxError> {
    let fx = build_workspace("smoke")?;
    let (forge, base, server) =
        FakeForge::new(vec![(404, "{\"message\":\"Not Found\"}".to_owned())])?;
    let (code, out) = halyard_with_env(
        fx.root(),
        &[
            "forge-smoke",
            "--repo",
            "octo/halyard-demo",
            "--format",
            "json",
        ],
        &[
            ("GITHUB_API_URL", &base),
            ("HALYARD_FORGE_TOKEN", "smoke-token"),
        ],
    )?;
    server.join().expect("the fake forge served the probe");
    assert_eq!(code, 0, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    assert_eq!(
        value.get("mode").and_then(serde_json::Value::as_str),
        Some("smoke")
    );
    assert_eq!(
        value.get("repo").and_then(serde_json::Value::as_str),
        Some("octo/halyard-demo")
    );
    assert_eq!(
        value
            .get("release_exists")
            .and_then(serde_json::Value::as_bool),
        Some(false),
        "the probe tag never exists on a healthy forge"
    );
    assert_eq!(
        value
            .get("calls")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(1),
        "exactly one read-only probe: {out}"
    );
    let received = forge.received();
    assert_eq!(received.len(), 1);
    let request = received.first().expect("one capture");
    assert_eq!(
        field(request, "path"),
        "/repos/octo/halyard-demo/releases/tags/halyard-forge-smoke-probe"
    );
    assert_eq!(field(request, "authorization"), "Bearer smoke-token");
    assert_eq!(field(request, "accept"), "application/vnd.github+json");
    assert_eq!(field(request, "api_version"), "2022-11-28");
    assert!(
        field(request, "user_agent").starts_with("halyard/"),
        "the mandatory User-Agent rides the real client: {request}"
    );
    Ok(())
}

/// `forge-smoke` without credentials fails closed with `E-FORGE-AUTH`
/// before any network attempt (the nightly job's opt-in contract). The
/// code is asserted via the JSON error payload — the human render
/// carries the message, not the stable code.
#[test]
fn forge_smoke_needs_a_token() -> Result<(), BoxError> {
    let fx = build_workspace("smoke-no-token")?;
    let (code, out) = halyard_with_env(
        fx.root(),
        &[
            "forge-smoke",
            "--repo",
            "octo/halyard-demo",
            "--format",
            "json",
        ],
        &[("HALYARD_FORGE_TOKEN", ""), ("GITHUB_TOKEN", "")],
    )?;
    assert_eq!(code, 1, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    let code_field = value
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(serde_json::Value::as_str);
    assert_eq!(code_field, Some("E-FORGE-AUTH"), "{out}");
    Ok(())
}

/// A `401` from the forge surfaces as `E-FORGE-AUTH` with exit 1 — the
/// §15.4 code passes through the CLI verbatim.
#[test]
fn forge_smoke_surfaces_the_auth_failure() -> Result<(), BoxError> {
    let fx = build_workspace("smoke-auth")?;
    let (forge, base, server) =
        FakeForge::new(vec![(401, "{\"message\":\"Bad credentials\"}".to_owned())])?;
    let (code, out) = halyard_with_env(
        fx.root(),
        &[
            "forge-smoke",
            "--repo",
            "octo/halyard-demo",
            "--format",
            "json",
        ],
        &[("GITHUB_API_URL", &base), ("HALYARD_FORGE_TOKEN", "bad")],
    )?;
    server.join().expect("the fake forge served the probe");
    assert_eq!(code, 1, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    let code = value
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(serde_json::Value::as_str);
    assert_eq!(code, Some("E-FORGE-AUTH"), "{out}");
    let received = forge.received();
    assert_eq!(received.len(), 1, "the failing probe is still observable");
    Ok(())
}

/// Live `pr` over the loopback forge: the machine's prepare phase
/// opens the Release PR for real — the fake server saw the find probes
/// and the create POST with the §11.2-derived base, and the report
/// names the opened PR. The git remote is GitHub-shaped so the slug
/// resolution succeeds; the loopback API base keeps the exchange
/// hermetic.
#[test]
fn pr_live_opens_the_release_pr_over_loopback() -> Result<(), BoxError> {
    let mut fx = build_workspace("pr-live")?;
    // The post-merge state (the §11.1 machine PROPAGATEs while anything
    // is pending, so the prepare phase only runs once the manifest
    // version is tagged): v0.1.0 tagged, the release commit carries
    // v0.1.1 and is itself tagged (the tag phase's own doing), and one
    // fresh `feat` commit lands after the release — the plan proposes
    // 0.1.2, the prepare phase commits the content and opens the
    // Release PR.
    fx.tag_annotated("v0.1.0", "solo v0.1.0")?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "Cargo.toml".to_owned(),
            "[package]\nname = \"solo\"\nversion = \"0.1.1\"\nedition = \"2021\"\n".to_owned(),
        )],
        "chore(main): release v0.1.1",
    ))?;
    fx.tag_annotated("v0.1.1", "solo v0.1.1")?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "src/lib.rs".to_owned(),
            "pub const C: usize = 2;\n".to_owned(),
        )],
        "feat: extend the crate surface",
    ))?;
    // The §11.5 commit mode: the API path (the forge signs the content
    // commit) — the git-side CAS push is the M4.2 remainder's shell
    // half, so the live pass rides `commit_via_api`.
    fx.write("halyard.toml", "[forge]\ncommit_mode = \"api\"\n")?;
    fx.run_git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/octo/halyard-demo.git",
    ])?;
    // The §11.1 probe is git-side (`ls-remote`, 0 REST): the origin URL
    // stays GitHub-shaped (the forge slug parses from it) and `insteadOf`
    // rewrites it onto an offline bare repo — the standard offline-CI
    // rewriting, so the probe runs without any network.
    probe_offline_origin(&mut fx)?;
    // The forecast names the exact branch the live run will target.
    let branch = forecast_release_branch(&fx)?;
    // The scripted exchange, in the §11.1 order: the API-commit chain
    // (ref → commit → tree → commit → ref CAS update — steps 1–6 of
    // `commit_via_api`), then the machine's find, the upsert's own find
    // and the create.
    let tip = "1".repeat(40);
    let base_tree = "2".repeat(40);
    let new_tree = "3".repeat(40);
    let new_commit = "4".repeat(40);
    let (forge, api, server) = FakeForge::new(vec![
        (
            200,
            format!("{{\"object\": {{\"sha\": \"{tip}\", \"type\": \"commit\"}}}}"),
        ),
        (
            200,
            format!("{{\"sha\": \"{tip}\", \"tree\": {{\"sha\": \"{base_tree}\"}}}}"),
        ),
        (201, format!("{{\"sha\": \"{new_tree}\"}}")),
        (201, format!("{{\"sha\": \"{new_commit}\"}}")),
        (
            200,
            format!("{{\"object\": {{\"sha\": \"{new_commit}\"}}}}"),
        ),
        (200, "[]".to_owned()),
        (200, "[]".to_owned()),
        (201, pr_body(9, &branch)),
    ])?;
    let (code, out) = halyard_with_env(
        fx.root(),
        &["pr", "--format", "json"],
        &[
            ("GITHUB_API_URL", &api),
            ("HALYARD_FORGE_TOKEN", "smoke-token"),
        ],
    )?;
    server.join().expect("the fake forge served the pass");
    assert_eq!(code, 0, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    assert_eq!(
        value.get("mode").and_then(serde_json::Value::as_str),
        Some("live")
    );
    assert_eq!(
        value.get("repo").and_then(serde_json::Value::as_str),
        Some("octo/halyard-demo")
    );
    let effects = value
        .get("effects")
        .and_then(serde_json::Value::as_array)
        .ok_or("the report carries the machine effects")?;
    let kinds: Vec<String> = effects
        .iter()
        .filter_map(|effect| effect.get("type").and_then(serde_json::Value::as_str))
        .map(str::to_owned)
        .collect();
    assert!(
        kinds.iter().any(|kind| kind == "pr_opened"),
        "the Release PR opened after the git-side half: {out}"
    );
    assert_eq!(
        forge.received().len(),
        3,
        "the forge sees only the PR half — the content rode git"
    );
    // The remote release branch carries the content commit: its tree is
    // the content tree the plan's edits produce.
    assert_release_branch_carries_the_content(&fx, &branch)?;
    Ok(())
}

/// The dry forecast's release branch (the §11.2 `halyard/release/<base>`
/// shape): the name the live run will target, fetched offline.
fn forecast_release_branch(fx: &GitFixture) -> Result<String, BoxError> {
    let (_, dry_out) = halyard_with_env(
        fx.root(),
        &["pr", "--dry", "--format", "json"],
        &[("GITHUB_TOKEN", "")],
    )?;
    let dry: serde_json::Value = serde_json::from_str(dry_out.trim())?;
    let branch = dry
        .get("branch")
        .and_then(serde_json::Value::as_str)
        .ok_or("the forecast names the release branch")?
        .to_owned();
    branch
        .strip_prefix("halyard/release/")
        .filter(|base| !base.is_empty())
        .ok_or("the branch is the §11.2 shape")?;
    Ok(branch)
}

/// The §11.1 step-3 truth pin: the pushed release branch's tip commit is
/// the staged release commit and its tree equals the content tree the
/// plan's applied edits produce over the base snapshot.
fn assert_release_branch_carries_the_content(
    fx: &GitFixture,
    branch: &str,
) -> Result<(), BoxError> {
    let tip = fx.run_git(&["ls-remote", "origin", &format!("refs/heads/{branch}")])?;
    assert!(!tip.trim().is_empty(), "the branch exists remotely: {tip}");
    let commit = tip.split('\t').next().unwrap_or_default().trim().to_owned();
    let tree = fx.run_git(&["rev-parse", &format!("{commit}^{{tree}}")])?;
    let staged = fx.run_git(&["show", "--format=%s", "--no-patch", &commit])?;
    assert_eq!(staged.trim(), "chore: release v0.1.1", "{staged}");
    let expected_tree = fx.run_git(&["rev-parse", "HEAD^{tree}"])?;
    assert_eq!(
        tree.trim(),
        expected_tree.trim(),
        "the release branch carries the content tree (base + plan edits)"
    );
    Ok(())
}

/// Keeps the §11.1 probe offline: `insteadOf` rewrites the GitHub-shaped
/// origin onto a local bare repo (the forge slug still parses from the
/// origin URL, and `ls-remote`/`fetch` stay on the loopback disk).
fn probe_offline_origin(fx: &mut GitFixture) -> Result<(), BoxError> {
    let parent = fx.root().parent().ok_or("fixture root has a parent")?;
    let bare = parent.join("origin-probe.git");
    fx.run_git(&["init", "--bare", bare.to_string_lossy().as_ref()])?;
    fx.run_git(&[
        "config",
        &format!("url.{}.insteadOf", bare.to_string_lossy()),
        "https://github.com/octo/halyard-demo.git",
    ])?;
    Ok(())
}

/// The `HALYARD_FORGE_*` env grammar fails closed on a present-but-
/// invalid value, naming the variable and the accepted shape — the
/// App-env convention (ADR-0014), `E-FORGE-REQUEST`. The parse runs
/// after the credentials resolve and before any HTTP: `forge-smoke`
/// with a token exercises exactly that gate.
#[test]
fn governor_env_invalid_value_fails_closed_naming_the_variable() -> Result<(), BoxError> {
    let fx = build_workspace("governor-env")?;
    let (code, out) = halyard_with_env(
        fx.root(),
        &[
            "forge-smoke",
            "--repo",
            "octo/halyard-demo",
            "--format",
            "json",
        ],
        &[
            ("GITHUB_TOKEN", ""),
            ("HALYARD_FORGE_TOKEN", "smoke-token"),
            ("HALYARD_FORGE_MIN_CALL_SPACING_MS", "soon"),
        ],
    )?;
    assert_eq!(code, 1, "an invalid governor env fails closed: {out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    let code_field = value
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(serde_json::Value::as_str);
    assert_eq!(code_field, Some("E-FORGE-REQUEST"), "{out}");
    let message = value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    assert!(
        message.contains("HALYARD_FORGE_MIN_CALL_SPACING_MS"),
        "the error names the variable: {message}"
    );
    assert!(
        message.contains("non-negative integer"),
        "the error states the accepted shape: {message}"
    );
    Ok(())
}

/// `forge-smoke`'s JSON carries the governor row under the default
/// policy (M5.5): pacing 1 000 ms, unbounded budget, three retries —
/// and the probe itself is the run's one unit of spend.
#[test]
fn forge_smoke_carries_the_governor_row() -> Result<(), BoxError> {
    let fx = build_workspace("smoke-governor")?;
    let (forge, base, server) =
        FakeForge::new(vec![(404, "{\"message\":\"Not Found\"}".to_owned())])?;
    let (code, out) = halyard_with_env(
        fx.root(),
        &[
            "forge-smoke",
            "--repo",
            "octo/halyard-demo",
            "--format",
            "json",
        ],
        &[
            ("GITHUB_API_URL", &base),
            ("HALYARD_FORGE_TOKEN", "smoke-token"),
        ],
    )?;
    server.join().expect("the fake forge served the probe");
    assert_eq!(code, 0, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    let governor = value
        .get("governor")
        .ok_or("the smoke JSON carries the governor row")?;
    assert_eq!(
        governor
            .get("min_spacing_ms")
            .and_then(serde_json::Value::as_u64),
        Some(1_000),
        "the shell default paces content calls"
    );
    assert!(
        governor
            .get("max_calls")
            .and_then(serde_json::Value::as_u64)
            .is_none(),
        "the default budget is unbounded: {governor}"
    );
    assert_eq!(
        governor.get("spent").and_then(serde_json::Value::as_u64),
        Some(1),
        "the read-only probe is the run's one spend"
    );
    assert_eq!(forge.received().len(), 1, "one probe request");
    Ok(())
}

// ---------------------------------------------------------------------------
// The §11.6 check-comment mode over the loopback forge (M4.5 step 1):
// `check --pr-title --comment` finds the PR under review by its head
// branch and marker-upserts the verdict comment. The assertions cover
// both sides — the report the binary prints and the requests the fake
// server actually received.
// ---------------------------------------------------------------------------

/// A comment-list body shaped like the REST response the adapter parses.
fn comment_list(comments: &[serde_json::Value]) -> String {
    serde_json::Value::Array(comments.to_vec()).to_string()
}

fn comment(id: u64, body: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "html_url": format!("https://github.com/octo/halyard-demo/pull/7#issuecomment-{id}"),
        "body": body,
    })
}

/// The check-comment pass over the loopback forge: the PR is found by
/// head branch, the marker scan comes back empty, and the verdict is
/// posted for real — the fake server saw the find probe, the scan and
/// the create POST, and the report names the posted comment.
#[test]
fn check_comment_posts_the_verdict_over_loopback() -> Result<(), BoxError> {
    let fx = build_workspace("check-comment-post")?;
    fx.run_git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/octo/halyard-demo.git",
    ])?;
    // The find exchange parses a PR *list* — the scripted PR rides an
    // array (the wire shape the contract tests pin).
    let pr = format!("[{}]", pr_body(7, "feature/topic"));
    let (forge, api, server) = FakeForge::new(vec![
        (200, pr),
        (200, "[]".to_owned()),
        (
            201,
            r#"{"id":9,"html_url":"https://github.com/octo/halyard-demo/pull/7#issuecomment-9"}"#
                .to_owned(),
        ),
    ])?;
    let (code, out) = halyard_with_env(
        fx.root(),
        &[
            "--format",
            "json",
            "check",
            "--pr-title",
            "feat: extend the crate surface",
            "--comment",
        ],
        &[
            ("GITHUB_API_URL", &api),
            ("HALYARD_FORGE_TOKEN", "smoke-token"),
            ("GITHUB_HEAD_REF", "feature/topic"),
        ],
    )?;
    server
        .join()
        .map_err(|_| "the fake forge thread panicked")?;
    assert_eq!(code, 0, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    let comment = value
        .get("comment")
        .ok_or("the report carries the comment outcome")?;
    assert_eq!(
        comment.get("posted").and_then(serde_json::Value::as_bool),
        Some(true),
        "{out}"
    );
    assert_eq!(
        comment.get("created").and_then(serde_json::Value::as_bool),
        Some(true),
        "the first run creates: {out}"
    );
    assert_eq!(
        comment.get("url").and_then(serde_json::Value::as_str),
        Some("https://github.com/octo/halyard-demo/pull/7#issuecomment-9"),
        "{out}"
    );
    // The fake server saw the find probe, the marker scan and the
    // create POST with the marker.
    let received = forge.received();
    assert_eq!(
        received.len(),
        3,
        "one find GET + one scan GET + one create POST"
    );
    let post = received
        .iter()
        .find(|request| field(request, "method") == "POST")
        .ok_or("the create POST reached the fake forge")?;
    let body = field(post, "body");
    assert!(
        body.contains("<!-- halyard:check v1 -->"),
        "the marker rides: {body}"
    );
    assert!(body.contains("feat: extend the crate surface"), "{body}");
    Ok(())
}

/// The idempotent re-run: the marker scan finds the existing comment and
/// the verdict is `PATCH`ed in place — no duplicate comment (I1).
#[test]
fn check_comment_rerun_rewrites_in_place_over_loopback() -> Result<(), BoxError> {
    let fx = build_workspace("check-comment-rerun")?;
    fx.run_git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/octo/halyard-demo.git",
    ])?;
    let pr = format!("[{}]", pr_body(7, "feature/topic"));
    let existing = comment(
        9,
        "<!-- halyard:check v1 -->\n\n## 🧪 halyard check\n\nstale verdict",
    );
    let (forge, api, server) = FakeForge::new(vec![
        (200, pr),
        (200, comment_list(&[existing])),
        (
            200,
            r#"{"id":9,"html_url":"https://github.com/octo/halyard-demo/pull/7#issuecomment-9"}"#
                .to_owned(),
        ),
    ])?;
    let (code, out) = halyard_with_env(
        fx.root(),
        &[
            "--format",
            "json",
            "check",
            "--pr-title",
            "feat: extend the crate surface",
            "--comment",
        ],
        &[
            ("GITHUB_API_URL", &api),
            ("HALYARD_FORGE_TOKEN", "smoke-token"),
            ("GITHUB_HEAD_REF", "feature/topic"),
        ],
    )?;
    server
        .join()
        .map_err(|_| "the fake forge thread panicked")?;
    assert_eq!(code, 0, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    let comment = value
        .get("comment")
        .ok_or("the report carries the outcome")?;
    assert_eq!(
        comment.get("created").and_then(serde_json::Value::as_bool),
        Some(false),
        "the re-run rewrites in place: {out}"
    );
    let received = forge.received();
    let patched = received
        .iter()
        .find(|request| field(request, "method") == "PATCH")
        .ok_or("the PATCH reached the fake forge")?;
    assert!(
        field(patched, "path").contains("issues/comments/9"),
        "the rewrite addresses the found comment: {}",
        field(patched, "path")
    );
    Ok(())
}

/// No open PR on the head branch: nothing is posted, the report says so
/// (the §11.4 not-found-is-a-value convention surfaced), and the run
/// still exits 0 — the verdict itself is unchanged.
#[test]
fn check_comment_without_a_target_pr_reports_the_skip() -> Result<(), BoxError> {
    let fx = build_workspace("check-comment-nopr")?;
    fx.run_git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/octo/halyard-demo.git",
    ])?;
    let (forge, api, server) = FakeForge::new(vec![(200, "[]".to_owned())])?;
    let (code, out) = halyard_with_env(
        fx.root(),
        &[
            "--format",
            "json",
            "check",
            "--pr-title",
            "feat: extend the crate surface",
            "--comment",
        ],
        &[
            ("GITHUB_API_URL", &api),
            ("HALYARD_FORGE_TOKEN", "smoke-token"),
            ("GITHUB_HEAD_REF", "feature/topic"),
        ],
    )?;
    server
        .join()
        .map_err(|_| "the fake forge thread panicked")?;
    assert_eq!(code, 0, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    let comment = value
        .get("comment")
        .ok_or("the report carries the outcome")?;
    assert_eq!(
        comment.get("posted").and_then(serde_json::Value::as_bool),
        Some(false),
        "{out}"
    );
    assert!(
        comment
            .get("reason")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .contains("feature/topic"),
        "the skip names the branch it probed: {out}"
    );
    assert_eq!(forge.received().len(), 1, "one find probe, no comment POST");
    Ok(())
}
