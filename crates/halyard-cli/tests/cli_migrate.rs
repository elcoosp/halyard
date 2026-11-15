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
    assert_eq!(tools.len(), 4, "every registered source surveys");
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
fn migrate_surveys_the_knope_and_release_plz_configs() -> Result<(), BoxError> {
    let dir = fixture_dir("knope-release-plz")?;
    std::fs::write(
        dir.join("knope.toml"),
        "[packages.\"left\"]\nversioned_files = [\"left/Cargo.toml\"]\n",
    )?;
    std::fs::write(
        dir.join("release-plz.toml"),
        "[workspace]\n\n[[package]]\nname = \"left\"\nrelease = false\n",
    )?;
    let (ok, text) = halyard(&dir, &["migrate", "--format", "json"])?;
    assert!(
        ok,
        "both §16.2 extensions survey like the older tools: {text}"
    );
    let value: serde_json::Value = serde_json::from_str(text.trim())
        .map_err(|e| format!("stdout is one JSON document: {e}\n{text}"))?;
    let tools = value
        .get("tools")
        .and_then(|t| t.as_array())
        .ok_or("the report carries a tools array")?;
    let knope = tools
        .iter()
        .find(|t| t.get("tool").and_then(|v| v.as_str()) == Some("knope"))
        .ok_or("the knope survey is present")?;
    assert_eq!(
        knope.get("configured"),
        Some(&serde_json::Value::Bool(true))
    );
    assert!(
        knope.to_string().contains("{name}/v{version}"),
        "the documented multi-package tag shape rides the report: {knope}"
    );
    let plz = tools
        .iter()
        .find(|t| t.get("tool").and_then(|v| v.as_str()) == Some("release-plz"))
        .ok_or("the release-plz survey is present")?;
    assert_eq!(plz.get("configured"), Some(&serde_json::Value::Bool(true)));
    assert!(
        plz.to_string().contains("keep-a-changelog"),
        "the documented changelog default rides the report: {plz}"
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

// ---------------------------------------------------------------------------
// `--write` (M4.7 step 2, §16.2): the validated draft lands at
// `halyard.toml` — atomic, fail-closed on every guess.
// ---------------------------------------------------------------------------

/// Writes the release-please fixture the write tests migrate.
fn write_release_please_fixture(dir: &std::path::Path) -> Result<(), BoxError> {
    std::fs::write(
        dir.join("release-please-config.json"),
        r#"{
  "packages": { "crates/p1": { "package-name": "p1-crate" } },
  "bump-minor-pre-major": true,
  "include-v-in-tag": false
}"#,
    )?;
    Ok(())
}

#[test]
fn migrate_write_lands_the_validated_draft_and_reports_it() -> Result<(), BoxError> {
    let dir = fixture_dir("write")?;
    write_release_please_fixture(&dir)?;
    let (ok, text) = halyard(&dir, &["migrate", "--write", "--format", "json"])?;
    assert!(ok, "the write is the honest happy path: {text}");
    let value: serde_json::Value = serde_json::from_str(text.trim())
        .map_err(|e| format!("stdout is one JSON document: {e}\n{text}"))?;
    let write = value
        .get("write")
        .ok_or("the write block exists when --write ran")?;
    assert_eq!(
        write["outcome"], "written",
        "the outcome is snake_case data: {text}"
    );
    assert_eq!(write["tool"], "release-please");
    assert_eq!(write["path"], "halyard.toml");
    let bytes = write["bytes"].as_u64().ok_or("the byte count is data")?;
    assert!(bytes > 0, "a real draft landed: {write}");
    // The file is on disk and is exactly what the report reviewed.
    let landed = std::fs::read_to_string(dir.join("halyard.toml"))?;
    assert_eq!(
        landed.len() as u64,
        bytes,
        "the byte count matches the file"
    );
    assert!(
        landed.contains("[versioning.pre_major]") && landed.contains("tag_style"),
        "the draft carries the mapped sections: {landed}"
    );
    Ok(())
}

#[test]
fn migrate_write_never_clobbers_an_existing_config() -> Result<(), BoxError> {
    let dir = fixture_dir("write-existing")?;
    write_release_please_fixture(&dir)?;
    let previous = "# the human's hand-written config\n[repo]\nbase_branch = \"main\"\n";
    std::fs::write(dir.join("halyard.toml"), previous)?;
    let (ok, text) = halyard(&dir, &["migrate", "--write", "--format", "json"])?;
    assert!(!ok, "an existing halyard.toml is never overwritten");
    let value: serde_json::Value = serde_json::from_str(text.trim())
        .map_err(|e| format!("the refusal is one JSON document: {e}\n{text}"))?;
    let error = value
        .get("error")
        .ok_or("the refusal carries an error object")?;
    assert_eq!(
        error.get("code").and_then(|c| c.as_str()),
        Some("E-CFG-MIGRATE"),
        "the stable code surfaces: {text}"
    );
    assert!(
        error
            .get("message")
            .and_then(|m| m.as_str())
            .is_some_and(|m| m.contains("already exists")),
        "the diagnosis names the rule: {text}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("halyard.toml"))?,
        previous,
        "the existing config is byte-identical after the refusal"
    );
    Ok(())
}

#[test]
fn migrate_write_without_foreign_configs_is_an_honest_no_op() -> Result<(), BoxError> {
    let dir = fixture_dir("write-empty")?;
    let (ok, text) = halyard(&dir, &["migrate", "--write", "--format", "json"])?;
    assert!(ok, "absence is a value, not an error: {text}");
    let value: serde_json::Value = serde_json::from_str(text.trim())
        .map_err(|e| format!("stdout is one JSON document: {e}\n{text}"))?;
    assert_eq!(
        value
            .get("write")
            .and_then(|w| w.get("outcome"))
            .and_then(|o| o.as_str()),
        Some("nothing_configured"),
        "the no-op is reported, not thrown: {text}"
    );
    assert!(
        !dir.join("halyard.toml").exists(),
        "nothing is written when nothing is configured"
    );
    Ok(())
}

#[test]
fn migrate_write_refuses_to_guess_between_two_tools() -> Result<(), BoxError> {
    let dir = fixture_dir("write-ambiguous")?;
    write_release_please_fixture(&dir)?;
    std::fs::create_dir_all(dir.join(".changeset"))?;
    std::fs::write(
        dir.join(".changeset/config.json"),
        r#"{ "access": "public", "baseBranch": "develop" }"#,
    )?;
    let (ok, text) = halyard(&dir, &["migrate", "--write", "--format", "json"])?;
    assert!(!ok, "two drafts never resolve by guessing");
    let value: serde_json::Value = serde_json::from_str(text.trim())
        .map_err(|e| format!("the refusal is one JSON document: {e}\n{text}"))?;
    let error = value
        .get("error")
        .ok_or("the refusal carries an error object")?;
    assert_eq!(
        error.get("code").and_then(|c| c.as_str()),
        Some("E-CFG-MIGRATE")
    );
    let message = error
        .get("message")
        .and_then(|m| m.as_str())
        .ok_or("the refusal carries a message")?;
    assert!(
        message.contains("release-please") && message.contains("changesets"),
        "the refusal names every candidate: {message}"
    );
    assert!(
        !dir.join("halyard.toml").exists(),
        "the refusal writes nothing"
    );
    Ok(())
}

#[test]
fn migrate_without_the_write_flag_stays_report_only() -> Result<(), BoxError> {
    let dir = fixture_dir("write-absent")?;
    write_release_please_fixture(&dir)?;
    let (ok, text) = halyard(&dir, &["migrate", "--format", "json"])?;
    assert!(ok);
    let value: serde_json::Value = serde_json::from_str(text.trim())
        .map_err(|e| format!("stdout is one JSON document: {e}\n{text}"))?;
    assert!(
        value.get("write").is_none(),
        "the write block exists only when the flag ran: {text}"
    );
    assert!(
        !dir.join("halyard.toml").exists(),
        "report-only stays report-only"
    );
    Ok(())
}

#[test]
fn migrate_write_human_output_names_what_landed() -> Result<(), BoxError> {
    let dir = fixture_dir("write-human")?;
    write_release_please_fixture(&dir)?;
    let (ok, text) = halyard(&dir, &["migrate", "--write"])?;
    assert!(ok, "the write is the honest happy path: {text}");
    assert!(
        text.contains("wrote `halyard.toml`"),
        "the human header names the write: {text}"
    );
    assert!(
        text.contains("release-please"),
        "the header names the draft's tool: {text}"
    );
    Ok(())
}
