//! CLI `publish` smoke tests (M5.2 step 2): builds a real git fixture
//! workspace, runs the compiled binary with `publish` in dry and live
//! modes. The tool spawn is exercised through a PATH shim that records
//! its argv (the real `cargo`/`pnpm` never runs; the network is never
//! touched — the dry forecast skips the resume probes, I5, and the
//! live-mode tests here only cover the offline paths: zero targets and
//! the fail-closed routing gap).

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use halyard_test_support::{CommitSpec, GitFixture};

type BoxError = Box<dyn std::error::Error>;

fn fixture_dir(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir = std::env::temp_dir().join(format!(
        "halyard-cli-publish-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// A two-crate workspace plus a feat commit, so the plan proposes
/// releases; `publish_config` is written as `halyard.toml` at the root.
fn build_workspace(label: &str, publish_config: &str) -> Result<GitFixture, BoxError> {
    let dir = fixture_dir(label)?;
    let mut fx = GitFixture::init_in(&dir)?;
    let scaffold = vec![
        (
            "Cargo.toml".to_owned(),
            "[workspace]\nresolver = \"2\"\nmembers = [\"crates/p1\", \"crates/p2\"]\n".to_owned(),
        ),
        (
            "halyard.toml".to_owned(),
            publish_config.to_owned(),
        ),
        (
            "crates/p1/Cargo.toml".to_owned(),
            "[package]\nname = \"p1\"\nversion = \"0.1.0\"\nedition = \"2021\"\n".to_owned(),
        ),
        (
            "crates/p2/Cargo.toml".to_owned(),
            "[package]\nname = \"p2\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\np1 = { path = \"../p1\", version = \"0.1\" }\n".to_owned(),
        ),
        ("crates/p1/src/lib.rs".to_owned(), "pub const C: usize = 1;\n".to_owned()),
        ("crates/p2/src/lib.rs".to_owned(), "pub const C: usize = 2;\n".to_owned()),
    ];
    fx.commit_with(&CommitSpec::files(scaffold, "chore: scaffold workspace"))?;
    let feat = vec![
        (
            "crates/p1/src/lib.rs".to_owned(),
            "pub const C: usize = 2;\n".to_owned(),
        ),
        (
            "crates/p2/src/lib.rs".to_owned(),
            "pub const C: usize = 3;\n".to_owned(),
        ),
    ];
    fx.commit_with(&CommitSpec::files(feat, "feat: bump the constants"))?;
    Ok(fx)
}

fn halyard_env(
    root: &std::path::Path,
    args: &[&str],
    path_prepend: Option<&std::path::Path>,
    shim_log: Option<&std::path::Path>,
) -> Result<(std::process::Output, String), BoxError> {
    halyard_env_with(root, args, path_prepend, shim_log, &[])
}

fn halyard(root: &std::path::Path, args: &[&str]) -> Result<(bool, String), BoxError> {
    let (out, text) = halyard_env(root, args, None, None)?;
    Ok((out.status.success(), text))
}

fn native_path(path: &str) -> String {
    path.replace('/', std::path::MAIN_SEPARATOR_STR)
}

/// `halyard_env` plus extra environment pairs for the child (the auth
/// tests set the documented token variables without touching the test
/// process's own environment).
fn halyard_env_with(
    root: &std::path::Path,
    args: &[&str],
    path_prepend: Option<&std::path::Path>,
    shim_log: Option<&std::path::Path>,
    vars: &[(&str, &str)],
) -> Result<(std::process::Output, String), BoxError> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_halyard"));
    command.arg("-C").arg(root).args(args).env("NO_COLOR", "1");
    if let (Some(shim_dir), Some(log)) = (path_prepend, shim_log) {
        let original = std::env::var("PATH").unwrap_or_default();
        let path = if original.is_empty() {
            shim_dir.display().to_string()
        } else {
            format!("{}:{original}", shim_dir.display())
        };
        command.env("PATH", path);
        command.env("HALYARD_SHIM_LOG", log);
    }
    for (key, value) in vars {
        command.env(key, value);
    }
    let out = command.output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Ok((out, text))
}

fn publish_enabled_config() -> &'static str {
    "[publish]\nenabled = true\n"
}

/// A `cargo` stand-in that records its argv and exits 0 (the packaged
/// tool never runs; the argv is the assertion).
fn write_cargo_shim(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir = fixture_dir(label)?;
    let script = dir.join("cargo");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$HALYARD_SHIM_LOG\"\nexit 0\n",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms)?;
    }
    Ok(dir)
}

#[test]
fn publish_dry_forecasts_over_the_shimmed_tool() -> Result<(), BoxError> {
    let fx = build_workspace("dry", publish_enabled_config())?;
    let shim = write_cargo_shim("shim-dry")?;
    let log = fixture_dir("shim-log-dry")?.join("argv.log");
    let (out, text) = halyard_env(
        fx.root(),
        &["publish", "--dry", "--format", "json"],
        Some(&shim),
        Some(&log),
    )?;
    assert!(out.status.success(), "publish --dry failed: {text}");

    // The forecast reports both targets with `forecast` outcomes and a
    // zero publish tally.
    assert!(text.contains("\"forecast\""), "rows say forecast: {text}");
    assert!(text.contains("crates/p1"), "targets named: {text}");
    assert!(text.contains("crates/p2"), "targets named: {text}");

    // The shim captured the documented dry argv per target.
    let recorded = std::fs::read_to_string(&log)?;
    assert!(recorded.contains("--dry-run"), "dry argv: {recorded}");
    assert!(recorded.contains("--offline"), "hermetic argv: {recorded}");
    assert!(
        recorded.contains(&native_path("crates/p1/Cargo.toml")),
        "manifest scoped: {recorded}"
    );
    assert!(
        recorded.contains(&native_path("crates/p2/Cargo.toml")),
        "manifest scoped: {recorded}"
    );
    Ok(())
}

#[test]
fn publish_dry_json_is_byte_stable_across_runs() -> Result<(), BoxError> {
    let fx = build_workspace("stable", publish_enabled_config())?;
    let shim = write_cargo_shim("shim-stable")?;
    let log = fixture_dir("shim-log-stable")?.join("argv.log");
    let (first, text1) = halyard_env(
        fx.root(),
        &["publish", "--dry", "--format", "json"],
        Some(&shim),
        Some(&log),
    )?;
    let (second, text2) = halyard_env(
        fx.root(),
        &["publish", "--dry", "--format", "json"],
        Some(&shim),
        Some(&log),
    )?;
    assert!(first.status.success() && second.status.success());
    assert_eq!(text1, text2, "I3: identical inputs ⇒ byte-identical JSON");
    Ok(())
}

