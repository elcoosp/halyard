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

// ---------------------------------------------------------------------------
// The §12 `pr` mode (ADR-0043): the §11.1 machine pass — driven over the
// preview adapter in dry mode — rendered through the action-output seam.
// The Release-PR keys ride the forge's own report values (number 0 and
// `preview://` URLs mark a forecast: a real forge can produce neither).
// ---------------------------------------------------------------------------

/// A released workspace whose manifest was bumped post-merge (solo
/// crate, v0.1.0 tagged, manifest + changelog at 0.1.1): the tag phase
/// owes v0.1.1 and PROPAGATE defers the Release PR (§11.1 step 2).
fn merged_manifest_fixture(label: &str) -> Result<GitFixture, BoxError> {
    let mut fx = GitFixture::new(label)?;
    let scaffold = vec![
        (
            "Cargo.toml".to_owned(),
            "[package]\nname = \"solo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n".to_owned(),
        ),
        (
            "src/lib.rs".to_owned(),
            "pub const C: usize = 1;\n".to_owned(),
        ),
    ];
    fx.commit_with(&CommitSpec::files(scaffold, "feat: scaffold the crate"))?;
    fx.run_git(&["tag", "-a", "v0.1.0", "-m", "solo 0.1.0"])?;
    fx.write(
        "Cargo.toml",
        "[package]\nname = \"solo\"\nversion = \"0.1.1\"\nedition = \"2021\"\n",
    )?;
    fx.write(
        "CHANGELOG.md",
        "# Changelog\n\n## [0.1.1]\n\n### Features\n\n- scaffold the crate\n",
    )?;
    fx.stage_all()?;
    fx.commit("chore(main): release v0.1.1")?;
    Ok(fx)
}

#[test]
fn action_pr_mode_forecasts_the_release_pr_through_the_seam() -> Result<(), BoxError> {
    let fx = two_package_fixture("cli-action-pr-forecast")?;
    let (ok, text) = halyard(fx.root(), &["action", "--mode", "pr", "--dry"])?;
    assert!(ok, "the dry forecast succeeds: {text}");
    // The §12 scalars: the pr mode executes nothing in dry mode.
    assert!(text.contains("released=false\n"), "the scalar: {text}");
    assert!(text.contains("published=false\n"), "the scalar: {text}");
    // The Release-PR keys ride the forge's own report values — the
    // preview's number 0 + preview:// URL mark the forecast. The
    // branch is `halyard/release/<base>` over the plan's base_branch,
    // which stays `HEAD` until a live shell resolves the remote HEAD
    // (the planner's documented posture, pinned by cli_pr too).
    assert!(text.contains("pr_number=0\n"), "the PR number: {text}");
    assert!(
        text.contains("pr_url=preview://halyard/release/HEAD\n"),
        "the forge's own URL: {text}"
    );
    // The plan vocabulary rides unchanged (the plan file, the rows, the
    // per-package keys).
    assert!(
        text.contains("plan_json=halyard-plan.json\n"),
        "the plan-file pointer: {text}"
    );
    assert!(text.contains("lib--version=0.1.1\n"), "the keys: {text}");
    assert!(text.contains("lib--tag=lib-v0.1.1\n"), "the keys: {text}");
    // The job summary names the pass and its posture.
    assert!(
        text.contains("## halyard release PR"),
        "the summary heading: {text}"
    );
    assert!(
        text.contains("PR #0 opened (preview://halyard/release/HEAD)"),
        "the pass line: {text}"
    );
    assert!(
        text.contains("forecast (nothing executed)"),
        "the forecast posture: {text}"
    );
    // The plan file is written (the `plan_json` pointer's target).
    let document = fs::read_to_string(fx.root().join("halyard-plan.json"))?;
    let value: serde_json::Value = serde_json::from_str(&document)?;
    assert_eq!(
        value.get("schema"),
        Some(&serde_json::json!("halyard-plan/v1"))
    );
    Ok(())
}

