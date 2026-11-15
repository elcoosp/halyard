//! The §16.2 write step's contract (M4.7 step 2): the validated draft
//! that step 1 surfaces as a report string can be written to
//! `halyard.toml` through the injected [`DraftSink`] — the pure planner
//! ([`plan_draft_write`]) picks the draft, the runner
//! ([`run_draft_write`]) refuses an existing target and writes
//! atomically. Fail-closed rules pinned here: the migration never
//! clobbers a config, never guesses between two configured tools, and
//! treats a missing foreign config as an honest no-op (absence is a
//! value). Deterministic (I3): the plan and the report are plain data.

use halyard_config::migrate::{
    AtomicDraftSink, DRAFT_TARGET, DraftSink, DraftWriteOutcome, DraftWriteReport, ForeignFiles,
    MemoryDraftSink, MigrationSource, ToolSurvey, default_sources, migrate, plan_draft_write,
    run_draft_write,
};
use halyard_config::{ConfigError, load_from_str};

type BoxError = Box<dyn std::error::Error>;

// ---------------------------------------------------------------------------
// Fixtures (the VERIFIED grammars, reused from step 1)
// ---------------------------------------------------------------------------

fn release_please_files() -> ForeignFiles {
    let mut files = ForeignFiles::new();
    files.insert(
        "release-please-config.json",
        r#"{
  "packages": {
    "crates/p1": { "package-name": "p1-crate" },
    "crates/p2": {}
  },
  "bump-minor-pre-major": true,
  "include-v-in-tag": false
}"#,
    );
    files.insert(
        ".release-please-manifest.json",
        r#"{ "crates/p1": "1.2.3", "crates/p2": "0.5.0" }"#,
    );
    files
}

fn changesets_files() -> ForeignFiles {
    let mut files = ForeignFiles::new();
    files.insert(
        ".changeset/config.json",
        r#"{ "access": "public", "baseBranch": "develop", "ignore": ["@scope/legacy"] }"#,
    );
    files
}

/// A test-local source (the trait is proven downstream-usable): surveys
/// as configured with an empty draft — nothing mappable.
#[derive(Debug)]
struct EmptyDraftSource;

impl MigrationSource for EmptyDraftSource {
    fn tool(&self) -> &'static str {
        "empty-draft-tool"
    }

    fn config_paths(&self) -> &'static [&'static str] {
        &["empty-draft.cfg"]
    }

    fn survey(&self, files: &ForeignFiles) -> Result<ToolSurvey, ConfigError> {
        Ok(ToolSurvey {
            tool: self.tool().to_owned(),
            configured: files.get("empty-draft.cfg").is_some(),
            draft: Some(String::new()),
            ..ToolSurvey::default()
        })
    }
}

// ---------------------------------------------------------------------------
// The pure planner
// ---------------------------------------------------------------------------

#[test]
fn the_plan_selects_the_single_configured_draft_verbatim() -> Result<(), BoxError> {
    let report = migrate(&release_please_files(), &default_sources())?;
    let survey = report
        .tools
        .iter()
        .find(|t| t.tool == "release-please")
        .ok_or_else(|| ConfigError::Migrate {
            path: "test".to_owned(),
            message: "the release-please survey is present".to_owned(),
        })?;
    let plan = plan_draft_write(&report)?.ok_or_else(|| ConfigError::Migrate {
        path: "test".to_owned(),
        message: "a configured tool is a write candidate".to_owned(),
    })?;
    assert_eq!(plan.tool, "release-please");
    assert_eq!(plan.path, DRAFT_TARGET);
    // The written payload is exactly the report's draft — no
    // re-serialization drift between what the human reviewed and what
    // lands.
    assert_eq!(
        plan.contents,
        survey.draft.as_deref().unwrap_or_default(),
        "the plan carries the reviewed draft verbatim"
    );
    // And the payload is exactly what halyard.toml v1 accepts (the
    // step-1 validation rides the plan).
    load_from_str(&plan.contents)?;
    Ok(())
}