#[test]
fn publish_on_a_publish_disabled_fixture_reports_nothing_to_publish() -> Result<(), BoxError> {
    // The default config keeps publishing off: zero targets, zero
    // probes — the offline-safe live-mode path.
    let fx = build_workspace("disabled", "")?;
    let (ok, out) = halyard(fx.root(), &["publish", "--format", "json"])?;
    assert!(ok, "publish (no targets) failed: {out}");
    assert!(out.contains("\"targets\": 0"), "report shape: {out}");
    assert!(out.contains("\"rows\": []"), "no rows: {out}");
    Ok(())
}

#[test]
fn publish_live_fails_closed_on_an_unroutable_registry() -> Result<(), BoxError> {
    let config = "[publish]\nenabled = true\n\n[publish.cargo]\nregistry = \"private-index\"\n";
    let fx = build_workspace("unroutable", config)?;
    let (out, text) = halyard_env(fx.root(), &["publish", "--format", "json"], None, None)?;
    assert!(
        !out.status.success(),
        "an unroutable registry must fail: {text}"
    );
    assert!(
        text.contains("E-PUB-REGISTRY"),
        "the stable code is reported: {text}"
    );
    assert!(
        text.contains("private-index"),
        "the registry is named: {text}"
    );
    Ok(())
}