#[test]
fn action_pr_mode_propagate_forecasts_the_tag_phase() -> Result<(), BoxError> {
    let fx = merged_manifest_fixture("cli-action-pr-propagate")?;
    let (ok, text) = halyard(fx.root(), &["action", "--mode", "pr", "--dry"])?;
    assert!(ok, "the dry forecast succeeds: {text}");
    // The tag phase owned the pass: no Release PR keys at all.
    assert!(
        !text.contains("pr_number="),
        "PROPAGATE defers the Release PR: {text}"
    );
    // `released` stays honestly false — the dry run executed nothing.
    assert!(text.contains("released=false\n"), "the scalar: {text}");
    // `releases_json` rides the plan vocabulary (ADR-0037's contract):
    // the plan's next-release forecast row — the merged manifest makes
    // this the post-release forecast (0.1.2), while the tag phase minted
    // the manifest version (v0.1.1); the plan row's url stays empty
    // because the minted tag is not the forecast row's tag.
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
    assert_eq!(row("name"), Some(&serde_json::json!("solo")));
    assert_eq!(row("version"), Some(&serde_json::json!("0.1.2")));
    assert_eq!(row("tag"), Some(&serde_json::json!("v0.1.2")));
    assert_eq!(row("url"), Some(&serde_json::json!("")));
    // The summary names the PROPAGATE posture and the pass's own mint —
    // the manifest version, in the forecast's preview:// shape.
    assert!(
        text.contains("tag phase owns this pass"),
        "the posture line: {text}"
    );
    assert!(
        text.contains("minted v0.1.1 (preview://v0.1.1)"),
        "the minted release is named: {text}"
    );
    Ok(())
}

#[test]
fn action_pr_mode_writes_the_env_files_inside_a_workflow_step() -> Result<(), BoxError> {
    let fx = two_package_fixture("cli-action-pr-env-files")?;
    let sinks = TempDir::new()?;
    let outputs = sinks.path().join("output.txt");
    let summary = sinks.path().join("summary.md");
    let (ok, text) = halyard_with_env(fx.root(), &["action", "--mode", "pr", "--dry"], |cmd| {
        cmd.env("GITHUB_OUTPUT", &outputs)
            .env("GITHUB_STEP_SUMMARY", &summary);
    })?;
    assert!(ok, "the in-step run succeeds: {text}");
    let outputs_text = fs::read_to_string(&outputs)?;
    assert!(
        outputs_text.contains("pr_number=0\n"),
        "the sink: {outputs_text}"
    );
    assert!(
        outputs_text.contains("pr_url=preview://halyard/release/HEAD\n"),
        "the sink: {outputs_text}"
    );
    let summary_text = fs::read_to_string(&summary)?;
    assert!(
        summary_text.contains("## halyard release PR"),
        "the summary: {summary_text}"
    );
    assert!(text.contains("GITHUB_OUTPUT"), "the confirmation: {text}");
    Ok(())
}

#[test]
fn action_pr_mode_json_document_carries_the_pass() -> Result<(), BoxError> {
    let fx = two_package_fixture("cli-action-pr-json")?;
    let (ok, text) = halyard(
        fx.root(),
        &["action", "--mode", "pr", "--dry", "--format", "json"],
    )?;
    assert!(ok, "the JSON run succeeds: {text}");
    let value = parse_json(&text)?;
    assert_eq!(value.get("mode"), Some(&serde_json::json!("pr")));
    assert_eq!(value.get("dry"), Some(&serde_json::json!(true)));
    assert_eq!(
        value.get("branch"),
        Some(&serde_json::json!("halyard/release/HEAD"))
    );
    let pr = value
        .get("pr")
        .and_then(serde_json::Value::as_object)
        .ok_or("the document carries the Release PR view")?;
    assert_eq!(pr.get("number"), Some(&serde_json::json!(0)));
    assert_eq!(
        pr.get("url"),
        Some(&serde_json::json!("preview://halyard/release/HEAD"))
    );
    let outputs = value
        .get("outputs")
        .and_then(serde_json::Value::as_object)
        .ok_or("the document carries the outputs map")?;
    assert_eq!(outputs.get("pr_number"), Some(&serde_json::json!("0")));
    assert_eq!(outputs.get("released"), Some(&serde_json::json!("false")));
    Ok(())
}