#[test]
fn two_configured_tools_refuse_to_guess() {
    let mut files = release_please_files();
    files.insert(
        ".changeset/config.json",
        changesets_files()
            .get(".changeset/config.json")
            .unwrap_or_default()
            .to_owned(),
    );
    let report = migrate(&files, &default_sources()).expect("both surveys parse");
    let error = plan_draft_write(&report).expect_err("two drafts never resolve by guessing");
    assert_eq!(error.code(), ConfigError::MIGRATE_CODE);
    let rendered = error.to_string();
    assert!(
        rendered.contains("release-please") && rendered.contains("changesets"),
        "the refusal names every candidate: {rendered}"
    );
}

#[test]
fn a_configured_tool_with_nothing_to_map_is_not_a_candidate() -> Result<(), BoxError> {
    let mut files = ForeignFiles::new();
    files.insert("empty-draft.cfg", "present");
    let sources: Vec<Box<dyn MigrationSource>> = vec![Box::new(EmptyDraftSource)];
    let report = migrate(&files, &sources)?;
    assert!(
        plan_draft_write(&report)?.is_none(),
        "an empty draft writes nothing"
    );
    Ok(())
}

#[test]
fn no_foreign_configuration_plans_nothing() -> Result<(), BoxError> {
    let report = migrate(&ForeignFiles::new(), &default_sources())?;
    assert!(plan_draft_write(&report)?.is_none(), "absence is a value");
    Ok(())
}

// ---------------------------------------------------------------------------
// The runner over the seam
// ---------------------------------------------------------------------------

#[test]
fn the_write_lands_the_plan_through_the_sink() -> Result<(), BoxError> {
    let report = migrate(&release_please_files(), &default_sources())?;
    let plan = plan_draft_write(&report)?.ok_or_else(|| ConfigError::Migrate {
        path: "test".to_owned(),
        message: "a configured tool is a write candidate".to_owned(),
    })?;
    let sink = MemoryDraftSink::new();
    let write = run_draft_write(Some(plan.clone()), &sink)?;
    assert_eq!(write.outcome, DraftWriteOutcome::Written);
    assert_eq!(write.tool, "release-please");
    assert_eq!(write.path, DRAFT_TARGET);
    assert_eq!(write.bytes, plan.contents.len());
    assert_eq!(
        sink.content(DRAFT_TARGET),
        Some(plan.contents.clone()),
        "the sink holds exactly the validated draft"
    );
    assert_eq!(sink.writes().len(), 1, "one write, no partial retries");
    Ok(())
}

#[test]
fn an_existing_target_is_never_clobbered() -> Result<(), BoxError> {
    let report = migrate(&release_please_files(), &default_sources())?;
    let plan = plan_draft_write(&report)?.ok_or_else(|| ConfigError::Migrate {
        path: "test".to_owned(),
        message: "a configured tool is a write candidate".to_owned(),
    })?;
    let sink = MemoryDraftSink::with_existing(DRAFT_TARGET);
    let error =
        run_draft_write(Some(plan), &sink).expect_err("an existing config is never overwritten");
    assert_eq!(error.code(), ConfigError::MIGRATE_CODE);
    let rendered = error.to_string();
    assert!(
        rendered.contains(DRAFT_TARGET) && rendered.contains("already exists"),
        "the refusal names the target and the rule: {rendered}"
    );
    assert!(
        sink.writes().is_empty(),
        "validate-then-act: the refusal happens before any write"
    );
    Ok(())
}

#[test]
fn nothing_configured_writes_nothing_and_is_not_an_error() -> Result<(), BoxError> {
    let sink = MemoryDraftSink::new();
    let write = run_draft_write(None, &sink)?;
    assert_eq!(write.outcome, DraftWriteOutcome::NothingConfigured);
    assert_eq!(write.bytes, 0);
    assert!(write.tool.is_empty() && write.path.is_empty());
    assert!(sink.writes().is_empty(), "the sink is never touched");
    Ok(())
}

