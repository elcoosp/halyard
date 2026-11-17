//! The `halyard-action` contract gates (§12, ADR-0037): the env-file
//! grammar and the writer seam's fail-closed posture over real temp
//! files, the §12 forecast renderer over the plan wire types, the
//! preview writer's ordered call log (the observability convention)
//! and the canonical plan-file write. No test mutates the process
//! environment (edition-2024 `set_var` is unsafe; the present-env
//! branch is gated by the CLI e2e suite, which sets it on the child).

use std::fs;
use std::path::Path;

use halyard_action::{
    ActionCall, ActionError, ActionWriter, GithubActionsWriter, OutputName, PreviewActionWriter,
    env_file_entry, plan_outputs, plan_summary, release_rows, write_plan_file,
};
use halyard_core::{
    Bump, PLAN_SCHEMA_V1, Plan, PlanEdit, PlanPatch, PlanReason, PlanRelease, PlanSkip,
    canonical_json,
};
use tempfile::TempDir;

type BoxError = Box<dyn std::error::Error>;

/// A two-release plan (`core` minor cargo bump, `app` patch npm bump)
/// with a duplicated edit site (the `paths_released` dedupe pin) and
/// one skip (the summary's honest counts).
fn sample_plan() -> Plan {
    Plan {
        schema: PLAN_SCHEMA_V1.to_owned(),
        base_branch: "main".to_owned(),
        head: "9f2ca67".to_owned(),
        plan_hash: "blake3:ab12".to_owned(),
        releases: vec![
            release(
                "core",
                "0.4.1",
                "0.5.0",
                "core-v0.5.0",
                "cargo",
                Bump::Minor,
            ),
            release("app", "1.0.0", "1.1.0", "app-v1.1.0", "npm", Bump::Patch),
        ],
        edits: vec![
            edit("crates/core/Cargo.toml", 123, 128, "0.5.0"),
            edit("crates/core/CHANGELOG.md", 0, 11, "## 0.5.0"),
            edit("crates/core/Cargo.toml", 210, 215, "0.5.0"),
        ],
        order: vec!["core".to_owned(), "app".to_owned()],
        skipped: vec![PlanSkip {
            package: "docs".to_owned(),
            reason: "no_changes".to_owned(),
        }],
    }
}

fn release(
    package: &str,
    from: &str,
    to: &str,
    tag: &str,
    ecosystem: &str,
    bump: Bump,
) -> PlanRelease {
    PlanRelease {
        package: package.to_owned(),
        ecosystem: ecosystem.to_owned(),
        from: from.to_owned(),
        to: to.to_owned(),
        bump,
        tag: tag.to_owned(),
        reasons: vec![PlanReason::Commit {
            oid: "ab12cd34".to_owned(),
            kind: "feat".to_owned(),
            breaking: false,
        }],
        changelog: None,
        publish: None,
    }
}

fn edit(file: &str, start: u32, end: u32, text: &str) -> PlanEdit {
    PlanEdit {
        file: file.to_owned(),
        patches: vec![PlanPatch {
            start,
            end,
            text: text.to_owned(),
        }],
    }
}

#[test]
fn plan_forecast_outputs_carry_the_12_vocabulary() -> Result<(), BoxError> {
    let plan = sample_plan();
    let outputs = plan_outputs(&plan, Path::new("out/plan.json"))?;
    let flat: Vec<(String, String)> = outputs
        .iter()
        .map(|(name, value)| (name.to_string(), value.clone()))
        .collect();

    // The §12 scalar outputs, in the documented order (`.get()` over
    // indexing — the workspace slice restriction, §15.3).
    let entry = |idx: usize| -> Option<(String, String)> { flat.get(idx).cloned() };
    assert_eq!(entry(0), Some(("released".to_owned(), "false".to_owned())));
    assert_eq!(entry(1), Some(("published".to_owned(), "false".to_owned())));
    assert_eq!(
        entry(2),
        Some(("plan_json".to_owned(), "out/plan.json".to_owned()))
    );

    // `releases_json` — the §12 row grammar (name, version, tag,
    // ecosystem, url); the forecast url stays empty until a forge mode
    // fills it.
    let releases = entry(3).expect("releases_json is present");
    assert_eq!(releases.0, "releases_json");
    let rows: serde_json::Value = serde_json::from_str(&releases.1)?;
    let array = rows.as_array().expect("releases_json is an array");
    assert_eq!(array.len(), 2);
    let row = |key: &str| -> Option<&serde_json::Value> {
        array.first().and_then(|value| value.get(key))
    };
    assert_eq!(row("name"), Some(&serde_json::json!("core")));
    assert_eq!(row("version"), Some(&serde_json::json!("0.5.0")));
    assert_eq!(row("tag"), Some(&serde_json::json!("core-v0.5.0")));
    assert_eq!(row("ecosystem"), Some(&serde_json::json!("cargo")));
    assert_eq!(row("url"), Some(&serde_json::json!("")));

    // Per-package keys in plan order (§12: `core--version`, `core--tag`).
    assert_eq!(
        entry(4),
        Some(("core--version".to_owned(), "0.5.0".to_owned()))
    );
    assert_eq!(
        entry(5),
        Some(("core--tag".to_owned(), "core-v0.5.0".to_owned()))
    );
    assert_eq!(
        entry(6),
        Some(("app--version".to_owned(), "1.1.0".to_owned()))
    );
    assert_eq!(
        entry(7),
        Some(("app--tag".to_owned(), "app-v1.1.0".to_owned()))
    );

    // `paths_released` — the distinct edit sites, sorted, newline-joined
    // (the heredoc form renders at write time).
    let (last_name, last_value) = flat.last().expect("paths_released is present");
    assert_eq!(last_name, "paths_released");
    assert_eq!(
        last_value,
        "crates/core/CHANGELOG.md\ncrates/core/Cargo.toml"
    );
    assert_eq!(flat.len(), 9);
    Ok(())
}