// ---------------------------------------------------------------------------
// The §12 `check` mode (ADR-0044): the check surface's verdict — the
// squash-merge title lint plus the per-package conveyed bumps — through
// the action-output seam. A rejected title refuses fail-closed before
// anything is written (the §11.6 rule); without a title the pending
// forecast is the verdict.
// ---------------------------------------------------------------------------

#[test]
fn action_check_mode_lints_the_title_and_conveys_the_bumps() -> Result<(), BoxError> {
    let fx = two_package_fixture("cli-action-check-accepted")?;
    let (ok, text) = halyard(
        fx.root(),
        &[
            "action",
            "--mode",
            "check",
            "--pr-title",
            "feat: add the widget",
        ],
    )?;
    assert!(ok, "the accepted verdict succeeds: {text}");
    // The plan vocabulary rides unchanged (the merge forecast).
    assert!(text.contains("released=false\n"), "the scalar: {text}");
    assert!(
        text.contains("plan_json=halyard-plan.json\n"),
        "the plan-file pointer: {text}"
    );
    assert!(text.contains("lib--version=0.1.1\n"), "the keys: {text}");
    // The summary names the verdict and the conveyed bumps — the §10.2
    // pre-major remap teaches its own cause (0.1.y: feat conveys patch).
    assert!(
        text.contains("## halyard check"),
        "the summary heading: {text}"
    );
    assert!(
        text.contains("title accepted: feat → minor"),
        "the verdict line: {text}"
    );
    assert!(
        text.contains("conveyed lib patch"),
        "the conveyed bump (pre-major remapped): {text}"
    );
    Ok(())
}

#[test]
fn action_check_mode_refuses_a_rejected_title_fail_closed() -> Result<(), BoxError> {
    let fx = two_package_fixture("cli-action-check-rejected")?;
    let (ok, text) = halyard(
        fx.root(),
        &[
            "action",
            "--mode",
            "check",
            "--pr-title",
            "not a conventional title",
            "--format",
            "json",
        ],
    )?;
    assert!(!ok, "the rejected title fails the step: {text}");
    // The JSON error document carries the stable §15.4 code (the same
    // surface the `check` command's rejections ride).
    let error = parse_json(&text)?;
    let code = error
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(serde_json::Value::as_str)
        .ok_or("the JSON error document carries the code")?;
    assert_eq!(code, "E-CLI-TITLE");
    // Fail-closed ordering: nothing was written — no plan file, no
    // sink call (the classification precedes every effect).
    assert!(
        !fx.root().join("halyard-plan.json").exists(),
        "the plan file is written only after the verdict"
    );
    Ok(())
}

#[test]
fn action_check_mode_without_a_title_is_the_pending_forecast() -> Result<(), BoxError> {
    let fx = two_package_fixture("cli-action-check-pending")?;
    let (ok, text) = halyard(fx.root(), &["action", "--mode", "check"])?;
    assert!(ok, "the pending forecast succeeds: {text}");
    assert!(
        text.contains("no title to lint — the pending forecast is the verdict"),
        "the no-title posture: {text}"
    );
    assert!(
        text.contains("plan_json=halyard-plan.json\n"),
        "the forecast rides the plan vocabulary: {text}"
    );
    Ok(())
}

#[test]
fn action_check_mode_json_document_carries_the_verdict() -> Result<(), BoxError> {
    let fx = two_package_fixture("cli-action-check-json")?;
    let (ok, text) = halyard(
        fx.root(),
        &[
            "action",
            "--mode",
            "check",
            "--pr-title",
            "fix: patch the lib",
            "--format",
            "json",
        ],
    )?;
    assert!(ok, "the JSON run succeeds: {text}");
    let value = parse_json(&text)?;
    assert_eq!(value.get("mode"), Some(&serde_json::json!("check")));
    assert_eq!(value.get("verdict"), Some(&serde_json::json!("accepted")));
    assert_eq!(
        value.get("pr_title"),
        Some(&serde_json::json!("fix: patch the lib"))
    );
    let outputs = value
        .get("outputs")
        .and_then(serde_json::Value::as_object)
        .ok_or("the document carries the outputs map")?;
    assert_eq!(outputs.get("released"), Some(&serde_json::json!("false")));
    Ok(())
}