#[test]
fn the_write_report_is_deterministic() -> Result<(), BoxError> {
    let report = migrate(&release_please_files(), &default_sources())?;
    let plan = plan_draft_write(&report)?.ok_or_else(|| ConfigError::Migrate {
        path: "test".to_owned(),
        message: "a configured tool is a write candidate".to_owned(),
    })?;
    let first = run_draft_write(Some(plan.clone()), &MemoryDraftSink::new())?;
    let second = run_draft_write(Some(plan), &MemoryDraftSink::new())?;
    assert_eq!(first, second, "the outcome is plain data (I3)");
    Ok(())
}

#[test]
fn the_write_report_serializes_with_snake_case_outcomes() -> Result<(), BoxError> {
    let write = DraftWriteReport {
        tool: "release-please".to_owned(),
        path: DRAFT_TARGET.to_owned(),
        bytes: 128,
        outcome: DraftWriteOutcome::Written,
    };
    let value = serde_json::to_value(&write)?;
    assert_eq!(
        value.get("outcome").and_then(|o| o.as_str()),
        Some("written")
    );
    assert_eq!(
        value.get("path").and_then(|p| p.as_str()),
        Some(DRAFT_TARGET)
    );
    let nothing = run_draft_write(None, &MemoryDraftSink::new())?;
    let nothing_value = serde_json::to_value(&nothing)?;
    assert_eq!(
        nothing_value.get("outcome").and_then(|o| o.as_str()),
        Some("nothing_configured")
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// The production sink (real filesystem, atomic protocol)
// ---------------------------------------------------------------------------

#[test]
fn the_atomic_sink_writes_reads_and_refuses_re_writes() -> Result<(), BoxError> {
    let dir = tempfile::tempdir()?;
    let target = dir.path().join(DRAFT_TARGET);
    let sink = AtomicDraftSink;
    assert!(
        !sink.exists(&target.display().to_string())?,
        "a fresh dir has no target"
    );
    sink.write(
        &target.display().to_string(),
        "[repo]\ntag_style = \"v{version}\"\n",
    )?;
    let landed = std::fs::read_to_string(&target)?;
    assert_eq!(landed, "[repo]\ntag_style = \"v{version}\"\n");
    assert!(
        sink.exists(&target.display().to_string())?,
        "the write is observable"
    );
    // I1-shaped: the second attempt at the same target refuses instead
    // of overwriting.
    let error = sink
        .write(&target.display().to_string(), "clobber")
        .expect_err("the production sink refuses an existing target");
    assert_eq!(error.code(), ConfigError::MIGRATE_CODE);
    assert_eq!(
        std::fs::read_to_string(&target)?,
        landed,
        "the file is untouched"
    );
    // The atomic protocol leaves no temp siblings behind.
    let entries: Vec<_> = std::fs::read_dir(dir.path())?.collect::<Result<_, _>>()?;
    assert_eq!(entries.len(), 1, "only the target exists: {entries:?}");
    Ok(())
}

#[test]
fn the_atomic_sink_maps_io_failures_onto_the_stable_code() -> Result<(), BoxError> {
    let dir = tempfile::tempdir()?;
    let sink = AtomicDraftSink;
    // A target whose parent directory does not exist fails closed with
    // the stable code (an IO error is an E-CFG-MIGRATE diagnosis, not a
    // panic).
    let missing = dir.path().join("nope").join(DRAFT_TARGET);
    let error = sink
        .write(&missing.display().to_string(), "x")
        .expect_err("a missing parent dir refuses");
    assert_eq!(error.code(), ConfigError::MIGRATE_CODE);
    Ok(())
}

#[test]
fn the_memory_sink_models_the_contract_downstream() {
    let sink = MemoryDraftSink::with_existing("other.toml");
    assert!(sink.exists("other.toml").expect("modeled files report"));
    assert!(!sink.exists(DRAFT_TARGET).expect("unmodeled files report"));
    sink.write(DRAFT_TARGET, "a").expect("records writes");
    sink.write("b.toml", "b").expect("records writes");
    assert_eq!(sink.content(DRAFT_TARGET), Some("a".to_owned()));
    assert_eq!(sink.content("b.toml"), Some("b".to_owned()));
    assert_eq!(sink.writes().len(), 2, "the log keeps the order");
    assert_eq!(
        sink.content("other.toml"),
        None,
        "a modeled-but-unwritten file stays as-is"
    );
}
