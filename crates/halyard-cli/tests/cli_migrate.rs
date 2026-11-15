//! CLI `migrate` smoke tests (M4.7 step 1): the command surveys the
//! foreign release-tool configuration in the working directory and
//! prints the report — it reads, never writes, and needs no git repo
//! (the migration targets a repo that is not yet halyard-configured).

use std::process::Command;

type BoxError = Box<dyn std::error::Error>;

fn fixture_dir(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir = std::env::temp_dir().join(format!(
        "halyard-cli-migrate-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn halyard(root: &std::path::Path, args: &[&str]) -> Result<(bool, String), BoxError> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_halyard"));
    command.arg("-C").arg(root).args(args).env("NO_COLOR", "1");
    let out = command.output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Ok((out.status.success(), text))
}

#[test]
fn migrate_reports_a_release_please_workspace() -> Result<(), BoxError> {
    let dir = fixture_dir("report")?;
    std::fs::write(
        dir.join("release-please-config.json"),
        r#"{
  "packages": { "crates/p1": { "package-name": "p1-crate" } },
  "bump-minor-pre-major": true
}"#,
    )?;
    let args = ["migrate", "--format", "json"];
    let (ok, text) = halyard(&dir, &args)?;
    assert!(ok, "migrate exits 0 (the report is data): {text}");
    let value: serde_json::Value = serde_json::from_str(text.trim())
        .map_err(|e| format!("stdout is one JSON document: {e}\n{text}"))?;
    let tools = value
        .get("tools")
        .and_then(|t| t.as_array())
        .ok_or("the report carries a tools array")?;
    assert_eq!(tools.len(), 2, "every registered source surveys");
    let rp = tools
        .iter()
        .find(|t| t.get("tool").and_then(|v| v.as_str()) == Some("release-please"))
        .ok_or("the release-please survey is present")?;
    assert_eq!(rp.get("configured"), Some(&serde_json::Value::Bool(true)));
    assert!(
        rp.get("draft").and_then(|d| d.as_str()).is_some(),
        "a configured source drafts the config"
    );
    // Byte-stable JSON (I3): a second run prints the same document.
    let (ok2, text2) = halyard(&dir, &args)?;
    assert!(ok2);
    assert_eq!(text, text2, "the JSON report is byte-stable");
    Ok(())
}

#[test]
fn migrate_human_output_names_the_differences_and_the_draft() -> Result<(), BoxError> {
    let dir = fixture_dir("human")?;
    std::fs::write(
        dir.join("release-please-config.json"),
        r#"{ "packages": { "crates/p1": {} }, "plugins": ["node-workspace"] }"#,
    )?;
    let (ok, text) = halyard(&dir, &["migrate"])?;
    assert!(ok);
    assert!(
        text.contains("release-please"),
        "the tool name appears: {text}"
    );
    assert!(
        text.contains("plugins"),
        "the unmapped semantic difference is reported: {text}"
    );
    assert!(
        text.contains("halyard.toml"),
        "the draft block is labeled as a draft (nothing written): {text}"
    );
    assert!(
        text.contains("never wrote"),
        "the report states it wrote nothing: {text}"
    );
    Ok(())
}

#[test]
fn migrate_without_any_foreign_config_is_an_honest_report() -> Result<(), BoxError> {
    let dir = fixture_dir("empty")?;
    let (ok, text) = halyard(&dir, &["migrate"])?;
    assert!(ok, "absence is a value, not an error: {text}");
    assert!(
        text.contains("not detected"),
        "each registered tool reports not-detected: {text}"
    );
    Ok(())
}

#[test]
fn migrate_refuses_a_malformed_config_with_the_stable_code() -> Result<(), BoxError> {
    let dir = fixture_dir("malformed")?;
    std::fs::write(dir.join("release-please-config.json"), "{ not json")?;
    let (ok, text) = halyard(&dir, &["migrate", "--format", "json"])?;
    assert!(!ok, "a configured-but-unparseable file fails closed");
    let value: serde_json::Value = serde_json::from_str(text.trim())
        .map_err(|e| format!("the error path is one JSON document: {e}\n{text}"))?;
    assert_eq!(
        value
            .get("error")
            .and_then(|e| e.get("code"))
            .and_then(|c| c.as_str()),
        Some("E-CFG-MIGRATE"),
        "the stable code surfaces: {text}"
    );
    Ok(())
}

#[test]
fn migrate_gha_output_carries_the_survey_rows() -> Result<(), BoxError> {
    let dir = fixture_dir("gha")?;
    std::fs::write(
        dir.join("release-please-config.json"),
        r#"{ "packages": { "crates/p1": {} } }"#,
    )?;
    let (ok, text) = halyard(&dir, &["migrate", "--format", "gha"])?;
    assert!(ok);
    assert!(
        text.contains("::notice") || text.contains("::warning"),
        "gha annotations carry the rows: {text}"
    );
    Ok(())
}
