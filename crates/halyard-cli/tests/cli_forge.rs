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
        let mut private_token = String::new();
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
                "private-token" => private_token = value,
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
                "private_token": private_token,
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

/// The GitLab-shaped MR body the M7 adapter's upsert parses (research
/// §3: `iid`, `source_branch`, `web_url`; the docs' field names).
fn mr_body(number: u64, branch: &str) -> String {
    serde_json::json!({
        "iid": number,
        "title": "chore: release v0.1.1",
        "description": "## 🚀 Release plan",
        "state": "opened",
        "source_branch": branch,
        "target_branch": "main",
        "web_url": format!("https://gitlab.com/octo/halyard-demo/-/merge_requests/{number}"),
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
    // The §11.5 commit mode: the git half — the shell stages the plan's
    // edits, commits on the base snapshot and pushes the release branch
    // under the CAS lease (§11.2); the forge sees only the PR half. The
    // api half's forge-signed chain is pinned by the forge-github
    // contract suite and the GitLab loopback e2e.
    fx.write("halyard.toml", "[forge]\ncommit_mode = \"git\"\n")?;
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
    // The scripted exchange, in the §11.1 order: the machine's find,
    // the upsert's own find and the create — the content rode the
    // git-side CAS push, so the forge sees only the PR half.
    let (forge, api, server) = FakeForge::new(vec![
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

/// The §11.1 step-3 truth pin: the pushed release branch's tip commit
/// carries the plan's release title and its tree carries the plan's
/// applied edit (the manifest's next version) over the base snapshot.
fn assert_release_branch_carries_the_content(
    fx: &GitFixture,
    branch: &str,
) -> Result<(), BoxError> {
    let tip = fx.run_git(&["ls-remote", "origin", &format!("refs/heads/{branch}")])?;
    assert!(!tip.trim().is_empty(), "the branch exists remotely: {tip}");
    let commit = tip.split('\t').next().unwrap_or_default().trim().to_owned();
    let staged = fx.run_git(&["show", "--format=%s", "--no-patch", &commit])?;
    assert_eq!(
        staged.trim(),
        "chore: release v0.1.2",
        "the staged commit carries the plan's release title"
    );
    let manifest = fx.run_git(&["show", &format!("{commit}:Cargo.toml")])?;
    assert!(
        manifest.contains("version = \"0.1.2\""),
        "the staged tree carries the plan's applied edit: {manifest}"
    );
    Ok(())
}

/// Keeps the §11.1 probe offline: `insteadOf` rewrites the GitHub-shaped
/// origin onto a local bare repo (the forge slug still parses from the
/// origin URL, and `ls-remote`/`fetch` stay on the loopback disk).
fn probe_offline_origin(fx: &mut GitFixture) -> Result<(), BoxError> {
    offline_origin_rewrite(fx, "https://github.com/octo/halyard-demo.git")
}

/// The GitLab lane's variant: the same `insteadOf` rewrite over the
/// GitLab-shaped origin URL (the adapter's remote grammar still parses
/// the configured URL, and `ls-remote`/`fetch` stay on the disk).
fn gitlab_probe_offline_origin(fx: &mut GitFixture) -> Result<(), BoxError> {
    offline_origin_rewrite(fx, "https://gitlab.com/octo/halyard-demo.git")
}

fn offline_origin_rewrite(fx: &mut GitFixture, origin: &str) -> Result<(), BoxError> {
    let parent = fx.root().parent().ok_or("fixture root has a parent")?;
    let bare = parent.join(format!(
        "origin-probe-{}.git",
        origin
            .split('/')
            .next_back()
            .unwrap_or_default()
            .strip_suffix(".git")
            .unwrap_or_default()
    ));
    fx.run_git(&["init", "--bare", bare.to_string_lossy().as_ref()])?;
    fx.run_git(&[
        "config",
        &format!("url.{}.insteadOf", bare.to_string_lossy()),
        origin,
    ])?;
    Ok(())
}

/// Live `pr` over the GitLab lane (M7 step 1, ADR-0049): the same
/// machine pass the GitHub loopback test pins, over a GitLab-shaped
/// origin — the fake forge saw the URL-encoded project path, the
/// `PRIVATE-TOKEN` auth, the commits-API probe + create and the MR
/// create, and the report names the opened MR. `GITLAB_API_URL` keeps
/// the exchange on the loopback; `insteadOf` keeps the git-side probe
/// offline.
#[test]
fn pr_live_opens_the_release_mr_over_gitlab_loopback() -> Result<(), BoxError> {
    let (fx, branch) = shape_post_merge_gitlab_repo("pr-live-gitlab")?;
    // The scripted exchange, in the §11.1 order over the GitLab shapes:
    // the commits-API chain (the file-existence probe → the create
    // action commits once), then the machine's find, the upsert's own
    // lookup and the MR create.
    let commit_sha = "4".repeat(40);
    let (forge, api, server) = FakeForge::new(vec![
        (404, "{\"message\":\"404 Not Found\"}".to_owned()),
        (
            201,
            format!(
                "{{\"id\": \"{commit_sha}\", \"short_id\": \"abc1234\", \"title\": \"chore: release v0.1.1\"}}"
            ),
        ),
        (200, "[]".to_owned()),
        (200, "[]".to_owned()),
        (201, mr_body(9, &branch)),
    ])?;
    let (code, out) = halyard_with_env(
        fx.root(),
        &["pr", "--format", "json"],
        &[
            ("GITLAB_API_URL", &api),
            ("GITLAB_TOKEN", "glpat-smoke"),
            ("GITHUB_TOKEN", ""),
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
        Some("octo/halyard-demo"),
        "the GitLab lane reports the project path as the repo coordinates: {out}"
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
        "the Release MR opened over the GitLab adapter: {out}"
    );
    assert!(
        kinds.iter().any(|kind| kind == "content_committed"),
        "the content commit rode the commits API (§11.5 commit_mode = api): {out}"
    );
    let received = forge.received();
    assert_eq!(
        received.len(),
        5,
        "probe + commit + find + upsert-lookup + create: {received:?}"
    );
    // Every exchange carried the documented auth and the URL-encoded
    // project path (research §1/§2).
    for request in &received {
        assert_eq!(
            field(request, "private_token"),
            "glpat-smoke",
            "the PRIVATE-TOKEN auth rides every call: {request}"
        );
    }
    let first = received.first().expect("the file probe");
    assert!(
        field(first, "path").starts_with("/projects/octo%2Fhalyard-demo/repository/files/"),
        "the project path is URL-encoded and the file probe rides the commits-API          precondition: {}",
        field(first, "path")
    );
    let commit = received.get(1).expect("the commits create");
    assert!(
        field(commit, "path").ends_with("/repository/commits"),
        "{}",
        field(commit, "path")
    );
    let create = received.last().expect("the MR create");
    assert_eq!(field(create, "method"), "POST");
    assert!(
        field(create, "path").ends_with("/merge_requests"),
        "the MR create posts to the project's merge_requests: {}",
        field(create, "path")
    );
    Ok(())
}

/// Shapes the post-merge GitLab fixture the loopback e2e runs: v0.1.0
/// tagged, the release commit v0.1.1 tagged, one fresh `feat` commit
/// after it (the plan proposes 0.1.2), `commit_mode = "api"` and the
/// GitLab-shaped origin. Returns the fixture and the forecast release
/// branch.
fn shape_post_merge_gitlab_repo(label: &str) -> Result<(GitFixture, String), BoxError> {
    let mut fx = build_workspace(label)?;
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
    fx.write("halyard.toml", "[forge]\ncommit_mode = \"api\"\n")?;
    fx.run_git(&[
        "remote",
        "add",
        "origin",
        "https://gitlab.com/octo/halyard-demo.git",
    ])?;
    gitlab_probe_offline_origin(&mut fx)?;
    let branch = forecast_release_branch(&fx)?;
    Ok((fx, branch))
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

/// A Forgejo-shaped pull body the M7 step-2 adapter's upsert parses
/// (research §2: `number`, `title`, `body`, `html_url`, `head.ref`).
fn forgejo_pull_body(number: u64, branch: &str) -> String {
    serde_json::json!({
        "number": number,
        "title": "chore: release v0.1.1",
        "body": "## 🚀 Release plan",
        "state": "open",
        "html_url": format!("https://codeberg.org/octo/halyard-demo/pulls/{number}"),
        "head": { "ref": branch },
    })
    .to_string()
}

/// The Forgejo lane's variant of the offline probe rewrite: the same
/// `insteadOf` rewrite over the codeberg.org-shaped origin URL (the
/// adapter's remote grammar still parses the configured URL, and
/// `ls-remote`/`fetch` stay on the disk).
fn forgejo_probe_offline_origin(fx: &mut GitFixture) -> Result<(), BoxError> {
    offline_origin_rewrite(fx, "https://codeberg.org/octo/halyard-demo.git")
}

/// Live `pr` over the Forgejo/Gitea lane (M7 step 2, ADR-0050): the
/// same machine pass the GitHub and GitLab loopback tests pin, over a
/// codeberg.org-shaped origin — the fake forge saw the
/// `Authorization: token` auth and the pull find + create, and the
/// report names the opened pull. The content rides the git half
/// (`commit_mode = "git"` — the lane's documented mode: the surface has
/// no atomic multi-file commit, research §5), so the forge sees only
/// the PR half. `FORGEJO_API_URL` keeps the exchange on the loopback;
/// `insteadOf` keeps the git-side probe offline.
/// Shapes the post-merge Forgejo fixture the loopback e2e runs: v0.1.0
/// tagged, the release commit v0.1.1 tagged, one fresh `feat` commit
/// after it (the plan proposes 0.1.2), `commit_mode = "git"` (the
/// lane's documented mode — ADR-0050) and the codeberg.org-shaped
/// origin. Returns the fixture and the forecast release branch.
fn shape_post_merge_forgejo_repo(label: &str) -> Result<(GitFixture, String), BoxError> {
    let mut fx = build_workspace(label)?;
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
    fx.write("halyard.toml", "[forge]\ncommit_mode = \"git\"\n")?;
    fx.run_git(&[
        "remote",
        "add",
        "origin",
        "https://codeberg.org/octo/halyard-demo.git",
    ])?;
    forgejo_probe_offline_origin(&mut fx)?;
    let branch = forecast_release_branch(&fx)?;
    Ok((fx, branch))
}

#[test]
fn pr_live_opens_the_release_pull_over_forgejo_loopback() -> Result<(), BoxError> {
    let (fx, branch) = shape_post_merge_forgejo_repo("pr-live-forgejo")?;
    // The scripted exchange, in the §11.1 order over the Forgejo
    // shapes: the machine's find, the upsert's own lookup and the
    // create — the content rode the git-side CAS push, so the forge
    // sees only the PR half.
    let (forge, api, server) = FakeForge::new(vec![
        (200, "[]".to_owned()),
        (200, "[]".to_owned()),
        (201, forgejo_pull_body(9, &branch)),
    ])?;
    let (code, out) = halyard_with_env(
        fx.root(),
        &["pr", "--format", "json"],
        &[
            ("FORGEJO_API_URL", &api),
            ("FORGEJO_TOKEN", "forgejo-smoke"),
            ("GITHUB_TOKEN", ""),
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
        Some("octo/halyard-demo"),
        "the Forgejo lane reports the owner/repo path as the repo coordinates: {out}"
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
        "the Release pull opened over the Forgejo adapter: {out}"
    );
    assert_eq!(
        forge.received().len(),
        3,
        "find + upsert-lookup + create — the content rode git: {received:?}",
        received = forge.received()
    );
    // Every exchange carried the documented auth (research §1: the
    // token is prepended with `token` followed by a space).
    for request in forge.received() {
        assert_eq!(
            field(&request, "authorization"),
            "token forgejo-smoke",
            "the Authorization token auth rides every call: {request}"
        );
    }
    let received = forge.received();
    let create = received.last().expect("the pull create");
    assert_eq!(field(create, "method"), "POST");
    assert!(
        field(create, "path").ends_with("/repos/octo/halyard-demo/pulls"),
        "the create posts to the repository's pulls: {}",
        field(create, "path")
    );
    let create_body: serde_json::Value = serde_json::from_str(&field(create, "body"))?;
    // The §11.2 inverse mapping: the pull's base is the release branch's
    // own base segment (whatever the planner resolved — the fixture
    // repos' recorded no-base-tag quirk resolves `HEAD`).
    let expected_base = branch
        .strip_prefix("halyard/release/")
        .filter(|base| !base.is_empty())
        .ok_or("the forecast branch is the §11.2 shape")?;
    assert_eq!(
        create_body.get("base").and_then(serde_json::Value::as_str),
        Some(expected_base),
        "the §11.2 inverse mapping derives the pull's base from the release          branch the planner resolved (never a guess)"
    );
    assert_eq!(
        create_body.get("head").and_then(serde_json::Value::as_str),
        Some(branch.as_str()),
        "the pull rides the release branch"
    );
    // The remote release branch carries the content commit: its tree is
    // the content tree the plan's edits produce (the lane's git half).
    assert_release_branch_carries_the_content(&fx, &branch)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// The §11.1 step-3 content half on the live §12 `auto` mode (the plan's
// acceptance criterion, docs/HALYARD_ARCHITECTURE.md §11.1: "On every
// push to the base branch the same command (`halyard release --auto`)
// runs" — step 3: "apply EditSet in memory → build tree → compare tree
// OID with remote release branch tip (`ls-remote`, 0 REST); if equal:
// update PR body only if plan_hash differs; else commit & `push
// --force-with-lease`"). The auto pass now runs the same content half
// the `pr` executor's loopback e2e pins: the git-side CAS push lands
// the plan's edits on the release branch before the machine's PR half.
// Written first (red: the auto pass never pushed content), implemented
// second.
// ---------------------------------------------------------------------------

#[test]
fn auto_live_pushes_the_release_branch_content_over_forgejo_loopback() -> Result<(), BoxError> {
    let (fx, branch) = shape_post_merge_forgejo_repo("auto-live-forgejo")?;
    // The scripted exchange, in the §11.1 order over the Forgejo
    // shapes: the machine's find, the upsert's own lookup and the
    // create — the content rode the git-side CAS push (the pass's own
    // git half, §11.1 step 3 under `commit_mode = "git"`), so the forge
    // sees only the PR half.
    let (forge, api, server) = FakeForge::new(vec![
        (200, "[]".to_owned()),
        (200, "[]".to_owned()),
        (201, forgejo_pull_body(9, &branch)),
    ])?;
    let (code, out) = halyard_with_env(
        fx.root(),
        &["action", "--mode", "auto", "--format", "json"],
        &[
            ("FORGEJO_API_URL", &api),
            ("FORGEJO_TOKEN", "forgejo-smoke"),
            ("GITHUB_TOKEN", ""),
        ],
    )?;
    server.join().expect("the fake forge served the pass");
    assert_eq!(code, 0, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    // The §12 auto document: the pass executed (dry=false), the prepare
    // phase owned it (propagated=false — nothing pending) and the PR
    // keys ride the outputs map.
    assert_eq!(
        value.get("mode").and_then(serde_json::Value::as_str),
        Some("auto")
    );
    assert_eq!(value.get("dry"), Some(&serde_json::json!(false)));
    assert_eq!(value.get("propagated"), Some(&serde_json::json!(false)));
    let outputs = value
        .get("outputs")
        .and_then(serde_json::Value::as_object)
        .ok_or("the document carries the outputs map")?;
    assert!(
        outputs.get("pr_number").is_some(),
        "the Release pull opened over the Forgejo adapter: {out}"
    );
    // The content half's verdict: the pass pushed the plan's edits
    // before the machine ran, so the machine sees the branch already
    // carrying the content tree (I1 — no content call spent). The
    // content block rides the JSON document (the §12 observability
    // surface), naming the tree the branch carries.
    let content = value
        .get("content")
        .ok_or("the JSON carries the content block")?;
    assert_eq!(
        content.get("type").and_then(serde_json::Value::as_str),
        Some("content_matched"),
        "{out}"
    );
    assert_eq!(
        forge.received().len(),
        3,
        "find + upsert-lookup + create — the content rode git: {received:?}",
        received = forge.received()
    );
    // The remote release branch carries the content commit: its tree is
    // the content tree the plan's edits produce (the §11.1 step-3 truth
    // pin, now on the auto command).
    assert_release_branch_carries_the_content(&fx, &branch)?;
    Ok(())
}

/// The Bitbucket lane's variant of the offline probe rewrite (the
/// same `insteadOf` rewrite over the bitbucket.org-shaped origin URL).
fn bitbucket_probe_offline_origin(fx: &mut GitFixture) -> Result<(), BoxError> {
    offline_origin_rewrite(fx, "https://bitbucket.org/octo/halyard-demo.git")
}

/// A Bitbucket-shaped pull body the M7 step-3 adapter's upsert parses
/// (research §2: `id`, `title`, `description`, `links.html.href`,
/// `source.branch.name`).
fn bitbucket_pull_body(number: u64, branch: &str) -> String {
    serde_json::json!({
        "id": number,
        "title": "chore: release v0.1.2",
        "description": "## 🚀 Release plan",
        "state": "open",
        "links": { "html": { "href": format!("https://bitbucket.org/octo/halyard-demo/pullrequests/{number}") } },
        "source": { "branch": { "name": branch } },
    })
    .to_string()
}

/// Live `pr` over the Bitbucket lane (M7 step 3, ADR-0051): the same
/// machine pass the other lanes' loopback tests pin, over a
/// bitbucket.org-shaped origin — the fake forge saw the
/// `Authorization: Bearer` auth and the pull find + create, and the
/// report names the opened pull. The content rides the git half
/// (`commit_mode = "git"` — the lane's documented mode: the commits
/// resource does not support new commit creation, research §5), so the
/// forge sees only the PR half. `BITBUCKET_API_URL` keeps the exchange
/// on the loopback; `insteadOf` keeps the git-side probe offline.
#[test]
fn pr_live_opens_the_release_pull_over_bitbucket_loopback() -> Result<(), BoxError> {
    let mut fx = build_workspace("pr-live-bitbucket")?;
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
    fx.write("halyard.toml", "[forge]\ncommit_mode = \"git\"\n")?;
    fx.run_git(&[
        "remote",
        "add",
        "origin",
        "https://bitbucket.org/octo/halyard-demo.git",
    ])?;
    bitbucket_probe_offline_origin(&mut fx)?;
    let branch = forecast_release_branch(&fx)?;
    let (forge, api, server) = FakeForge::new(vec![
        (200, "[]".to_owned()),
        (200, "[]".to_owned()),
        (201, bitbucket_pull_body(9, &branch)),
    ])?;
    let (code, out) = halyard_with_env(
        fx.root(),
        &["pr", "--format", "json"],
        &[
            ("BITBUCKET_API_URL", &api),
            ("BITBUCKET_TOKEN", "bitbucket-smoke"),
            ("GITHUB_TOKEN", ""),
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
        Some("octo/halyard-demo"),
        "the Bitbucket lane reports the workspace/slug as the repo coordinates: {out}"
    );
    assert_eq!(
        forge.received().len(),
        3,
        "find + upsert-lookup + create — the content rode git: {received:?}",
        received = forge.received()
    );
    for request in forge.received() {
        assert_eq!(
            field(&request, "authorization"),
            "Bearer bitbucket-smoke",
            "the OAuth 2.0 Bearer auth rides every call (research §1): {request}"
        );
    }
    let received = forge.received();
    let create = received.last().expect("the pull create");
    assert_eq!(field(create, "method"), "POST");
    assert!(
        field(create, "path").ends_with("/repositories/octo/halyard-demo/pullrequests"),
        "the create posts to the repository's pullrequests: {}",
        field(create, "path")
    );
    let create_body: serde_json::Value = serde_json::from_str(&field(create, "body"))?;
    let expected_base = branch
        .strip_prefix("halyard/release/")
        .filter(|base| !base.is_empty())
        .ok_or("the forecast branch is the §11.2 shape")?;
    assert_eq!(
        create_body
            .get("destination")
            .and_then(|d| d.get("branch"))
            .and_then(|b| b.get("name"))
            .and_then(serde_json::Value::as_str),
        Some(expected_base),
        "the §11.2 inverse mapping derives the pull's destination from the release branch"
    );
    assert_eq!(
        create_body
            .get("source")
            .and_then(|s| s.get("branch"))
            .and_then(|b| b.get("name"))
            .and_then(serde_json::Value::as_str),
        Some(branch.as_str()),
        "the pull rides the release branch"
    );
    assert_release_branch_carries_the_content(&fx, &branch)?;
    Ok(())
}