#[test]
fn output_name_grammar_is_fail_closed() {
    assert!(OutputName::new("released").is_ok());
    assert!(OutputName::new("core--version").is_ok());
    // The file grammar's delimiters never ride a name.
    assert!(matches!(
        OutputName::new(""),
        Err(ActionError::Output { .. })
    ));
    assert!(matches!(
        OutputName::new("a=b"),
        Err(ActionError::Output { .. })
    ));
    assert!(matches!(
        OutputName::new("a\nb"),
        Err(ActionError::Output { .. })
    ));
    // Non-ASCII names are refused (the keys ride ASCII; cargo/npm
    // package names are ASCII).
    assert!(matches!(
        OutputName::new("π"),
        Err(ActionError::Output { .. })
    ));
    // Scoped npm names compose per-package keys — `/` and `@` are not
    // grammar-breaking (consumers access the outputs object by key).
    let scoped = OutputName::package_field("@scope/name", "version").expect("scoped key");
    assert_eq!(scoped.as_str(), "@scope/name--version");
    // A hostile package name cannot smuggle the `=` delimiter through
    // the composer.
    assert!(matches!(
        OutputName::package_field("a=b", "version"),
        Err(ActionError::Output { .. })
    ));
}

#[test]
fn env_file_entry_grammar_matches_the_documented_forms() -> Result<(), BoxError> {
    let name = OutputName::new("released")?;
    // Single-line values ride the documented `{name}={value}` form.
    assert_eq!(env_file_entry(&name, "false")?, "released=false\n");
    // Multi-line values ride the documented heredoc form; a trailing
    // newline in the value is not preserved (consumers split on
    // newlines — the extra empty item would be drift).
    let paths = OutputName::new("paths_released")?;
    assert_eq!(
        env_file_entry(&paths, "crates/cli/Cargo.toml\ncrates/core/Cargo.toml\n")?,
        "paths_released<<HALYARD\ncrates/cli/Cargo.toml\ncrates/core/Cargo.toml\nHALYARD\n"
    );
    // A value carrying the delimiter on its own line is refused
    // fail-closed (the docs' own caution).
    assert!(matches!(
        env_file_entry(&paths, "HALYARD\nx"),
        Err(ActionError::Output { .. })
    ));
    Ok(())
}

#[test]
fn the_real_writer_appends_the_documented_bytes() -> Result<(), BoxError> {
    let dir = TempDir::new()?;
    let outputs = dir.path().join("output.txt");
    let summary = dir.path().join("summary.md");
    let writer = GithubActionsWriter::from_paths(outputs.clone(), Some(summary.clone()));

    writer.set_output(&OutputName::new("released")?, "false")?;
    writer.set_output(
        &OutputName::new("paths_released")?,
        "crates/core/Cargo.toml\ncrates/app/Cargo.toml",
    )?;
    // Appends accumulate (the `>>` convention) — the second call does
    // not clobber the first.
    let outputs_bytes = fs::read_to_string(&outputs)?;
    assert_eq!(
        outputs_bytes,
        "released=false\npaths_released<<HALYARD\ncrates/core/Cargo.toml\ncrates/app/Cargo.toml\nHALYARD\n"
    );

    writer.append_summary("## halyard plan\n")?;
    // The runner's automatic newline: a summary chunk without a
    // trailing newline gets one appended.
    writer.append_summary("1 planned")?;
    assert_eq!(
        fs::read_to_string(&summary)?,
        "## halyard plan\n1 planned\n"
    );
    // The annotation protocol rides the log (stdout), never the files.
    writer.emit_warning("no commit-graph — degraded walk")?;
    Ok(())
}