#[test]
fn publish_dry_gated_rows_follow_a_broken_tool() -> Result<(), BoxError> {
    // A shim that fails: the row is `failed` and the dependent gate
    // shows in the forecast — p2 depends on p1, so p2 is skipped.
    let fx = build_workspace("broken", publish_enabled_config())?;
    let shim = write_cargo_shim("shim-broken")?;
    // Overwrite the shim with a failing one.
    let script = shim.join("cargo");
    std::fs::write(&script, "#!/bin/sh\nexit 101\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms)?;
    }
    let (out, text) = halyard_env(
        fx.root(),
        &["publish", "--dry", "--format", "json"],
        Some(&shim),
        None,
    )?;
    assert!(
        !out.status.success(),
        "failed rows fail the command: {text}"
    );
    assert!(text.contains("E-PUB-EFFECT"), "row code surfaces: {text}");
    assert!(text.contains("upstream_failed"), "the gate shows: {text}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Publish auth (M5.3 step 1, ADR-0019): the resolved token reaches the
// spawn only as an environment pair, a present-but-empty token fails
// closed before any spawn (E-PUB-AUTH), and tool output is scrubbed —
// a token a misconfigured tool echoes must never reach a report row.
// ---------------------------------------------------------------------------

/// The token value the auth fixtures share (a parent-env var the child
/// also sees — inheritance makes injection and inheritance
/// indistinguishable from outside; what these tests pin is the
/// fail-closed and the scrubbing contracts, the seam's observable
/// surface).
const TOKEN: &str = "hunter2-secret-token-value";
const OIDC_REQUEST_TOKEN: &str = "oidc-request-token-secret-value";

#[test]
fn publish_failure_message_scrubs_the_resolved_token() -> Result<(), BoxError> {
    let fx = build_workspace("scrub", publish_enabled_config())?;
    let shim = write_cargo_shim("shim-scrub")?;
    let log = fixture_dir("shim-log-scrub")?.join("argv.log");
    // A failing tool that echoes the token it was handed (the
    // misconfigured-tool leak the scrubbing exists for).
    let script = shim.join("cargo");
    std::fs::write(
        &script,
        format!("#!/bin/sh\necho \"upload rejected: token {TOKEN} invalid\" >&2\nexit 101\n"),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms)?;
    }
    let (out, text) = halyard_env_with(
        fx.root(),
        &["publish", "--dry", "--format", "json"],
        Some(&shim),
        Some(&log),
        &[("CARGO_REGISTRY_TOKEN", TOKEN)],
    )?;
    assert!(
        !out.status.success(),
        "failed rows fail the command: {text}"
    );
    assert!(text.contains("E-PUB-EFFECT"), "row code surfaces: {text}");
    assert!(
        !text.contains(TOKEN),
        "the token must never reach a report row: {text}"
    );
    assert!(
        text.contains("[secret]"),
        "the scrubbed marker replaces the value: {text}"
    );
    Ok(())
}

#[test]
fn publish_refuses_a_present_but_empty_token_before_any_spawn() -> Result<(), BoxError> {
    let fx = build_workspace("empty-token", publish_enabled_config())?;
    let shim = write_cargo_shim("shim-empty-token")?;
    let log = fixture_dir("shim-log-empty-token")?.join("argv.log");
    let (out, text) = halyard_env_with(
        fx.root(),
        &["publish", "--dry", "--format", "json"],
        None,
        None,
        &[("CARGO_REGISTRY_TOKEN", "")],
    )?;
    // The capture-level misconfiguration is refused with the stable
    // code, naming the variable — before any tool spawn (the shim's
    // argv log must never appear).
    assert!(!out.status.success(), "an empty token must fail: {text}");
    assert!(text.contains("E-PUB-AUTH"), "the stable code: {text}");
    assert!(
        text.contains("CARGO_REGISTRY_TOKEN"),
        "the variable is named: {text}"
    );
    assert!(!log.exists(), "no spawn may happen after the refusal");
    let _ = shim;
    Ok(())
}

// ---------------------------------------------------------------------------
// Auth provenance observability (M5.3 step 2): the dry forecast's JSON
// carries which documented variable authenticates each registry —
// provenance only, never values — byte-stable across runs (I3), and
// `doctor` reports the same resolution before any run.
// ---------------------------------------------------------------------------

#[test]
fn publish_dry_json_carries_the_auth_provenance() -> Result<(), BoxError> {
    let fx = build_workspace("auth-json", publish_enabled_config())?;
    let shim = write_cargo_shim("shim-auth-json")?;
    let log = fixture_dir("shim-log-auth-json")?.join("argv.log");
    let args = ["publish", "--dry", "--format", "json"];
    let (first, text1) = halyard_env_with(
        fx.root(),
        &args,
        Some(&shim),
        Some(&log),
        &[("CARGO_REGISTRY_TOKEN", TOKEN)],
    )?;
    let (second, text2) = halyard_env_with(
        fx.root(),
        &args,
        Some(&shim),
        Some(&log),
        &[("CARGO_REGISTRY_TOKEN", TOKEN)],
    )?;
    assert!(first.status.success() && second.status.success(), "{text1}");
    // The provenance rides the JSON document, name only.
    assert!(
        text1.contains("\"auth\""),
        "the auth block is present: {text1}"
    );
    assert!(
        text1.contains("env:CARGO_REGISTRY_TOKEN"),
        "the resolved variable is named: {text1}"
    );
    assert!(
        !text1.contains(TOKEN),
        "no value may ride the JSON: {text1}"
    );
    // I3: identical inputs ⇒ byte-identical JSON.
    assert_eq!(text1, text2, "the auth block is deterministic");
    Ok(())
}

#[test]
fn doctor_reports_the_publish_auth_row() -> Result<(), BoxError> {
    let fx = build_workspace("doctor-auth", publish_enabled_config())?;
    let (out, text) = halyard_env_with(
        fx.root(),
        &["doctor", "--format", "json"],
        None,
        None,
        &[("CARGO_REGISTRY_TOKEN", TOKEN)],
    )?;
    assert!(out.status.success(), "doctor failed: {text}");
    assert!(text.contains("publish-auth"), "the check row: {text}");
    assert!(
        text.contains("CARGO_REGISTRY_TOKEN"),
        "the documented variable is named: {text}"
    );
    assert!(
        !text.contains(TOKEN),
        "the token value never reaches doctor: {text}"
    );
    Ok(())
}

/// A configured non-default cargo registry fail-closes at the shell's
/// routing map when the registry is **not configured anywhere**: the
/// routing map owns the two §13 clients (crates.io, npm), and a name
/// neither of them covers nor a cargo config file resolves is refused
/// with `E-PUB-REGISTRY` naming the registry **before any spawn**, dry
/// mode included — a forecast must never show a spawn the live run
/// would refuse. The configured side of this contract is pinned below
/// (`publish_dry_forecasts_a_configured_custom_registry` &
/// `publish_live_probes_a_custom_registry_over_loopback`): a
/// `[registries.<name>]` entry (or its env override) resolves the
/// registry's sparse index and joins the routing map (ADR-0021).
#[test]
fn publish_refuses_a_custom_registry_before_any_spawn() -> Result<(), BoxError> {
    let config = "[publish]\nenabled = true\n\n[publish.cargo]\nregistry = \"corp.repo\"\n";
    let fx = build_workspace("registry", config)?;
    let shim = write_cargo_shim("shim-registry")?;
    let log = fixture_dir("shim-log-registry")?.join("argv.log");
    let (out, text) = halyard_env(
        fx.root(),
        &["publish", "--dry", "--format", "json"],
        Some(&shim),
        Some(&log),
    )?;
    assert!(
        !out.status.success(),
        "an unroutable registry is refused, not half-run: {text}"
    );
    assert!(text.contains("E-PUB-REGISTRY"), "the stable code: {text}");
    assert!(
        text.contains("corp.repo"),
        "the refusal names the registry: {text}"
    );
    assert!(
        !std::fs::exists(&log).unwrap_or(false) || std::fs::read_to_string(&log)?.trim().is_empty(),
        "zero side effects — the tool never spawned: {text}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Custom cargo registry routing (ADR-0021): a `publish.registry` name
// other than the default resolves through the cargo configuration — the
// `[registries.<name>]` `index` key (Cargo Book "Registries", VERIFIED)
// or its `CARGO_REGISTRIES_<NAME>_INDEX` env override — and a
// sparse-HTTP index joins the routing map as a probe client. An
// unresolvable name still refuses (the pin above); a resolvable name
// routes, offline-verifiable end to end over a loopback sparse index.
// ---------------------------------------------------------------------------

/// A stateful loopback registry: the **first** GET per path answers
/// `404` (the version is not published yet), every later GET answers
/// `200` with the version's document — the §13 resume/visibility
/// sequence in one deterministic server (the fake-forge loopback
/// discipline: hand-rolled HTTP, no new dependencies). The body style
/// follows the protocol the routed client speaks: the cargo sparse
/// index's newline-delimited file or the npm packument document. The
/// serving thread lives until the test process ends; the served
/// request count and paths are read from the shared `Arc` after the
/// run.
struct FakeIndex {
    versions: BTreeMap<String, String>,
    packument: bool,
    served: std::sync::atomic::AtomicUsize,
    seen: Mutex<BTreeSet<String>>,
}

impl FakeIndex {
    /// Starts the server; returns the shared fake and the base URL the
    /// configuration must carry.
    fn start(
        versions: BTreeMap<String, String>,
        base_path: &'static str,
        packument: bool,
    ) -> Result<(Arc<Self>, String), BoxError> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let index = Arc::new(Self {
            versions,
            packument,
            served: std::sync::atomic::AtomicUsize::new(0),
            seen: Mutex::new(BTreeSet::new()),
        });
        let server = Arc::clone(&index);
        thread::spawn(move || {
            while let Ok((mut stream, _addr)) = listener.accept() {
                if let Some(path) = server.serve_one(&mut stream) {
                    server
                        .seen
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(path);
                }
                server
                    .served
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        });
        Ok((index, format!("http://127.0.0.1:{port}{base_path}")))
    }

    /// How many requests the fake answered so far.
    fn served(&self) -> usize {
        self.served.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// The served paths, sorted.
    fn served_paths(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }

    /// Reads one HTTP exchange and answers it: `404` on the first hit
    /// of the path (not yet published), `200` + the index line after.
    /// Returns the served path.
    fn serve_one(&self, stream: &mut TcpStream) -> Option<String> {
        let mut buf = Vec::new();
        let mut chunk = [0_u8; 4096];
        let header_end;
        loop {
            let read = stream.read(&mut chunk).ok()?;
            if read == 0 {
                return None;
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
        let request_line = head.lines().next().unwrap_or_default();
        let path = request_line.split(' ').nth(1)?.to_owned();
        // The sparse index layout's last path segment is the package
        // name (`/index/2/p1`); the version turns visible after the
        // first probe miss (the §13 sequence).
        let first_hit = self
            .seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path.clone());
        let name = path.rsplit('/').next().unwrap_or_default().to_owned();
        let version = self.versions.get(&name);
        let (status, body) = match (first_hit, version) {
            (true, _) | (false, None) => ("404 Not Found".to_owned(), String::new()),
            (false, Some(version)) if self.packument => (
                "200 OK".to_owned(),
                format!("{{\"versions\":{{\"{version}\":{{}}}}}}"),
            ),
            (false, Some(version)) => (
                "200 OK".to_owned(),
                format!(
                    "{{\"name\":\"{name}\",\"vers\":\"{version}\",\"deps\":[],\
                     \"cksum\":\"0\",\"features\":{{}},\"yanked\":false}}\n"
                ),
            ),
        };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/x-ndjson\r\nContent-Length: {}\r\n\
             Connection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).ok()?;
        stream.flush().ok()?;
        Some(path)
    }
}

/// Commits `.cargo/config.toml` into the fixture (the cargo
/// configuration the routing resolution reads at publish time).
fn configure_registry(fx: &mut GitFixture, config: &str) -> Result<(), BoxError> {
    fx.commit_with(&CommitSpec::files(
        vec![(".cargo/config.toml".to_owned(), config.to_owned())],
        "chore: configure the corp registry",
    ))?;
    Ok(())
}

/// A configured custom registry routes: the dry forecast proceeds over
/// the shimmed tool (no probe — dry never touches the registry, I5)
/// and the argv carries the documented `--registry <name>` mapping.
#[test]
fn publish_dry_forecasts_a_configured_custom_registry() -> Result<(), BoxError> {
    let config = "[publish]\nenabled = true\n\n[publish.cargo]\nregistry = \"corp\"\n";
    let mut fx = build_workspace("custom-dry", config)?;
    configure_registry(
        &mut fx,
        "[registries.corp]\nindex = \"sparse+https://corp.example/index/\"\n",
    )?;
    let shim = write_cargo_shim("shim-custom-dry")?;
    let log = fixture_dir("shim-log-custom-dry")?.join("argv.log");
    let (out, text) = halyard_env(
        fx.root(),
        &["publish", "--dry", "--format", "json"],
        Some(&shim),
        Some(&log),
    )?;
    assert!(
        out.status.success(),
        "the configured registry routes: {text}"
    );
    assert!(text.contains("\"forecast\""), "rows say forecast: {text}");
    let recorded = std::fs::read_to_string(&log)?;
    assert!(
        recorded.contains("--registry") && recorded.contains("corp"),
        "the argv carries the registry mapping: {recorded}"
    );
    Ok(())
}

/// A custom registry configured with a git-protocol index is an honest
/// refusal: the probe speaks the sparse-HTTP protocol only, the decline
/// names the protocol and the fix, and nothing spawns (zero side
/// effects — validate-then-act, §15.3).
#[test]
fn publish_refuses_a_custom_registry_with_a_git_index() -> Result<(), BoxError> {
    let config = "[publish]\nenabled = true\n\n[publish.cargo]\nregistry = \"corp\"\n";
    let mut fx = build_workspace("custom-git", config)?;
    configure_registry(
        &mut fx,
        "[registries.corp]\nindex = \"https://corp.example/git/index\"\n",
    )?;
    let shim = write_cargo_shim("shim-custom-git")?;
    let log = fixture_dir("shim-log-custom-git")?.join("argv.log");
    let (out, text) = halyard_env(
        fx.root(),
        &["publish", "--dry", "--format", "json"],
        Some(&shim),
        Some(&log),
    )?;
    assert!(
        !out.status.success(),
        "a git index is not probeable: {text}"
    );
    assert!(text.contains("E-PUB-REGISTRY"), "the stable code: {text}");
    assert!(
        text.contains("git") && text.contains("sparse+"),
        "the decline names the protocol and the fix: {text}"
    );
    assert!(
        !std::fs::exists(&log).unwrap_or(false) || std::fs::read_to_string(&log)?.trim().is_empty(),
        "zero side effects — the tool never spawned: {text}"
    );
    Ok(())
}

/// The end-to-end custom-registry publish (the routing map's new
/// client): the resume probe hits the configured sparse index over the
/// loopback transport, the shim records the `--registry` argv, and the
/// §13 step-3 visibility wait confirms both targets through the same
/// index — zero duplicates, dependents un-gated.
#[test]
fn publish_live_probes_a_custom_registry_over_loopback() -> Result<(), BoxError> {
    let config = "[publish]\nenabled = true\n\n[publish.cargo]\nregistry = \"corp\"\n";
    let mut fx = build_workspace("custom-live", config)?;
    let (index, base) = FakeIndex::start(
        BTreeMap::from([
            ("p1".to_owned(), "0.1.1".to_owned()),
            ("p2".to_owned(), "0.1.1".to_owned()),
        ]),
        "/index",
        false,
    )?;
    configure_registry(
        &mut fx,
        &format!("[registries.corp]\nindex = \"sparse+{base}/\"\n"),
    )?;
    let shim = write_cargo_shim("shim-custom-live")?;
    let log = fixture_dir("shim-log-custom-live")?.join("argv.log");
    let (out, text) = halyard_env(
        fx.root(),
        &["publish", "--format", "json"],
        Some(&shim),
        Some(&log),
    )?;
    assert!(
        out.status.success(),
        "the loopback publish succeeded: {text}"
    );
    assert!(
        text.contains("\"published\": 2"),
        "both targets landed: {text}"
    );
    assert!(
        text.contains("\"visibility_confirmed\": true"),
        "the visibility wait confirmed through the custom index: {text}"
    );
    let recorded = std::fs::read_to_string(&log)?;
    assert!(
        recorded.contains("--registry") && recorded.contains("corp"),
        "the argv carries the registry mapping: {recorded}"
    );
    // The probe sequence rode the custom index: one resume probe plus
    // one visibility probe per target (the §13 step-2/3 shape), at the
    // sparse-index paths of both package names.
    assert_eq!(
        index.served(),
        4,
        "resume + visibility per target: served {:?}",
        index.served_paths()
    );
    let served = index.served_paths();
    for path in ["/index/2/p1", "/index/2/p2"] {
        assert!(
            served.iter().any(|seen| seen == path),
            "the probe reached the custom index at {path}: served {served:?}"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The npm-side per-target routing (M5.2 continued, ADR-0021): a
// package.json `publishConfig.registry` names the registry the spawned
// `pnpm publish` targets, so the §13 probe resolves the same way — a
// custom base joins the routing map as a per-target override, offline-
// verifiable end to end over a loopback packument.
// ---------------------------------------------------------------------------

/// A two-package pnpm workspace plus a feat commit; `publish_config` is
/// written into **every** package.json (the per-package override the
/// npm routing reads).
fn build_npm_workspace(label: &str, publish_config: Option<&str>) -> Result<GitFixture, BoxError> {
    let dir = fixture_dir(label)?;
    let mut fx = GitFixture::init_in(&dir)?;
    let publish = publish_config
        .map(|registry| format!("\"publishConfig\": {{\"registry\": \"{registry}\"}},"))
        .unwrap_or_default();
    let root = format!(
        "{{\n  \"name\": \"npm-ws\",\n  \"private\": true,\n  {publish}\n  \"workspaces\": \
         [\"pkgs/*\"]\n}}\n"
    );
    let p1 = format!(
        "{{\n  \"name\": \"p1\",\n  \"version\": \"0.1.0\",\n  {publish}\n  \"main\": \
         \"lib.js\"\n}}\n"
    );
    let p2 = format!(
        "{{\n  \"name\": \"p2\",\n  \"version\": \"0.1.0\",\n  {publish}\n  \"main\": \
         \"lib.js\",\n  \"dependencies\": {{\"p1\": \"^0.1.0\"}}\n}}\n"
    );
    fx.commit_with(&CommitSpec::files(
        vec![
            ("package.json".to_owned(), root),
            ("pkgs/p1/package.json".to_owned(), p1),
            ("pkgs/p1/lib.js".to_owned(), String::new()),
            ("pkgs/p2/package.json".to_owned(), p2),
            ("pkgs/p2/lib.js".to_owned(), String::new()),
        ],
        "chore: scaffold npm workspace",
    ))?;
    let feat = vec![
        (
            "pkgs/p1/lib.js".to_owned(),
            "exports.const = 2;\n".to_owned(),
        ),
        (
            "pkgs/p2/lib.js".to_owned(),
            "exports.const = 3;\n".to_owned(),
        ),
    ];
    fx.commit_with(&CommitSpec::files(feat, "feat: bump the constants"))?;
    Ok(fx)
}

/// A `pnpm` stand-in that records its argv and exits 0 (the packaged
/// tool never runs; the argv is the assertion).
fn write_pnpm_shim(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir = fixture_dir(label)?;
    let script = dir.join("pnpm");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$HALYARD_SHIM_LOG\"\nexit 0\n",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms)?;
    }
    Ok(dir)
}

/// The dry forecast proceeds for an npm workspace whose packages pin a
/// custom registry: routing validation is offline (the override client
/// builds from the publishConfig value; the dry run never probes, I5).
#[test]
fn publish_dry_forecasts_an_npm_publishconfig_workspace() -> Result<(), BoxError> {
    let mut fx = build_npm_workspace("npm-dry", Some("https://corp.example/registry"))?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "halyard.toml".to_owned(),
            "[publish]\nenabled = true\n".to_owned(),
        )],
        "chore: enable publishing",
    ))?;
    let shim = write_pnpm_shim("shim-npm-dry")?;
    let log = fixture_dir("shim-log-npm-dry")?.join("argv.log");
    let (out, text) = halyard_env(
        fx.root(),
        &["publish", "--dry", "--format", "json"],
        Some(&shim),
        Some(&log),
    )?;
    assert!(out.status.success(), "the pinned registry routes: {text}");
    assert!(text.contains("\"forecast\""), "rows say forecast: {text}");
    assert!(text.contains("pkgs/p1"), "npm targets named: {text}");
    let recorded = std::fs::read_to_string(&log)?;
    assert!(
        recorded.contains("publish"),
        "the pnpm argv is recorded: {recorded}"
    );
    Ok(())
}

/// The end-to-end npm publish over a per-target override: the resume
/// probe hits the publishConfig registry (the loopback packument), the
/// shimmed `pnpm` records its argv, and the §13 step-3 wait confirms
/// through the same base — the probes rode the *custom* base, not the
/// default registry.
#[test]
fn publish_live_probes_an_npm_publishconfig_over_loopback() -> Result<(), BoxError> {
    let (index, base) = FakeIndex::start(
        BTreeMap::from([
            ("p1".to_owned(), "0.1.1".to_owned()),
            ("p2".to_owned(), "0.1.1".to_owned()),
        ]),
        "",
        true,
    )?;
    let mut fx = build_npm_workspace("npm-live", Some(&base))?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "halyard.toml".to_owned(),
            "[publish]\nenabled = true\n".to_owned(),
        )],
        "chore: enable publishing",
    ))?;
    let shim = write_pnpm_shim("shim-npm-live")?;
    let log = fixture_dir("shim-log-npm-live")?.join("argv.log");
    let (out, text) = halyard_env(
        fx.root(),
        &["publish", "--format", "json"],
        Some(&shim),
        Some(&log),
    )?;
    assert!(
        out.status.success(),
        "the loopback publish succeeded: {text}"
    );
    assert!(
        text.contains("\"published\": 2"),
        "both targets landed: {text}"
    );
    assert!(
        text.contains("\"visibility_confirmed\": true"),
        "the visibility wait confirmed through the custom registry: {text}"
    );
    // The probes reached the *packument* paths of the custom base (the
    // default registry is never contacted).
    let served = index.served_paths();
    for path in ["/p1", "/p2"] {
        assert!(
            served.iter().any(|seen| seen == path),
            "the probe reached the custom registry at {path}: served {served:?}"
        );
    }
    assert_eq!(index.served(), 4, "resume + visibility per target");
    let _ = std::fs::read_to_string(&log)?;
    Ok(())
}

