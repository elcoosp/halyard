//! CLI e2e over the `uv.lock` lockfile half (the §17 M8 `PyPI` cell's
//! recorded-open remainder, ADR-0056): a uv workspace with a committed
//! root `uv.lock`. The apply flow stages the lockfile through the
//! adapter's `patch_lockfile` seam — the adapter declines at uv's own
//! documented posture (*"managed by uv and should not be edited
//! manually"*, `docs/research/pypi-uv-grammars.md` §3) — and the
//! orchestrator's §9.2 rule-4 native refresh (`uv lock --offline`, the
//! tool's documented refresh) rewrites the member rows. The verifier
//! (`verify_native_refresh`) pins the observed fidelity: a member bump
//! moves exactly the member's `version` row, every other byte survives.

use std::process::Command;

use halyard_test_support::{CommitSpec, GitFixture};

type BoxError = Box<dyn std::error::Error>;

fn fixture_dir(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir =
        std::env::temp_dir().join(format!("halyard-cli-uvlock-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// The committed root `uv.lock` for the fixture workspace (the observed
/// uv 0.13.10 shape).
const UV_LOCK: &str = "version = 1\nrevision = 5\nrequires-python = \">=3.9\"\n\n[manifest]\nmembers = [\n    \"app-a\",\n    \"app-b\",\n    \"root\",\n]\n\n[[package]]\nname = \"app-a\"\nversion = \"0.1.0\"\nsource = { editable = \"app_a\" }\n\n[[package]]\nname = \"app-b\"\nversion = \"0.1.0\"\nsource = { editable = \"app_b\" }\n\n[[package]]\nname = \"root\"\nversion = \"0.0.0\"\nsource = { virtual = \".\" }\n";

/// A uv workspace: the virtual root claiming two members, one feat
/// commit under `app_a/` driving the release row.
fn build_uv_workspace(label: &str) -> Result<GitFixture, BoxError> {
    let dir = fixture_dir(label)?;
    let mut fx = GitFixture::init_in(&dir)?;
    fx.commit_with(&CommitSpec::files(
        vec![
            (
                "pyproject.toml".to_owned(),
                "[project]\nname = \"root\"\nversion = \"0.0.0\"\nrequires-python = \">=3.9\"\n\n[tool.uv.workspace]\nmembers = [\"app_a\", \"app_b\"]\n".to_owned(),
            ),
            (
                "app_a/pyproject.toml".to_owned(),
                "[project]\nname = \"app-a\"\nversion = \"0.1.0\"\nrequires-python = \">=3.9\"\n\n[build-system]\nrequires = [\"hatchling\"]\nbuild-backend = \"hatchling.build\"\n".to_owned(),
            ),
            (
                "app_b/pyproject.toml".to_owned(),
                "[project]\nname = \"app-b\"\nversion = \"0.1.0\"\nrequires-python = \">=3.9\"\n\n[build-system]\nrequires = [\"hatchling\"]\nbuild-backend = \"hatchling.build\"\n".to_owned(),
            ),
            ("uv.lock".to_owned(), UV_LOCK.to_owned()),
        ],
        "chore: scaffold the uv workspace",
    ))?;
    fx.commit_with(&CommitSpec::files(
        vec![("app_a/feature.py".to_owned(), "the feature\n".to_owned())],
        "feat: the app-a feature",
    ))?;
    Ok(fx)
}

fn halyard(root: &std::path::Path, args: &[&str]) -> Result<std::process::Output, BoxError> {
    Command::new(env!("CARGO_BIN_EXE_halyard"))
        .arg("-C")
        .arg(root)
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .map_err(Into::into)
}

/// Whether the native refresh tool is installed in this environment
/// (the success-path e2e runs only where `uv` exists; CI's rust-only
/// image skips it — the no-uv refusal e2e below is the hermetic half).
fn uv_available() -> bool {
    Command::new("uv")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

/// The hermetic half: without `uv` on the child's PATH the native
/// refresh refuses fail-closed (`E-LOCK-NATIVE`) naming the exact argv
/// — an effect step that silently did nothing would violate I2, and the
/// lockfile state is uv's business. The lockfile's bytes stay untouched.
#[test]
fn the_native_refresh_refuses_without_the_tool() -> Result<(), BoxError> {
    let fx = build_uv_workspace("no-uv")?;
    let out = Command::new(env!("CARGO_BIN_EXE_halyard"))
        .arg("-C")
        .arg(fx.root())
        .arg("apply")
        .env("NO_COLOR", "1")
        // An empty PATH: the child resolves no `uv` — the same state a
        // rust-only CI image is in.
        .env("PATH", "")
        .output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.status.success(), "the refresh refuses: {text}");
    assert!(text.contains("E-LOCK-NATIVE"), "{text}");
    assert!(
        text.contains("uv lock --offline"),
        "the refusal names the exact argv: {text}"
    );
    // The decline never mutates the lockfile.
    let lock = std::fs::read_to_string(fx.root().join("uv.lock"))?;
    assert_eq!(lock, UV_LOCK);
    Ok(())
}

/// The success path: `apply` rewrites the manifests, reports the
/// `uv.lock` decline, refreshes natively through `uv lock --offline`
/// and verifies the effect — only the member's `version` row moved
/// (the observed uv fidelity); every other byte survives verbatim.
#[test]
fn the_native_refresh_moves_only_member_rows() -> Result<(), BoxError> {
    if !uv_available() {
        eprintln!("uv is not installed; the success-path e2e runs where the tool exists");
        return Ok(());
    }
    let fx = build_uv_workspace("refresh")?;
    let out = halyard(fx.root(), &["apply", "--format", "json"])?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "apply succeeded: {text}");
    let value: serde_json::Value = serde_json::from_str(
        text.trim_start_matches(char::is_whitespace)
            .trim_start_matches(|c: char| c != '{'),
    )
    .map_err(|e| format!("the report is JSON: {e}\n{text}"))?;
    // The stage row reports the decline; the native refresh row names
    // the moved member.
    let lockfiles = value
        .get("lockfiles")
        .and_then(serde_json::Value::as_array)
        .ok_or("lockfiles is an array")?;
    let lock = lockfiles
        .iter()
        .find(|row| row.get("file").and_then(serde_json::Value::as_str) == Some("uv.lock"))
        .ok_or("the uv.lock stage row is reported")?;
    assert_eq!(
        lock.get("native_refresh_required")
            .and_then(serde_json::Value::as_bool),
        Some(true),
        "{lock}"
    );
    let native = value
        .get("native_refresh")
        .and_then(serde_json::Value::as_array)
        .ok_or("native_refresh is an array")?;
    let row = native
        .iter()
        .find(|row| row.get("file").and_then(serde_json::Value::as_str) == Some("uv.lock"))
        .ok_or("the uv.lock refresh row is reported")?;
    assert_eq!(
        row.get("members_moved")
            .and_then(serde_json::Value::as_array),
        Some(&vec![serde_json::Value::from("app-a")]),
        "{row}"
    );
    // The lockfile on disk: the member row moved, uv's own bytes
    // everywhere else (the observed fidelity contract). The feat commit
    // rides the §10.2 0.x policy: the patch bump moves the member row.
    let after = std::fs::read_to_string(fx.root().join("uv.lock"))?;
    assert!(
        after.contains("name = \"app-a\"\nversion = \"0.1.1\""),
        "the member row moved: {after}"
    );
    assert!(
        after.contains("name = \"app-b\"\nversion = \"0.1.0\"\nsource = { editable = \"app_b\" }"),
        "sibling rows keep uv's bytes: {after}"
    );
    assert!(
        after.contains("source = { virtual = \".\" }"),
        "the root row keeps uv's bytes: {after}"
    );
    assert!(!after.contains("name = \"app-a\"\nversion = \"0.1.0\""));
    Ok(())
}