// ---------------------------------------------------------------------------
// The §12 `release` mode (ADR-0046): the §11.1 tag phase's pass view —
// reconcile → directives → the writer → the push → the forge half —
// through the action-output seam. Dry forecasts offline (I5); no
// `pr_number`/`pr_url` keys exist (the release mode opens no Release
// PR). Written first (red on the unknown `--mode release` value),
// implemented second.
// ---------------------------------------------------------------------------

#[test]
fn action_release_mode_forecasts_the_tag_phase() -> Result<(), BoxError> {
    // The manifest-ahead shape (solo at 0.1.1, tagged v0.1.0): the
    // §11.1 reconciliation makes the manifest version pending, so the
    // dry forecast has a would-create tag to name. The two-package
    // fully-tagged shape forecasts zero creations — the honest
    // `no tags created` posture instead.
    let fx = merged_manifest_fixture("cli-action-release-forecast")?;
    let (ok, text) = halyard(fx.root(), &["action", "--mode", "release", "--dry"])?;
    assert!(ok, "the dry forecast succeeds: {text}");
    // The §12 scalars: the release mode executes nothing in dry mode,
    // and no Release-PR keys exist (the release mode opens none).
    assert!(text.contains("released=false\n"), "the scalar: {text}");
    assert!(text.contains("published=false\n"), "the scalar: {text}");
    assert!(
        !text.contains("pr_number="),
        "the release mode has no Release-PR keys: {text}"
    );
    // The plan vocabulary rides unchanged (the plan file, the pending
    // forecast's per-package keys — the plan's own next-release row,
    // which the tag phase's manifest-version tag deliberately differs
    // from, the same two-half distinction the `pr` mode renders).
    assert!(
        text.contains("plan_json=halyard-plan.json\n"),
        "the plan-file pointer: {text}"
    );
    assert!(text.contains("solo--version=0.1.2\n"), "the keys: {text}");
    assert!(text.contains("solo--tag=v0.1.2\n"), "the keys: {text}");
    // The job summary names the pass and its posture: the would-create
    // tag (the preview writer's forecast row — the manifest's own
    // version), the forge half's honest skip and the forecast marker.
    assert!(text.contains("## halyard release"), "the heading: {text}");
    assert!(
        text.contains("created v0.1.1"),
        "the forecast's created tag is named: {text}"
    );
    assert!(
        text.contains("forge releases skipped (dry run"),
        "the forge half's honest skip: {text}"
    );
    assert!(
        text.contains("forecast (nothing executed)"),
        "the forecast posture: {text}"
    );
    // The plan file is written (the `plan_json` pointer's target).
    let document = fs::read_to_string(fx.root().join("halyard-plan.json"))?;
    let value: serde_json::Value = serde_json::from_str(&document)?;
    assert_eq!(
        value.get("schema"),
        Some(&serde_json::json!("halyard-plan/v1"))
    );
    Ok(())
}

#[test]
fn action_release_mode_fully_tagged_shape_forecasts_no_creation() -> Result<(), BoxError> {
    // The fully-tagged shape (manifest == tags): the §11.1
    // reconciliation makes everything Tagged — the release pass
    // forecasts zero creations and says so, never a invented tag.
    let fx = two_package_fixture("cli-action-release-noop")?;
    let (ok, text) = halyard(fx.root(), &["action", "--mode", "release", "--dry"])?;
    assert!(ok, "the dry forecast succeeds: {text}");
    assert!(text.contains("released=false\n"), "the scalar: {text}");
    assert!(
        text.contains("no tags created"),
        "the honest no-creation posture: {text}"
    );
    assert!(
        text.contains("forecast (nothing executed)"),
        "the forecast posture: {text}"
    );
    Ok(())
}