/// A package pinning a non-HTTP registry is an honest refusal naming
/// the package and the manifest — nothing spawns (validate-then-act).
#[test]
fn publish_refuses_an_npm_package_with_a_non_http_registry() -> Result<(), BoxError> {
    let mut fx = build_npm_workspace("npm-bad", Some("corp.example/registry"))?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "halyard.toml".to_owned(),
            "[publish]\nenabled = true\n".to_owned(),
        )],
        "chore: enable publishing",
    ))?;
    let shim = write_pnpm_shim("shim-npm-bad")?;
    let log = fixture_dir("shim-log-npm-bad")?.join("argv.log");
    let (out, text) = halyard_env(
        fx.root(),
        &["publish", "--dry", "--format", "json"],
        Some(&shim),
        Some(&log),
    )?;
    assert!(
        !out.status.success(),
        "a non-http base is not probeable: {text}"
    );
    assert!(text.contains("E-PUB-REGISTRY"), "the stable code: {text}");
    assert!(
        text.contains("p1") && text.contains("package.json"),
        "the refusal names the package and the manifest: {text}"
    );
    assert!(
        !std::fs::exists(&log).unwrap_or(false) || std::fs::read_to_string(&log)?.trim().is_empty(),
        "zero side effects — the tool never spawned: {text}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// The OIDC trusted-publishing wire-up (M5.3 step 3, ADR-0031): the
// context detection rides the shell's fail-closed refusal point (a
// half-configured GitHub Actions OIDC context refuses before any plan,
// probe or spawn — offline testable, the exchange itself is
// contract-tested in halyard-publish over the scripted transport), and
// `doctor` reports the detected context.
// ---------------------------------------------------------------------------

#[test]
fn oidc_half_context_refuses_before_any_spawn() -> Result<(), BoxError> {
    let fx = build_workspace("oidc-half", publish_enabled_config())?;
    let shim = write_cargo_shim("shim-oidc-half")?;
    let log = fixture_dir("shim-log-oidc-half")?.join("argv.log");
    let (out, text) = halyard_env_with(
        fx.root(),
        &["publish", "--dry", "--format", "json"],
        Some(&shim),
        Some(&log),
        &[(
            "ACTIONS_ID_TOKEN_REQUEST_URL",
            "https://actions.example.test/oidc?audience=",
        )],
    )?;
    assert!(!out.status.success(), "the half context refuses: {text}");
    assert!(
        text.contains("E-PUB-AUTH"),
        "the refusal carries the seam's stable code: {text}"
    );
    assert!(
        text.contains("ACTIONS_ID_TOKEN_REQUEST_TOKEN"),
        "the message names the missing variable (the fix): {text}"
    );
    assert!(
        std::fs::read_to_string(&log).is_err() || std::fs::read_to_string(&log)?.trim().is_empty(),
        "no tool spawn may fire after the refusal"
    );
    Ok(())
}

#[test]
fn doctor_reports_the_publish_oidc_row() -> Result<(), BoxError> {
    let fx = build_workspace("doctor-oidc", publish_enabled_config())?;
    // A detected context: the row reports it, names never values.
    let (out, text) = halyard_env_with(
        fx.root(),
        &["doctor", "--format", "json"],
        None,
        None,
        &[
            (
                "ACTIONS_ID_TOKEN_REQUEST_URL",
                "https://actions.example.test/oidc?audience=",
            ),
            ("ACTIONS_ID_TOKEN_REQUEST_TOKEN", OIDC_REQUEST_TOKEN),
        ],
    )?;
    assert!(out.status.success(), "doctor failed: {text}");
    assert!(text.contains("publish-oidc"), "the check row: {text}");
    assert!(
        text.contains("gha-oidc("),
        "the detected context is named: {text}"
    );
    assert!(
        !text.contains(OIDC_REQUEST_TOKEN),
        "the request token never reaches doctor: {text}"
    );
    // Absence (outside GitHub Actions): the normal shape, still ok.
    let (out, text) =
        halyard_env_with(fx.root(), &["doctor", "--format", "json"], None, None, &[])?;
    assert!(out.status.success(), "doctor failed: {text}");
    assert!(
        text.contains("no GitHub Actions OIDC context"),
        "absence is honest: {text}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// The npm publish options (M5.3 step 6's npm half, ADR-0033): the
// `publish.npm.access`/`provenance` config rides the plan contract into
// the spawn argv, the report rows surface it, and a live
// `provenance = true` run refuses outside a supported cloud runtime
// before any call. Written first (red), implemented second.
// ---------------------------------------------------------------------------

/// The npm publish-options config: explicit access + provenance.
fn npm_options_config() -> &'static str {
    "[publish]\nenabled = true\n\n[publish.npm]\naccess = \"public\"\nprovenance = true\n"
}

#[test]
fn npm_publish_options_ride_the_forecast_argv_and_rows() -> Result<(), BoxError> {
    let mut fx = build_npm_workspace("npm-opts-dry", None)?;
    fx.commit_with(&CommitSpec::files(
        vec![("halyard.toml".to_owned(), npm_options_config().to_owned())],
        "chore: enable npm publishing",
    ))?;
    let shim = write_pnpm_shim("shim-npm-opts-dry")?;
    let log = fixture_dir("shim-log-npm-opts-dry")?.join("argv.log");
    let (out, text) = halyard_env(
        fx.root(),
        &["publish", "--dry", "--format", "json"],
        Some(&shim),
        Some(&log),
    )?;
    assert!(out.status.success(), "the dry forecast runs: {text}");
    // The rows carry the resolved options (the report's machine shape).
    assert!(
        text.contains("\"access\": \"public\""),
        "the access row: {text}"
    );
    assert!(
        text.contains("\"provenance\": true"),
        "the provenance row: {text}"
    );
    // The shim captured the documented argv per target.
    let recorded = std::fs::read_to_string(&log)?;
    assert!(
        recorded.contains("--access") && recorded.contains("public"),
        "the access argv: {recorded}"
    );
    assert!(
        recorded.contains("--provenance"),
        "the provenance argv: {recorded}"
    );
    Ok(())
}

#[test]
fn npm_provenance_live_refuses_outside_a_cloud_runtime() -> Result<(), BoxError> {
    // The verified npm musts (docs.npmjs.com, §0.4): a supported cloud
    // CI/CD on a hosted runner, `id-token: write`. Outside one, a live
    // `provenance = true` run fails closed before any probe or spawn —
    // the refusal names the fix and the escape hatch.
    let (index, base) = FakeIndex::start(
        BTreeMap::from([
            ("p1".to_owned(), "0.1.1".to_owned()),
            ("p2".to_owned(), "0.1.1".to_owned()),
        ]),
        "",
        true,
    )?;
    let mut fx = build_npm_workspace("npm-opts-live-refuse", Some(&base))?;
    fx.commit_with(&CommitSpec::files(
        vec![("halyard.toml".to_owned(), npm_options_config().to_owned())],
        "chore: enable npm publishing",
    ))?;
    let shim = write_pnpm_shim("shim-npm-opts-live-refuse")?;
    let log = fixture_dir("shim-log-npm-opts-live-refuse")?.join("argv.log");
    let (out, text) = halyard_env(
        fx.root(),
        &["publish", "--format", "json"],
        Some(&shim),
        Some(&log),
    )?;
    assert!(
        !out.status.success(),
        "the unsupported runtime refuses: {text}"
    );
    assert!(text.contains("E-PUB-AUTH"), "the stable code: {text}");
    assert!(
        text.contains("id-token: write"),
        "the must is named: {text}"
    );
    assert!(
        text.contains("provenance = false"),
        "the escape hatch is named: {text}"
    );
    assert!(
        !std::fs::exists(&log).unwrap_or(false) || std::fs::read_to_string(&log)?.trim().is_empty(),
        "zero side effects — the tool never spawned: {text}"
    );
    assert_eq!(index.served(), 0, "zero probes before the refusal");
    Ok(())
}

#[test]
fn npm_provenance_live_runs_under_a_detected_cloud_runtime() -> Result<(), BoxError> {
    // The GitLab marker (VERIFIED: docs.gitlab.com predefined variables,
    // `GITLAB_CI` = "true when available") is a supported cloud runtime:
    // the run proceeds over the loopback registry, the tool's own
    // trusted-publishing posture governs the attestation.
    let (index, base) = FakeIndex::start(
        BTreeMap::from([
            ("p1".to_owned(), "0.1.1".to_owned()),
            ("p2".to_owned(), "0.1.1".to_owned()),
        ]),
        "",
        true,
    )?;
    let mut fx = build_npm_workspace("npm-opts-live-gitlab", Some(&base))?;
    fx.commit_with(&CommitSpec::files(
        vec![("halyard.toml".to_owned(), npm_options_config().to_owned())],
        "chore: enable npm publishing",
    ))?;
    let shim = write_pnpm_shim("shim-npm-opts-live-gitlab")?;
    let log = fixture_dir("shim-log-npm-opts-live-gitlab")?.join("argv.log");
    let (out, text) = halyard_env_with(
        fx.root(),
        &["publish", "--format", "json"],
        Some(&shim),
        Some(&log),
        &[("GITLAB_CI", "true")],
    )?;
    assert!(out.status.success(), "the detected runtime runs: {text}");
    assert!(
        text.contains("\"published\": 2"),
        "both targets landed: {text}"
    );
    let recorded = std::fs::read_to_string(&log)?;
    assert!(
        recorded.contains("--access") && recorded.contains("public"),
        "the access argv: {recorded}"
    );
    assert!(
        recorded.contains("--provenance"),
        "the provenance argv: {recorded}"
    );
    // The resume probes rode the loopback registry (the run reached the
    // spawns, not only the detection).
    assert!(
        index.served() >= 2,
        "the loopback packuments were probed: {}",
        index.served()
    );
    Ok(())
}

#[test]
fn npm_trusted_publishing_decides_when_options_are_unset() -> Result<(), BoxError> {
    // Unset is semantic (VERIFIED: pnpm.io `provenance` config —
    // "When unset, trusted publishing decides"): no flag is invented,
    // and the report rows stay free of the option keys.
    let mut fx = build_npm_workspace("npm-opts-unset", None)?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "halyard.toml".to_owned(),
            "[publish]\nenabled = true\n".to_owned(),
        )],
        "chore: enable npm publishing",
    ))?;
    let shim = write_pnpm_shim("shim-npm-opts-unset")?;
    let log = fixture_dir("shim-log-npm-opts-unset")?.join("argv.log");
    let (out, text) = halyard_env(
        fx.root(),
        &["publish", "--dry", "--format", "json"],
        Some(&shim),
        Some(&log),
    )?;
    assert!(out.status.success(), "the dry forecast runs: {text}");
    let recorded = std::fs::read_to_string(&log)?;
    assert!(
        !recorded.contains("--provenance")
            && !recorded.contains("--no-provenance")
            && !recorded.contains("--access"),
        "no invented flags: {recorded}"
    );
    assert!(
        !text.contains("\"access\"") && !text.contains("\"provenance\""),
        "the rows stay free of the unset options: {text}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// M8 step 2 (ADR-0056): the pypi lane — a uv workspace publishes through
// the shimmed `uv`; the argv is the contract (bare live, `--dry-run`
// forecast); the PyPI probe joins the router under the plan contract's
// `pypi` registry name.
// ---------------------------------------------------------------------------

/// A uv workspace: the root project depends on one globed member; a
/// feat commit drives the plan's releases.
fn build_pypi_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let dir = fixture_dir(label)?;
    let mut fx = GitFixture::init_in(&dir)?;
    fx.commit_with(&CommitSpec::files(
        vec![
            (
                "pyproject.toml".to_owned(),
                "[project]\nname = \"py-ws\"\nversion = \"0.1.0\"\ndependencies = [\"p1>=0.1\"]\n\n[tool.uv.sources]\np1 = { workspace = true }\n\n[tool.uv.workspace]\nmembers = [\"pkgs/*\"]\n".to_owned(),
            ),
            (
                "pkgs/p1/pyproject.toml".to_owned(),
                "[project]\nname = \"p1\"\nversion = \"0.1.0\"\ndependencies = []\n".to_owned(),
            ),
        ],
        "chore: scaffold uv workspace",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("pkgs/p1/README.md".to_owned(), "member change\n".to_owned())],
        "feat: the member changes",
    ))?;
    Ok(fx)
}

