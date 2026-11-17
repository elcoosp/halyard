//! CLI `action` gates (§12 step 1, ADR-0037): the shell's forecast
//! mode renders the §12 outputs through the action-output seam. Pins
//! the two sink postures:
//!
//! * outside a workflow step (`GITHUB_OUTPUT` unset) the preview
//!   writer's call log is the output — the exact env-file entries a
//!   real step would receive, nothing written anywhere but the plan
//!   file;
//! * inside a workflow step (the env files set) the same calls land
//!   verbatim in `GITHUB_OUTPUT` / `GITHUB_STEP_SUMMARY`.
//!
//! Plus the `--format json` document shape and the `--plan-out`
//! contract (the `plan_json` output names the path the caller chose).

use std::fs;
use std::path::Path;
use std::process::Command;

use halyard_test_support::fixture::{CommitSpec, GitFixture};
use tempfile::TempDir;

type BoxError = Box<dyn std::error::Error>;

/// Runs the binary; returns (exit ok, combined output).
fn halyard(root: &Path, args: &[&str]) -> Result<(bool, String), BoxError> {
    halyard_with_env(root, args, |cmd| {
        cmd.env_remove("GITHUB_OUTPUT")
            .env_remove("GITHUB_STEP_SUMMARY");
    })
}

/// Runs the binary with an env hook (the sink postures are gated by
/// what the child sees — the test process never mutates its own
/// environment).
fn halyard_with_env(
    root: &Path,
    args: &[&str],
    env: impl FnOnce(&mut Command),
) -> Result<(bool, String), BoxError> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_halyard"));
    cmd.arg("-C").arg(root).args(args).env("NO_COLOR", "1");
    env(&mut cmd);
    let out = cmd.output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Ok((out.status.success(), text))
}

fn parse_json(text: &str) -> Result<serde_json::Value, BoxError> {
    serde_json::from_str(text.trim())
        .map_err(|e| format!("stdout is one JSON document: {e}\n{text}").into())
}