#[test]
fn summary_without_the_step_summary_path_is_refused() -> Result<(), BoxError> {
    let dir = TempDir::new()?;
    let outputs = dir.path().join("output.txt");
    let writer = GithubActionsWriter::from_paths(outputs, None);
    let error = writer
        .append_summary("## halyard plan")
        .expect_err("no GITHUB_STEP_SUMMARY path — the §12 job summary cannot land");
    assert_eq!(error.code(), "E-ACTION-SUMMARY");
    Ok(())
}

#[test]
fn from_env_without_the_output_file_is_none() {
    // The test process sets no GITHUB_OUTPUT (no env mutation — the
    // present-env branch is gated by the CLI e2e suite).
    assert!(GithubActionsWriter::from_env().is_none());
}

#[test]
fn the_preview_writer_records_the_ordered_calls() -> Result<(), BoxError> {
    let writer = PreviewActionWriter::new();
    writer.set_output(&OutputName::new("released")?, "false")?;
    writer.append_summary("## halyard plan\n")?;
    writer.emit_warning("no commit-graph — degraded walk")?;
    let calls = writer.calls();
    assert_eq!(calls.len(), 3);
    let call = |idx: usize| -> Option<&ActionCall> { calls.get(idx) };
    match call(0) {
        Some(ActionCall::SetOutput { name, value }) => {
            assert_eq!(name.as_str(), "released");
            assert_eq!(value, "false");
        }
        other => return Err(format!("the first call is set_output, got {other:?}").into()),
    }
    match call(1) {
        Some(ActionCall::AppendSummary { markdown }) => {
            assert_eq!(markdown, "## halyard plan\n");
        }
        other => return Err(format!("the second call is append_summary, got {other:?}").into()),
    }
    match call(2) {
        Some(ActionCall::Warning { message }) => {
            assert_eq!(message, "no commit-graph — degraded walk");
        }
        other => return Err(format!("the third call is emit_warning, got {other:?}").into()),
    }
    Ok(())
}

#[test]
fn release_rows_carry_the_12_row_grammar() {
    let rows = release_rows(&sample_plan());
    assert_eq!(rows.len(), 2);
    let row = |idx: usize| -> Option<&halyard_action::ReleaseRow> { rows.get(idx) };
    let first = row(0).expect("the first row");
    assert_eq!(first.name, "core");
    assert_eq!(first.version, "0.5.0");
    assert_eq!(first.tag, "core-v0.5.0");
    assert_eq!(first.ecosystem, "cargo");
    assert_eq!(first.url, "");
    let second = row(1).expect("the second row");
    assert_eq!(second.name, "app");
    assert_eq!(second.version, "1.1.0");
    assert_eq!(second.tag, "app-v1.1.0");
    assert_eq!(second.ecosystem, "npm");
    assert_eq!(second.url, "");
}

#[test]
fn plan_summary_renders_the_12_job_summary() {
    let summary = plan_summary(&sample_plan());
    assert!(summary.contains("## halyard plan"), "the §12 heading");
    assert!(
        summary.contains("| core | 0.4.1 → 0.5.0 | minor | core-v0.5.0 |"),
        "the release table rows: {summary}"
    );
    assert!(summary.contains("| app | 1.0.0 → 1.1.0 | patch | app-v1.1.0 |"));
    assert!(
        summary.contains("2 planned, 1 skipped"),
        "the honest counts: {summary}"
    );
    assert!(
        summary.contains("zero API calls (offline forecast, I5)"),
        "the call surface is stated, not implied: {summary}"
    );
}

#[test]
fn the_plan_file_is_canonical_json() -> Result<(), BoxError> {
    let dir = TempDir::new()?;
    let path = dir.path().join("halyard-plan.json");
    let plan = sample_plan();
    write_plan_file(&plan, &path)?;
    let expected = format!("{}\n", canonical_json(&serde_json::to_value(&plan)?));
    assert_eq!(fs::read_to_string(&path)?, expected);

    // A missing parent directory refuses with the plan-file code (the
    // path is the caller's contract, never invented).
    let orphan = dir.path().join("no-such-dir/plan.json");
    let error = write_plan_file(&plan, &orphan).expect_err("missing parent");
    assert_eq!(error.code(), "E-ACTION-PLAN");
    Ok(())
}