/// A `uv` stand-in that records its argv and exits 0 (the packaged tool
/// never runs; the argv is the assertion).
fn write_uv_shim(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir = fixture_dir(label)?;
    let script = dir.join("uv");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$HALYARD_SHIM_LOG\"\nexit 0\n",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms)?;
    }
    Ok(dir)
}

/// The dry forecast for a uv workspace: pypi targets ride the plan
/// contract's `pypi` registry (no npm options), and the shimmed `uv`
/// records the documented `--dry-run` argv — the forecast is offline
/// (dry never probes, I5).
#[test]
fn publish_dry_forecasts_a_pypi_uv_workspace() -> Result<(), BoxError> {
    let mut fx = build_pypi_workspace("pypi-dry")?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "halyard.toml".to_owned(),
            "[publish]\nenabled = true\n".to_owned(),
        )],
        "chore: enable publishing",
    ))?;
    let shim = write_uv_shim("shim-pypi-dry")?;
    let log = fixture_dir("shim-log-pypi-dry")?.join("argv.log");
    let (out, text) = halyard_env(
        fx.root(),
        &["publish", "--dry", "--format", "json"],
        Some(&shim),
        Some(&log),
    )?;
    assert!(out.status.success(), "the pypi forecast proceeded: {text}");
    assert!(text.contains("\"forecast\""), "rows say forecast: {text}");
    // The default group is independent policy (§5.4): the feat commit
    // touched the member only, so `p1` is the release target.
    assert!(text.contains("p1"), "pypi targets named: {text}");
    let recorded = std::fs::read_to_string(&log)?;
    assert!(
        recorded.contains("publish") && recorded.contains("--dry-run"),
        "the uv argv is recorded: {recorded}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// M8 (ADR-0057): the go lane — tag-only. A go workspace publishes
// NOTHING: the release rows carry no publish decision (`publish: None`),
// the engine's documented skip records them as `no_publish_contract`,
// zero targets exist, and no tool is spawned (there is no go publish
// step to shim — the tag is the artifact).
// ---------------------------------------------------------------------------

/// A go workspace: a root module plus one subdir member (`go.work`), a
/// feat commit driving the member's plan row. The manifests carry no
/// version anywhere — the release tag is the version.
fn build_go_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let dir = fixture_dir(label)?;
    let mut fx = GitFixture::init_in(&dir)?;
    fx.commit_with(&CommitSpec::files(
        vec![
            ("go.work".to_owned(), "go 1.23.0\n\nuse (\n\t.\n\t./api\n)\n".to_owned()),
            (
                "go.mod".to_owned(),
                "module example.com/halyard\n\ngo 1.23.0\n\nrequire example.com/halyard/api v0.0.0\n".to_owned(),
            ),
            (
                "api/go.mod".to_owned(),
                "module example.com/halyard/api\n\ngo 1.23.0\n".to_owned(),
            ),
        ],
        "chore: scaffold go workspace",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("api/README.md".to_owned(), "member change\n".to_owned())],
        "feat: the member changes",
    ))?;
    Ok(fx)
}

