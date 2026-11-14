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
    // 0.1.2, the prepare phase opens the Release PR.
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
    fx.run_git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/octo/halyard-demo.git",
    ])?;
    // The forecast names the exact branch the live run will target.
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
    let base = branch
        .strip_prefix("halyard/release/")
        .filter(|base| !base.is_empty())
        .ok_or("the branch is the §11.2 shape")?
        .to_owned();
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
    assert_eq!(effects.len(), 1, "{out}");
    let effect = effects.first().expect("one effect");
    assert_eq!(
        effect.get("type").and_then(serde_json::Value::as_str),
        Some("pr_opened")
    );
    assert_eq!(
        effect.get("number").and_then(serde_json::Value::as_i64),
        Some(9)
    );
    let received = forge.received();
    assert_eq!(received.len(), 3, "machine find + upsert find + create");
    let create = received.get(2).expect("three captures");
    assert_eq!(field(create, "method"), "POST");
    assert_eq!(field(create, "path"), "/repos/octo/halyard-demo/pulls");
    let payload: serde_json::Value = serde_json::from_str(&field(create, "body"))
        .unwrap_or_else(|error| unreachable!("the create carries a body: {error}"));
    assert_eq!(field(&payload, "head"), branch);
    assert_eq!(
        field(&payload, "base"),
        base,
        "the base is the §11.2 prefix inverse — no extra probe spent"
    );
    assert_eq!(field(&payload, "title"), "chore: release v0.1.2");
    Ok(())
}

/// `forge-smoke` over the GitHub App env (M4.1c, ADR-0014): the shell
/// resolves the App credentials (PEM → JWT → installation token), the
/// mint rides `POST /app/installations/{id}/access_tokens` with the
/// JWT as the bearer, and the probe carries the minted installation
/// token — the full e2e shape over the real transport and loopback
/// forge.
#[test]
fn forge_smoke_authenticates_app_over_loopback() -> Result<(), BoxError> {
    let fx = build_workspace("smoke-app")?;
    let (forge, base, server) = FakeForge::new(vec![
        (
            201,
            "{\"token\":\"installation-token-1\",\"expires_at\":\"2030-01-01T00:00:00Z\"}"
                .to_owned(),
        ),
        (404, "{\"message\":\"Not Found\"}".to_owned()),
    ])?;
    let key_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../halyard-forge-github/tests/fixtures/app_key_pkcs1.pem"
    );
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
            ("GITHUB_TOKEN", ""),
            ("HALYARD_FORGE_TOKEN", ""),
            ("HALYARD_APP_ID", "12345"),
            ("HALYARD_APP_INSTALLATION_ID", "42"),
            ("HALYARD_APP_PRIVATE_KEY_PATH", key_path),
        ],
    )?;
    server
        .join()
        .expect("the fake forge served the mint + probe");
    assert_eq!(code, 0, "{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim())?;
    assert_eq!(
        value.get("auth_mode").and_then(serde_json::Value::as_str),
        Some("app"),
        "the report names the credential mode: {out}"
    );
    assert_eq!(
        value
            .get("release_exists")
            .and_then(serde_json::Value::as_bool),
        Some(false),
        "the probe rides the minted installation token"
    );
    let received = forge.received();
    assert_eq!(received.len(), 2, "mint + probe: {out}");
    let mint = received.first().expect("one capture");
    assert_eq!(field(mint, "method"), "POST");
    assert_eq!(
        field(mint, "path"),
        "/app/installations/42/access_tokens",
        "the mint posts to the documented endpoint"
    );
    let authorization = field(mint, "authorization");
    let jwt = authorization
        .strip_prefix("Bearer ")
        .ok_or("the mint rides Authorization: Bearer <jwt>")?
        .to_owned();
    assert_eq!(
        jwt.split('.').count(),
        3,
        "the bearer is a three-segment JWT: {authorization:.40}…"
    );
    let probe = received.get(1).expect("two captures");
    assert_eq!(
        field(probe, "path"),
        "/repos/octo/halyard-demo/releases/tags/halyard-forge-smoke-probe"
    );
    assert_eq!(
        field(probe, "authorization"),
        "Bearer installation-token-1",
        "the repo call carries the minted installation token"
    );
    Ok(())
}

/// A partial `HALYARD_APP_*` set commits the run to App mode and fails
/// closed naming what is missing — never a guessed fallback to the
/// static token (ADR-0014).
#[test]
fn forge_smoke_fails_closed_on_a_partial_app_env() -> Result<(), BoxError> {
    let fx = build_workspace("smoke-app-partial")?;
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
            ("HALYARD_FORGE_TOKEN", ""),
            ("HALYARD_APP_ID", "12345"),
        ],
    )?;
    assert_ne!(code, 0, "a partial App env is a fail-closed error: {out}");
    assert!(out.contains("HALYARD_APP_INSTALLATION_ID"), "{out}");
    assert!(out.contains("HALYARD_APP_PRIVATE_KEY"), "{out}");
    Ok(())
}
