//! The §17 M6.4 stability promises (ADR-0055): "Stability promises:
//! CLI JSON, plan schema, exit codes, config semver policy —
//! `docs/stability.md`; schema-compat tests."
//!
//! The acceptance is two-sided and both halves live in this file's
//! gates plus `docs/stability.md`: the document is the human contract
//! (what a minor release may change, what it may not), and the tests
//! are the machine half — the surfaces the promise names are pinned
//! here so a break fails the build, not a downstream consumer:
//!
//! * the exit-code contract (0 success / 1 app error / 2 usage error),
//! * the JSON error envelope (`{"error":{"code","message"}}`),
//! * the `--format` vocabulary (`human|json|gha`),
//! * the `halyard-plan/v1` identity and its frozen schema document,
//! * the config loader's fail-closed unknown-key refusal (the config
//!   semver policy: unknown fields are never silently accepted).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use halyard_core::plan::PLAN_SCHEMA_V1;

type BoxError = Box<dyn std::error::Error>;

/// The repo root (the schema documents and the stability contract live
/// at their plan-declared paths, not inside any crate).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../")
}

/// Runs the built binary, returning (exit code, stdout, stderr).
fn halyard_run(args: &[&str], cwd: &Path) -> Result<(i32, String, String), BoxError> {
    let out = Command::new(env!("CARGO_BIN_EXE_halyard"))
        .args(args)
        .current_dir(cwd)
        .env("NO_COLOR", "1")
        .output()?;
    Ok((
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

/// An empty directory: the app-error surface outside any workspace.
fn empty_dir() -> Result<PathBuf, BoxError> {
    let dir = tempfile::tempdir()?;
    // The tempdir is deleted on drop; the tests run commands inside
    // the closure, so leak it deliberately (the suite is short-lived).
    let path = dir.path().to_path_buf();
    std::mem::forget(dir);
    Ok(path)
}

#[test]
fn the_stability_contract_document_exists_and_names_the_four_areas() -> Result<(), BoxError> {
    let text = fs::read_to_string(repo_root().join("docs/stability.md")).map_err(|e| {
        format!(
            "M6.4: the stability contract must exist at docs/stability.md (§17: \
             \"`docs/stability.md`; schema-compat tests\"): {e}"
        )
    })?;
    for area in [
        "CLI JSON",
        "plan schema",
        "exit codes",
        "config semver policy",
    ] {
        assert!(
            text.contains(area),
            "M6.4: docs/stability.md must name the stability area `{area}`"
        );
    }
    Ok(())
}

#[test]
fn exit_codes_are_the_documented_contract() -> Result<(), BoxError> {
    // 0: a successful run (`--help` never touches the workspace).
    let (code, _, _) = halyard_run(&["--help"], &repo_root())?;
    assert_eq!(code, 0, "`--help` exits 0");
    // 1: an application error — outside any repository, `status` hits
    // the git engine's open refusal, the CliError path.
    let dir = empty_dir()?;
    let (code, _, _) = halyard_run(&["status"], &dir)?;
    assert_eq!(code, 1, "an application error exits 1");
    // 2: a usage error — clap's own refusal for an unknown subcommand.
    let (code, _, _) = halyard_run(&["bogus-subcommand"], &repo_root())?;
    assert_eq!(code, 2, "a usage error exits 2 (clap's refusal)");
    Ok(())
}

#[test]
fn json_errors_ride_the_documented_envelope() -> Result<(), BoxError> {
    let dir = empty_dir()?;
    let (code, stdout, _) = halyard_run(&["--format", "json", "status"], &dir)?;
    assert_eq!(code, 1, "the JSON error path still exits 1");
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).map_err(|e| {
        format!("the JSON error path prints exactly one JSON document: {e} — got: {stdout}")
    })?;
    let envelope = value
        .get("error")
        .and_then(|error| error.as_object())
        .ok_or_else(|| format!("the envelope carries an `error` object — got: {stdout}"))?;
    let code_field = envelope
        .get("code")
        .and_then(|code| code.as_str())
        .ok_or_else(|| format!("the envelope carries a string `error.code` — got: {stdout}"))?;
    let message = envelope
        .get("message")
        .and_then(|message| message.as_str())
        .ok_or_else(|| format!("the envelope carries a string `error.message` — got: {stdout}"))?;
    assert!(
        code_field.starts_with("E-"),
        "the error code is the stable §15.4 form: {code_field}"
    );
    assert!(
        !message.is_empty(),
        "the error message names the condition and the fix"
    );
    Ok(())
}

#[test]
fn the_format_vocabulary_is_the_pinned_three() -> Result<(), BoxError> {
    let (_, stdout, _) = halyard_run(&["--help"], &repo_root())?;
    assert!(
        stdout.contains("human | json | gha"),
        "the §16.2 `--format` vocabulary is exactly human|json|gha"
    );
    Ok(())
}

#[test]
fn the_plan_schema_identity_is_pinned() -> Result<(), BoxError> {
    // The wire-format identity: the constant is the contract's name.
    assert_eq!(PLAN_SCHEMA_V1, "halyard-plan/v1");
    // The frozen schema document: the generated, checked-in contract
    // (M0.7's drift gate owns the regeneration; this gate owns the
    // identity — the document exists, is JSON, and names its
    // generator, so a hand edit or a deletion refuses here too).
    let path = repo_root().join("schemas/halyard-plan-v1.json");
    let text = fs::read_to_string(&path)
        .map_err(|e| format!("the plan schema must exist at schemas/halyard-plan-v1.json: {e}"))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("the plan schema parses as JSON: {e}"))?;
    let comment = value
        .get("$comment")
        .and_then(|comment| comment.as_str())
        .ok_or("the plan schema names its generator in `$comment`")?;
    assert!(
        comment.contains("schema-check"),
        "the plan schema is generated (`schema-check --write`), never hand-edited: {comment}"
    );
    let defs = value
        .get("$defs")
        .and_then(|defs| defs.as_object())
        .ok_or("the plan schema carries `$defs` (draft 2020-12)")?;
    assert!(
        !defs.is_empty(),
        "the plan schema's `$defs` carry the plan contract's types"
    );
    Ok(())
}

#[test]
fn config_unknown_keys_fail_closed() -> Result<(), BoxError> {
    // The config semver policy's machine half: an unknown key is
    // refused with the stable code, never silently accepted — a
    // forward-compatibility hole would let a renamed key drift a
    // release silently.
    let dir = empty_dir()?;
    fs::write(dir.join("halyard.toml"), "bogus-key = 1\n")?;
    let (code, stdout, _) = halyard_run(
        &["--format", "json", "--config", "halyard.toml", "plan"],
        &dir,
    )?;
    assert_eq!(code, 1, "the unknown-key refusal exits 1");
    let value: serde_json::Value = serde_json::from_str(stdout.trim())
        .map_err(|e| format!("the refusal prints the JSON envelope: {e} — got: {stdout}"))?;
    let code_field = value
        .pointer("/error/code")
        .and_then(|code| code.as_str())
        .ok_or_else(|| format!("the refusal carries `error.code` — got: {stdout}"))?;
    assert_eq!(
        code_field, "E-CFG-UNKNOWN-KEY",
        "the unknown-key refusal is the stable code (the config semver policy)"
    );
    Ok(())
}