/// The dry forecast for a go workspace: zero publish targets, the
/// release recorded as a `no_publish_contract` skip (never
/// `publish_disabled` — no publish decision exists to disable), and a
/// successful offline run.
#[test]
fn publish_dry_forecasts_no_registry_for_a_go_workspace() -> Result<(), BoxError> {
    let mut fx = build_go_workspace("go-dry")?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "halyard.toml".to_owned(),
            "[publish]\nenabled = true\n".to_owned(),
        )],
        "chore: enable publishing",
    ))?;
    let (out, text) = halyard_env(
        fx.root(),
        &["publish", "--dry", "--format", "json"],
        None,
        None,
    )?;
    assert!(out.status.success(), "the go forecast succeeded: {text}");
    let value: serde_json::Value = serde_json::from_str(
        text.trim_start_matches(char::is_whitespace)
            .trim_start_matches(|c: char| c != '{'),
    )
    .map_err(|e| format!("the report is JSON: {e}\n{text}"))?;
    assert_eq!(
        value.get("targets").and_then(serde_json::Value::as_u64),
        Some(0),
        "zero publish targets: {text}"
    );
    let skips = value
        .get("not_targets")
        .and_then(serde_json::Value::as_array)
        .ok_or("not_targets is an array")?;
    let member = skips
        .iter()
        .find(|row| {
            row.get("package").and_then(serde_json::Value::as_str)
                == Some("example.com/halyard/api")
        })
        .ok_or("the go release is recorded as a skip")?;
    assert_eq!(
        member.get("reason").and_then(serde_json::Value::as_str),
        Some("no_publish_contract"),
        "the tag-only skip reason: {text}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// M8 (ADR-0058): the maven lane — the publish posture. A maven release
// carries a publish decision with the engine's disabled shape: the row
// records `publish_disabled` (no maven publish grammar exists — no `mvn`
// argv arm, no registry probe, no auth arm), zero targets, no tool
// spawn.
// ---------------------------------------------------------------------------

/// A maven multi-module workspace: a root aggregator plus one member with
/// its own version, a feat commit driving the member's plan row.
fn build_maven_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let dir = fixture_dir(label)?;
    let mut fx = GitFixture::init_in(&dir)?;
    fx.commit_with(&CommitSpec::files(
        vec![
            (
                "pom.xml".to_owned(),
                concat!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
                    "<project xmlns=\"http://maven.apache.org/POM/4.0.0\">\n",
                    "  <modelVersion>4.0.0</modelVersion>\n",
                    "  <groupId>com.example</groupId>\n",
                    "  <artifactId>halyard-mvn</artifactId>\n",
                    "  <version>1.0.0</version>\n",
                    "  <packaging>pom</packaging>\n",
                    "  <modules>\n    <module>api</module>\n  </modules>\n",
                    "</project>\n",
                )
                .to_owned(),
            ),
            (
                "api/pom.xml".to_owned(),
                concat!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
                    "<project xmlns=\"http://maven.apache.org/POM/4.0.0\">\n",
                    "  <modelVersion>4.0.0</modelVersion>\n",
                    "  <parent>\n",
                    "    <groupId>com.example</groupId>\n",
                    "    <artifactId>halyard-mvn</artifactId>\n",
                    "    <version>1.0.0</version>\n",
                    "  </parent>\n",
                    "  <artifactId>halyard-mvn-api</artifactId>\n",
                    "  <version>1.0.0</version>\n",
                    "</project>\n",
                )
                .to_owned(),
            ),
        ],
        "chore: scaffold maven workspace",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("api/README.md".to_owned(), "member change\n".to_owned())],
        "feat: the member changes",
    ))?;
    Ok(fx)
}