#[test]
fn action_release_mode_json_document_carries_the_pass() -> Result<(), BoxError> {
    let fx = two_package_fixture("cli-action-release-json")?;
    let (ok, text) = halyard(
        fx.root(),
        &["action", "--mode", "release", "--dry", "--format", "json"],
    )?;
    assert!(ok, "the JSON run succeeds: {text}");
    let value = parse_json(&text)?;
    assert_eq!(value.get("mode"), Some(&serde_json::json!("release")));
    assert_eq!(value.get("dry"), Some(&serde_json::json!(true)));
    let outputs = value
        .get("outputs")
        .and_then(serde_json::Value::as_object)
        .ok_or("the document carries the outputs map")?;
    assert_eq!(outputs.get("released"), Some(&serde_json::json!("false")));
    assert_eq!(outputs.get("published"), Some(&serde_json::json!("false")));
    assert!(outputs.get("plan_json").is_some());
    // Byte-stable I3: the same run renders the same document.
    let (ok2, text2) = halyard(
        fx.root(),
        &["action", "--mode", "release", "--dry", "--format", "json"],
    )?;
    assert!(ok2, "the re-run succeeds: {text2}");
    assert_eq!(text, text2, "the release JSON document is byte-stable");
    Ok(())
}

// ---------------------------------------------------------------------------
// The §12 `auto` mode (ADR-0047): the §11.1 stateless reconciliation as
// one §12 mode — the composition policy over the two shipped executors
// (the release executor's git-side tag effects ride first, the §11.1
// machine pass then owns the half selection through its own PROPAGATE
// rule). The plan pins the mode in §12's input list (`mode`
// (`auto|pr|release|plan|check)`, docs/HALYARD_ARCHITECTURE.md §12) and
// PROGRESS.md's next-session start point names it ("the §12 `auto` mode
// (the composition policy over the two shipped executors)") — ADR-0046
// declared it "absent rather than stubbed". Written first (red on the
// unknown `--mode auto` value), implemented second.
// ---------------------------------------------------------------------------

#[test]
fn action_auto_mode_propagate_forecasts_the_release_half() -> Result<(), BoxError> {
    // The merged-manifest shape (solo at 0.1.1, tagged v0.1.0): the
    // §11.1 reconciliation makes the manifest version pending, so the
    // tag phase owns the pass — the forecast names the would-create
    // tag, mints the preview forge releases and defers the Release PR
    // (§11.1 step 2: "PROPAGATE to step 3 only if nothing pending").
    let fx = merged_manifest_fixture("cli-action-auto-propagate")?;
    let (ok, text) = halyard(fx.root(), &["action", "--mode", "auto", "--dry"])?;
    assert!(ok, "the dry forecast succeeds: {text}");
    // The §12 scalars: the auto mode executes nothing in dry mode.
    assert!(text.contains("released=false\n"), "the scalar: {text}");
    assert!(text.contains("published=false\n"), "the scalar: {text}");
    // PROPAGATE: the Release PR is not touched, no PR keys exist.
    assert!(
        !text.contains("pr_number="),
        "PROPAGATE defers the Release PR: {text}"
    );
    // The release half's forecast: the would-create tag is named (the
    // preview writer's row) and the machine's mints ride the report.
    assert!(
        text.contains("created v0.1.1"),
        "the forecast's created tag is named: {text}"
    );
    assert!(
        text.contains("minted v0.1.1 (preview://v0.1.1)"),
        "the machine's forecast mint: {text}"
    );
    assert!(
        text.contains("tag phase owns this pass"),
        "the PROPAGATE posture: {text}"
    );
    assert!(
        text.contains("forge releases skipped (dry run"),
        "the forge half's honest skip: {text}"
    );
    assert!(
        text.contains("forecast (nothing executed)"),
        "the forecast posture: {text}"
    );
    // The plan file is written (the `plan_json` pointer's target).
    let document = fs::read_to_string(fx.root().join("halyard-plan.json"))?;
    let value: serde_json::Value = serde_json::from_str(&document)?;
    assert_eq!(
        value.get("schema"),
        Some(&serde_json::json!("halyard-plan/v1"))
    );
    Ok(())
}