/// A two-package cargo workspace (`lib` ← `app`): the scaffold commit
/// touches both packages, then one commit touches only `lib`. Both
/// packages released at the scaffold; the lib patch is pending.
fn two_package_fixture(label: &str) -> Result<GitFixture, BoxError> {
    let mut fx = GitFixture::new(label)?;
    let scaffold = vec![
        (
            "Cargo.toml".to_owned(),
            "[workspace]\nresolver = \"2\"\nmembers = [\"crates/lib\", \"crates/app\"]\n".to_owned(),
        ),
        (
            "crates/lib/Cargo.toml".to_owned(),
            "[package]\nname = \"lib\"\nversion = \"0.1.0\"\nedition = \"2021\"\n".to_owned(),
        ),
        (
            "crates/lib/src/lib.rs".to_owned(),
            "pub const L: usize = 1;\n".to_owned(),
        ),
        (
            "crates/app/Cargo.toml".to_owned(),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nlib = { version = \"0.1\", path = \"../lib\" }\n".to_owned(),
        ),
        (
            "crates/app/src/lib.rs".to_owned(),
            "pub const A: usize = 2;\n".to_owned(),
        ),
    ];
    fx.commit_with(&CommitSpec::files(scaffold, "feat: scaffold workspace"))?;
    fx.write("crates/lib/src/lib.rs", "pub const L: usize = 2;\n")?;
    fx.stage_all()?;
    fx.commit("fix(lib): patch")?;
    fx.run_git(&["tag", "-a", "lib-v0.1.0", "-m", "lib 0.1.0", "HEAD~1"])?;
    fx.run_git(&["tag", "-a", "app-v0.1.0", "-m", "app 0.1.0", "HEAD~1"])?;
    Ok(fx)
}

#[test]
fn action_forecasts_through_the_preview_outside_a_workflow() -> Result<(), BoxError> {
    let fx = two_package_fixture("cli-action-preview")?;
    let (ok, text) = halyard(fx.root(), &["action"])?;
    assert!(ok, "the forecast is the honest happy path: {text}");
    // The §12 scalar outputs, in the documented order.
    assert!(text.contains("released=false\n"), "the scalar: {text}");
    assert!(text.contains("published=false\n"), "the scalar: {text}");
    assert!(
        text.contains("plan_json=halyard-plan.json\n"),
        "the plan-file pointer: {text}"
    );
    // The planned release rides the row grammar (lib is the pending
    // release; app's requirement stays satisfied). The rows parse from
    // the exact string a real step would receive (object key order is
    // the serializer's — the fields are the contract).
    assert!(text.contains("releases_json=[{"), "the row: {text}");
    let rows_value = text
        .lines()
        .find_map(|line| line.strip_prefix("releases_json="))
        .ok_or("the releases_json entry is printed")?;
    let rows: serde_json::Value = serde_json::from_str(rows_value)?;
    let row = |key: &str| -> Option<&serde_json::Value> {
        rows.as_array()
            .and_then(|items| items.first())
            .and_then(|value| value.get(key))
    };
    assert_eq!(row("name"), Some(&serde_json::json!("lib")));
    assert_eq!(row("version"), Some(&serde_json::json!("0.1.1")));
    assert_eq!(row("tag"), Some(&serde_json::json!("lib-v0.1.1")));
    assert_eq!(row("ecosystem"), Some(&serde_json::json!("cargo")));
    assert_eq!(row("url"), Some(&serde_json::json!("")));
    // The per-package keys.
    assert!(text.contains("lib--version=0.1.1\n"), "the keys: {text}");
    assert!(text.contains("lib--tag=lib-v0.1.1\n"), "the keys: {text}");
    // The job summary landed in the preview log.
    assert!(
        text.contains("# >> $GITHUB_STEP_SUMMARY"),
        "the summary marker: {text}"
    );
    assert!(
        text.contains("## halyard plan"),
        "the summary heading: {text}"
    );
    // The plan file is written (the `plan_json` pointer's target).
    let document = fs::read_to_string(fx.root().join("halyard-plan.json"))?;
    let value: serde_json::Value = serde_json::from_str(&document)?;
    assert_eq!(
        value.get("schema"),
        Some(&serde_json::json!("halyard-plan/v1"))
    );
    let releases = value.get("releases").and_then(serde_json::Value::as_array);
    assert_eq!(releases.map(Vec::len), Some(1));
    Ok(())
}

#[test]
fn action_writes_the_env_files_inside_a_workflow_step() -> Result<(), BoxError> {
    let fx = two_package_fixture("cli-action-env-files")?;
    let sinks = TempDir::new()?;
    let outputs = sinks.path().join("output.txt");
    let summary = sinks.path().join("summary.md");
    let (ok, text) = halyard_with_env(fx.root(), &["action"], |cmd| {
        cmd.env("GITHUB_OUTPUT", &outputs)
            .env("GITHUB_STEP_SUMMARY", &summary);
    })?;
    assert!(ok, "the in-step run succeeds: {text}");
    // The outputs landed verbatim.
    let outputs_text = fs::read_to_string(&outputs)?;
    assert!(
        outputs_text.starts_with("released=false\n"),
        "the sink: {outputs_text}"
    );
    assert!(
        outputs_text.contains("releases_json=["),
        "the rows: {outputs_text}"
    );
    // The job summary landed (the §12 heading + the release row).
    let summary_text = fs::read_to_string(&summary)?;
    assert!(
        summary_text.contains("## halyard plan"),
        "the summary: {summary_text}"
    );
    assert!(
        summary_text.contains("| lib | 0.1.0 → 0.1.1 | patch | lib-v0.1.1 |"),
        "the row: {summary_text}"
    );
    // The run reports the sinks it wrote.
    assert!(text.contains("GITHUB_OUTPUT"), "the confirmation: {text}");
    Ok(())
}

#[test]
fn action_forecast_json_document_is_the_output_map() -> Result<(), BoxError> {
    let fx = two_package_fixture("cli-action-json")?;
    let (ok, text) = halyard(fx.root(), &["action", "--format", "json"])?;
    assert!(ok, "the JSON run succeeds: {text}");
    let value = parse_json(&text)?;
    let outputs = value
        .get("outputs")
        .and_then(serde_json::Value::as_object)
        .ok_or("the document carries the outputs map")?;
    assert_eq!(outputs.get("released"), Some(&serde_json::json!("false")));
    assert_eq!(
        outputs.get("plan_json"),
        Some(&serde_json::json!("halyard-plan.json"))
    );
    let rows = outputs
        .get("releases_json")
        .and_then(serde_json::Value::as_str)
        .ok_or("releases_json rides as the string a real step would receive")?;
    let parsed: serde_json::Value = serde_json::from_str(rows)?;
    let row = |key: &str| -> Option<&serde_json::Value> {
        parsed
            .as_array()
            .and_then(|items| items.first())
            .and_then(|value| value.get(key))
    };
    assert_eq!(row("name"), Some(&serde_json::json!("lib")));
    assert_eq!(row("version"), Some(&serde_json::json!("0.1.1")));
    assert_eq!(
        outputs.get("lib--version"),
        Some(&serde_json::json!("0.1.1"))
    );
    Ok(())
}

#[test]
fn action_plan_out_names_the_written_plan_file() -> Result<(), BoxError> {
    let fx = two_package_fixture("cli-action-plan-out")?;
    let target = fx.root().join("forecast").join("plan.json");
    // The parent directory does not exist — the plan file refuses
    // fail-closed (the caller's contract, never invented).
    let (ok, text) = halyard(fx.root(), &["action", "--plan-out", "forecast/plan.json"])?;
    assert!(!ok, "the missing parent refuses: {text}");
    assert!(text.contains("E-ACTION-PLAN"), "the code: {text}");

    fs::create_dir_all(fx.root().join("forecast"))?;
    let (ok, text) = halyard(fx.root(), &["action", "--plan-out", "forecast/plan.json"])?;
    assert!(ok, "the chosen path is honored: {text}");
    assert!(
        text.contains("plan_json=forecast/plan.json"),
        "the pointer follows the flag: {text}"
    );
    let document = fs::read_to_string(&target)?;
    let value: serde_json::Value = serde_json::from_str(&document)?;
    assert_eq!(
        value.get("schema"),
        Some(&serde_json::json!("halyard-plan/v1"))
    );
    Ok(())
}