/// The dry forecast for a maven workspace: zero publish targets, the
/// member recorded as a `publish_disabled` skip (the row's publish
/// decision carries the engine's disabled shape — `registry` names the
/// canonical maven-central, `enabled` is false until the argv arm
/// lands), and a successful offline run.
#[test]
fn publish_dry_records_disabled_for_a_maven_workspace() -> Result<(), BoxError> {
    let mut fx = build_maven_workspace("maven-dry")?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "halyard.toml".to_owned(),
            "[publish]\nenabled = true\n".to_owned(),
        )],
        "chore: enable publishing",
    ))?;
    let (out, text) = halyard_env(
        fx.root(),
        &["publish", "--dry", "--format", "json"],
        None,
        None,
    )?;
    assert!(out.status.success(), "the maven forecast succeeded: {text}");
    let value: serde_json::Value = serde_json::from_str(
        text.trim_start_matches(char::is_whitespace)
            .trim_start_matches(|c: char| c != '{'),
    )
    .map_err(|e| format!("the report is JSON: {e}\n{text}"))?;
    assert_eq!(
        value.get("targets").and_then(serde_json::Value::as_u64),
        Some(0),
        "zero publish targets: {text}"
    );
    let skips = value
        .get("not_targets")
        .and_then(serde_json::Value::as_array)
        .ok_or("not_targets is an array")?;
    let member = skips
        .iter()
        .find(|row| {
            row.get("package").and_then(serde_json::Value::as_str) == Some("halyard-mvn-api")
        })
        .ok_or("the maven release is recorded as a skip")?;
    assert_eq!(
        member.get("reason").and_then(serde_json::Value::as_str),
        Some("publish_disabled"),
        "the disabled-shape skip reason: {text}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// M8 (ADR-0059): the nuget lane — the publish posture. A nuget release
// carries a publish decision with the engine's disabled shape: the row
// records `publish_disabled` (no nuget publish grammar exists — no
// `dotnet` argv arm, no registry probe, no auth arm), zero targets, no
// tool spawn.
// ---------------------------------------------------------------------------

/// A nuget SDK-style workspace: one project with a `PackageVersion`
/// row, a feat commit driving the plan row.
fn build_nuget_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let dir = fixture_dir(label)?;
    let mut fx = GitFixture::init_in(&dir)?;
    fx.commit_with(&CommitSpec::files(
        vec![
            (
                "src/app/App.csproj".to_owned(),
                concat!(
                    "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n",
                    "<Project Sdk=\"Microsoft.NET.Sdk\">\n",
                    "  <PropertyGroup>\n",
                    "    <TargetFramework>net10.0</TargetFramework>\n",
                    "    <PackageId>Contoso.App</PackageId>\n",
                    "    <PackageVersion>1.0.0</PackageVersion>\n",
                    "  </PropertyGroup>\n",
                    "</Project>\n",
                )
                .to_owned(),
            ),
            (
                "halyard.toml".to_owned(),
                "[publish]\nenabled = true\n".to_owned(),
            ),
        ],
        "chore: scaffold nuget workspace",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![(
            "src/app/README.md".to_owned(),
            "the app changes\n".to_owned(),
        )],
        "feat: the app changes",
    ))?;
    Ok(fx)
}