#[test]
fn action_auto_mode_prepare_forecasts_the_release_pr() -> Result<(), BoxError> {
    // The two-package shape (manifests == tags, the lib patch pending in
    // history): nothing is pending — the machine reaches the prepare
    // phase and forecasts the Release PR (§11.1 step 3), while the tag
    // phase honestly forecasts zero creations.
    let fx = two_package_fixture("cli-action-auto-prepare")?;
    let (ok, text) = halyard(fx.root(), &["action", "--mode", "auto", "--dry"])?;
    assert!(ok, "the dry forecast succeeds: {text}");
    assert!(text.contains("released=false\n"), "the scalar: {text}");
    assert!(
        text.contains("no tags created"),
        "the honest no-creation posture: {text}"
    );
    // The prepare half's forecast: the forge's own PR values (number 0
    // + preview:// URL mark the forecast; a real forge can produce
    // neither).
    assert!(text.contains("pr_number=0\n"), "the PR number: {text}");
    assert!(
        text.contains("pr_url=preview://halyard/release/HEAD\n"),
        "the forge's own URL: {text}"
    );
    assert!(
        text.contains("PR #0 opened (preview://halyard/release/HEAD)"),
        "the pass line: {text}"
    );
    assert!(
        text.contains("## halyard auto"),
        "the summary heading: {text}"
    );
    assert!(
        text.contains("forecast (nothing executed)"),
        "the forecast posture: {text}"
    );
    Ok(())
}

#[test]
fn action_auto_mode_json_document_carries_the_pass() -> Result<(), BoxError> {
    let fx = two_package_fixture("cli-action-auto-json")?;
    let (ok, text) = halyard(
        fx.root(),
        &["action", "--mode", "auto", "--dry", "--format", "json"],
    )?;
    assert!(ok, "the JSON run succeeds: {text}");
    let value = parse_json(&text)?;
    assert_eq!(value.get("mode"), Some(&serde_json::json!("auto")));
    assert_eq!(value.get("dry"), Some(&serde_json::json!(true)));
    assert_eq!(value.get("propagated"), Some(&serde_json::json!(false)));
    assert_eq!(value.get("created_tags"), Some(&serde_json::json!([])));
    let outputs = value
        .get("outputs")
        .and_then(serde_json::Value::as_object)
        .ok_or("the document carries the outputs map")?;
    assert_eq!(outputs.get("released"), Some(&serde_json::json!("false")));
    assert_eq!(outputs.get("published"), Some(&serde_json::json!("false")));
    assert!(outputs.get("plan_json").is_some());
    // Byte-stable I3: the same run renders the same document.
    let (ok2, text2) = halyard(
        fx.root(),
        &["action", "--mode", "auto", "--dry", "--format", "json"],
    )?;
    assert!(ok2, "the re-run succeeds: {text2}");
    assert_eq!(text, text2, "the auto JSON document is byte-stable");
    Ok(())
}

#[test]
fn action_auto_mode_writes_the_env_files_inside_a_workflow_step() -> Result<(), BoxError> {
    let fx = two_package_fixture("cli-action-auto-env-files")?;
    let sinks = TempDir::new()?;
    let outputs = sinks.path().join("output.txt");
    let summary = sinks.path().join("summary.md");
    let (ok, text) = halyard_with_env(fx.root(), &["action", "--mode", "auto", "--dry"], |cmd| {
        cmd.env("GITHUB_OUTPUT", &outputs)
            .env("GITHUB_STEP_SUMMARY", &summary);
    })?;
    assert!(ok, "the in-step run succeeds: {text}");
    // The outputs landed verbatim — the prepare half's PR keys among
    // them.
    let outputs_text = fs::read_to_string(&outputs)?;
    assert!(
        outputs_text.contains("pr_number=0\n"),
        "the sink: {outputs_text}"
    );
    assert!(
        outputs_text.contains("pr_url=preview://halyard/release/HEAD\n"),
        "the sink: {outputs_text}"
    );
    // The job summary landed (the §12 auto heading + the release row).
    let summary_text = fs::read_to_string(&summary)?;
    assert!(
        summary_text.contains("## halyard auto"),
        "the summary: {summary_text}"
    );
    assert!(
        summary_text.contains("| lib | 0.1.0 → 0.1.1 | patch | lib-v0.1.1 |"),
        "the row: {summary_text}"
    );
    assert!(text.contains("GITHUB_OUTPUT"), "the confirmation: {text}");
    Ok(())
}
