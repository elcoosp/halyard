//! CLI `publish` smoke tests (M5.2 step 2): builds a real git fixture
//! workspace, runs the compiled binary with `publish` in dry and live
//! modes. The tool spawn is exercised through a PATH shim that records
//! its argv (the real `cargo`/`pnpm` never runs; the network is never
//! touched — the dry forecast skips the resume probes, I5, and the
//! live-mode tests here only cover the offline paths: zero targets and
//! the fail-closed routing gap).

use std::process::Command;

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
        recorded.contains("crates/p1/Cargo.toml"),
        "manifest scoped: {recorded}"
    );
    assert!(
        recorded.contains("crates/p2/Cargo.toml"),
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