/// The dry forecast for a nuget workspace: zero publish targets, the
/// release recorded as a `publish_disabled` skip (the row's publish
/// decision carries the engine's disabled shape — `registry` names the
/// canonical nuget.org, `enabled` is false until the argv arm lands),
/// and a successful offline run.
#[test]
fn publish_dry_records_disabled_for_a_nuget_workspace() -> Result<(), BoxError> {
    let fx = build_nuget_workspace("nuget-dry")?;
    let (out, text) = halyard_env(
        fx.root(),
        &["publish", "--dry", "--format", "json"],
        None,
        None,
    )?;
    assert!(out.status.success(), "the nuget forecast succeeded: {text}");
    let value: serde_json::Value = serde_json::from_str(
        text.trim_start_matches(char::is_whitespace)
            .trim_start_matches(|c: char| c != '{'),
    )
    .map_err(|e| format!("the report is JSON: {e}\n{text}"))?;
    assert_eq!(
        value.get("targets").and_then(serde_json::Value::as_u64),
        Some(0),
        "zero publish targets: {text}"
    );
    let skips = value
        .get("not_targets")
        .and_then(serde_json::Value::as_array)
        .ok_or("not_targets is an array")?;
    let member = skips
        .iter()
        .find(|row| row.get("package").and_then(serde_json::Value::as_str) == Some("Contoso.App"))
        .ok_or("the nuget release is recorded as a skip")?;
    assert_eq!(
        member.get("reason").and_then(serde_json::Value::as_str),
        Some("publish_disabled"),
        "the disabled-shape skip reason: {text}"
    );
    Ok(())
}
