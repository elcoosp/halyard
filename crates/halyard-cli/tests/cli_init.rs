//! CLI `init` smoke tests (§16.1/§16.2, ADR-0029): the command surveys
//! the working directory, lands the create-only `halyard.toml` skeleton
//! and — with `--github` — the §16.1 release workflow. Like `migrate`
//! it needs no git repo (the scaffold targets a repo that is not yet
//! halyard-configured); one end-to-end gate proves the scaffolded repo
//! still plans zero-config.

use std::process::Command;

use halyard_test_support::fixture::GitFixture;

type BoxError = Box<dyn std::error::Error>;

fn fixture_dir(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir = std::env::temp_dir().join(format!("halyard-cli-init-{label}-{}", std::process::id()));
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

/// A plain (non-git) single-package Cargo repo — the scaffold's target
/// shape. Init does not need history; the git fixture exists only for
/// the plan end-to-end gate.
fn cargo_fixture(label: &str) -> Result<std::path::PathBuf, BoxError> {
    let dir = fixture_dir(label)?;
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    std::fs::create_dir_all(dir.join("src"))?;
    std::fs::write(dir.join("src").join("lib.rs"), "pub const C: usize = 1;\n")?;
    Ok(dir)
}

fn parse_json(text: &str) -> Result<serde_json::Value, BoxError> {
    serde_json::from_str(text.trim())
        .map_err(|e| format!("stdout is one JSON document: {e}\n{text}").into())
}

/// The `{"error": {"code", "message"}}` refusal document's parts.
fn error_code_and_message(
    value: &serde_json::Value,
) -> Result<(Option<&str>, Option<&str>), BoxError> {
    let error = value
        .get("error")
        .ok_or("the refusal carries an error object")?;
    Ok((
        error.get("code").and_then(|c| c.as_str()),
        error.get("message").and_then(|m| m.as_str()),
    ))
}

#[test]
fn init_scaffolds_and_the_repo_plans_zero_config() -> Result<(), BoxError> {
    let mut fx = GitFixture::new("cli-init-plan")?;
    fx.write(
        "Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fx.write("src/lib.rs", "pub const C: usize = 1;\n")?;
    fx.stage_all()?;
    fx.commit("feat: scaffold")?;
    let (ok, text) = halyard(fx.root(), &["init", "--format", "json"])?;
    assert!(ok, "init is the honest happy path: {text}");
    let value = parse_json(&text)?;
    assert!(
        value
            .get("shape")
            .and_then(|s| s.as_str())
            .is_some_and(|s| s.contains("1 package") && s.contains("cargo")),
        "the shape rides the report: {value}"
    );
    let files = value
        .get("files")
        .and_then(|f| f.as_array())
        .ok_or("the report carries the files array")?;
    assert_eq!(files.len(), 1, "no workflow without --github");
    assert_eq!(
        files
            .first()
            .and_then(|f| f.get("path"))
            .and_then(|p| p.as_str()),
        Some("halyard.toml")
    );
    let landed = std::fs::read_to_string(fx.root().join("halyard.toml"))?;
    assert!(
        landed.contains("halyard init") && landed.contains("[versioning]"),
        "the skeleton landed: {landed}"
    );
    // The scaffold never sets keys — the repo still plans zero-config.
    let (ok, text) = halyard(fx.root(), &["plan", "--format", "json"])?;
    assert!(ok, "the scaffolded repo plans: {text}");
    let plan = parse_json(&text)?;
    assert!(
        plan.get("releases").is_some_and(serde_json::Value::is_array),
        "the plan document is the frozen v1 shape: {plan}"
    );
    Ok(())
}

#[test]
fn init_is_create_only_and_reports_the_stable_code() -> Result<(), BoxError> {
    let dir = cargo_fixture("create-only")?;
    let (ok, _) = halyard(&dir, &["init"])?;
    assert!(ok);
    let previous = std::fs::read_to_string(dir.join("halyard.toml"))?;
    let (ok, text) = halyard(&dir, &["init", "--format", "json"])?;
    assert!(!ok, "a second init refuses (create-only)");
    let value = parse_json(&text)?;
    let (code, message) = error_code_and_message(&value)?;
    assert_eq!(code, Some("E-CFG-INIT"), "the stable code surfaces: {text}");
    assert!(
        message.is_some_and(|m| m.contains("already exists")),
        "the refusal names the rule: {text}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("halyard.toml"))?,
        previous,
        "the existing config is byte-identical after the refusal"
    );
    Ok(())
}

#[test]
fn init_github_writes_the_s16_workflow() -> Result<(), BoxError> {
    let dir = cargo_fixture("github")?;
    let (ok, text) = halyard(&dir, &["init", "--github", "--format", "json"])?;
    assert!(ok, "the workflow scaffold is the honest happy path: {text}");
    let value = parse_json(&text)?;
    let files = value
        .get("files")
        .and_then(|f| f.as_array())
        .ok_or("the report carries the files array")?;
    assert_eq!(files.len(), 2, "config skeleton + workflow");
    let workflow = dir.join(".github").join("workflows").join("release.yml");
    let landed = std::fs::read_to_string(&workflow)?;
    assert!(
        landed.contains("halyard/action@v1") && landed.contains("actions/checkout@v4"),
        "the §16.1 document landed: {landed}"
    );
    Ok(())
}

#[test]
fn init_refuses_an_existing_workflow_before_anything() -> Result<(), BoxError> {
    let dir = cargo_fixture("workflow-exists")?;
    std::fs::create_dir_all(dir.join(".github").join("workflows"))?;
    std::fs::write(
        dir.join(".github").join("workflows").join("release.yml"),
        "# the human's hand-written workflow\n",
    )?;
    let (ok, text) = halyard(&dir, &["init", "--github", "--format", "json"])?;
    assert!(!ok, "an existing workflow is never clobbered");
    let value = parse_json(&text)?;
    let (code, message) = error_code_and_message(&value)?;
    assert_eq!(code, Some("E-CFG-INIT"));
    assert!(
        message.is_some_and(|m| m.contains("release.yml")),
        "the refusal names the workflow: {text}"
    );
    assert!(
        !dir.join("halyard.toml").exists(),
        "the gate precedes every write — no half-scaffold"
    );
    Ok(())
}

#[test]
fn init_refuses_a_dotted_config_it_would_shadow() -> Result<(), BoxError> {
    let dir = cargo_fixture("dotted-config")?;
    std::fs::create_dir_all(dir.join(".config"))?;
    std::fs::write(
        dir.join(".config").join("halyard.toml"),
        "[repo]\nbase_branch = \"main\"\n",
    )?;
    let (ok, text) = halyard(&dir, &["init", "--format", "json"])?;
    assert!(!ok, "a config the scaffold would shadow refuses");
    let value = parse_json(&text)?;
    let (code, message) = error_code_and_message(&value)?;
    assert_eq!(code, Some("E-CFG-INIT"));
    assert!(
        message.is_some_and(|m| m.contains(".config/halyard.toml")),
        "the refusal names the shadowed config: {text}"
    );
    assert!(
        !dir.join("halyard.toml").exists(),
        "the refusal writes nothing"
    );
    Ok(())
}

#[test]
fn init_refuses_an_empty_directory() -> Result<(), BoxError> {
    let dir = fixture_dir("empty")?;
    let (ok, text) = halyard(&dir, &["init", "--format", "json"])?;
    assert!(!ok, "nothing to scaffold for");
    let value = parse_json(&text)?;
    let (code, message) = error_code_and_message(&value)?;
    assert_eq!(code, Some("E-CFG-INIT"));
    assert!(
        message.is_some_and(|m| m.contains("no package manifests")),
        "the refusal teaches what was looked for: {text}"
    );
    Ok(())
}

#[test]
fn init_surveys_the_ecosystem_mix() -> Result<(), BoxError> {
    let dir = fixture_dir("npm")?;
    std::fs::write(
        dir.join("package.json"),
        "{\n  \"name\": \"app\",\n  \"version\": \"0.1.0\"\n}\n",
    )?;
    let (ok, text) = halyard(&dir, &["init"])?;
    assert!(ok);
    assert!(
        text.contains("1 package") && text.contains("npm"),
        "the human shape names the ecosystem: {text}"
    );
    Ok(())
}

#[test]
fn init_human_output_names_what_landed() -> Result<(), BoxError> {
    let dir = cargo_fixture("human")?;
    let (ok, text) = halyard(&dir, &["init"])?;
    assert!(ok);
    assert!(
        text.contains("wrote `halyard.toml`"),
        "the header names the write: {text}"
    );
    assert!(
        text.contains("halyard plan"),
        "the next step is named: {text}"
    );
    Ok(())
}

#[test]
fn init_gha_output_carries_the_files() -> Result<(), BoxError> {
    let dir = cargo_fixture("gha")?;
    let (ok, text) = halyard(&dir, &["init", "--github", "--format", "gha"])?;
    assert!(ok);
    assert!(
        text.contains("::notice") && text.contains("halyard.toml"),
        "gha annotations name the landed files: {text}"
    );
    Ok(())
}
