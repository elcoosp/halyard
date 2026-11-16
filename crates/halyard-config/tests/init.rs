//! The `halyard init` engine contract (§16.1/§16.2, ADR-0029): the
//! scaffold planner ([`halyard_config::plan_init`]) turns the shell's
//! discovery survey into a create-only write plan (the documented
//! `halyard.toml` skeleton, plus the §16.1 workflow with `--github`),
//! and the runner ([`halyard_config::run_init`]) lands it through the
//! injected [`halyard_config::DraftSink`] — gated before any write, so
//! a refusal never leaves a half-scaffold behind.

use halyard_config::{
    AtomicScaffoldSink, ConfigError, DraftSink, INIT_CONFIG_TARGET, INIT_WORKFLOW_TARGET,
    InitSurvey, MemoryDraftSink, load_from_str, plan_init, run_init,
};

type BoxError = Box<dyn std::error::Error>;

/// The zero-config single-package Cargo repo (§16.1's opening example).
fn single_cargo() -> InitSurvey {
    InitSurvey {
        packages: 1,
        ecosystems: vec!["cargo".to_owned()],
        single_package: true,
    }
}

/// A mixed monorepo: two Cargo crates and one npm package.
fn mixed_workspace() -> InitSurvey {
    InitSurvey {
        packages: 3,
        ecosystems: vec!["cargo".to_owned(), "cargo".to_owned(), "npm".to_owned()],
        single_package: false,
    }
}

#[test]
fn plan_surveys_a_single_package_repo() -> Result<(), BoxError> {
    let plan = plan_init(&single_cargo(), false)?;
    let (config, rest) = plan
        .files
        .split_first()
        .ok_or("without --github the plan is the config skeleton alone")?;
    assert!(rest.is_empty(), "no workflow without --github");
    assert_eq!(config.path, INIT_CONFIG_TARGET);
    assert!(
        plan.shape.contains("1 package") && plan.shape.contains("cargo"),
        "the shape names the count and the ecosystem: {}",
        plan.shape
    );
    assert!(
        plan.shape.contains("single-package"),
        "the shape names the layout: {}",
        plan.shape
    );
    Ok(())
}

#[test]
fn plan_surveys_a_mixed_workspace_shape() -> Result<(), BoxError> {
    let plan = plan_init(&mixed_workspace(), false)?;
    assert!(
        plan.shape.contains("3 packages"),
        "the shape counts the packages: {}",
        plan.shape
    );
    assert!(
        plan.shape.contains("cargo: 2") && plan.shape.contains("npm: 1"),
        "the shape breaks the ecosystems down: {}",
        plan.shape
    );
    assert!(
        plan.shape.contains("workspace"),
        "the shape names the layout: {}",
        plan.shape
    );
    Ok(())
}

#[test]
fn the_draft_is_comment_only_and_loads_as_defaults() -> Result<(), BoxError> {
    let plan = plan_init(&mixed_workspace(), false)?;
    let draft = &plan
        .files
        .first()
        .ok_or("the config skeleton always plans")?
        .contents;
    assert!(
        draft.contains("halyard init"),
        "the skeleton names its maker: {draft}"
    );
    assert!(
        draft.contains("[versioning]"),
        "the skeleton documents the v1 sections: {draft}"
    );
    // The scaffold carries the defaults as comments — loading it must
    // produce the built-in defaults exactly (zero-config stays
    // zero-config; `init` never silently sets keys).
    let loaded = load_from_str(draft)?;
    assert_eq!(
        loaded,
        halyard_config::HalyardConfig::default(),
        "the comment-only skeleton plans the built-in defaults"
    );
    Ok(())
}

#[test]
fn plan_is_deterministic_and_github_adds_the_workflow() -> Result<(), BoxError> {
    let plain = plan_init(&single_cargo(), false)?;
    let again = plan_init(&single_cargo(), false)?;
    assert_eq!(plain, again, "the plan is pure data (I3)");
    let github = plan_init(&single_cargo(), true)?;
    assert_eq!(github.files.len(), 2, "--github adds one file");
    assert_eq!(
        github
            .files
            .first()
            .ok_or("the config skeleton lands first")?
            .path,
        INIT_CONFIG_TARGET
    );
    assert_eq!(
        github
            .files
            .get(1)
            .ok_or("--github adds the workflow second")?
            .path,
        INIT_WORKFLOW_TARGET
    );
    assert_eq!(
        github.shape, plain.shape,
        "the shape does not depend on the flag"
    );
    Ok(())
}

#[test]
fn the_workflow_is_the_s16_document() -> Result<(), BoxError> {
    let plan = plan_init(&single_cargo(), true)?;
    let workflow = &plan
        .files
        .get(1)
        .ok_or("--github plans the workflow second")?
        .contents;
    for marker in [
        "actions/checkout@v4",
        "halyard/action@v1",
        "fetch-depth: 0",
        "contents: write",
    ] {
        assert!(
            workflow.contains(marker),
            "the §16.1 workflow carries {marker}: {workflow}"
        );
    }
    Ok(())
}

#[test]
fn plan_refuses_an_empty_survey() {
    let empty = InitSurvey {
        packages: 0,
        ecosystems: vec![],
        single_package: true,
    };
    let error = plan_init(&empty, false).expect_err("nothing to scaffold for");
    assert_eq!(error.code(), ConfigError::INIT_CODE);
    assert!(
        error.to_string().contains("no package manifests"),
        "the refusal teaches what was looked for: {error}"
    );
}

#[test]
fn run_writes_both_files_in_order_over_the_memory_sink() -> Result<(), BoxError> {
    let plan = plan_init(&mixed_workspace(), true)?;
    let sink = MemoryDraftSink::new();
    let report = run_init(plan, &sink)?;
    assert_eq!(report.files.len(), 2, "both scaffold files landed");
    assert_eq!(
        report
            .files
            .first()
            .ok_or("the config report row is first")?
            .path,
        INIT_CONFIG_TARGET
    );
    assert!(
        report.files.first().is_some_and(|f| f.bytes > 0),
        "the skeleton is real bytes"
    );
    assert_eq!(
        sink.writes()
            .iter()
            .map(|(path, _)| path.as_str())
            .collect::<Vec<_>>(),
        vec![INIT_CONFIG_TARGET, INIT_WORKFLOW_TARGET],
        "the write order is plan order (config first)"
    );
    assert!(
        sink.content(INIT_WORKFLOW_TARGET)
            .is_some_and(|c| c.contains("halyard/action@v1")),
        "the workflow landed verbatim"
    );
    assert_eq!(
        report.shape,
        "3 packages (cargo: 2, npm: 1) — workspace layout"
    );
    Ok(())
}

#[test]
fn run_refuses_an_existing_config_and_writes_nothing() -> Result<(), BoxError> {
    let plan = plan_init(&single_cargo(), true)?;
    let sink = MemoryDraftSink::with_existing(INIT_CONFIG_TARGET);
    let error = run_init(plan, &sink).expect_err("an existing config is never clobbered");
    assert_eq!(error.code(), ConfigError::INIT_CODE);
    assert!(
        error.to_string().contains(INIT_CONFIG_TARGET),
        "the refusal names the target: {error}"
    );
    assert!(
        sink.writes().is_empty(),
        "the gate precedes every write — no half-scaffold"
    );
    Ok(())
}

#[test]
fn run_refuses_an_existing_workflow_and_writes_nothing() -> Result<(), BoxError> {
    let plan = plan_init(&single_cargo(), true)?;
    let sink = MemoryDraftSink::with_existing(INIT_WORKFLOW_TARGET);
    let error = run_init(plan, &sink).expect_err("an existing workflow is never clobbered");
    assert_eq!(error.code(), ConfigError::INIT_CODE);
    assert!(
        sink.writes().is_empty(),
        "the config skeleton is not written when the workflow refuses"
    );
    Ok(())
}

#[test]
fn the_atomic_sink_creates_directories_and_stays_create_only() -> Result<(), BoxError> {
    let dir = tempfile::tempdir()?;
    let sink = AtomicScaffoldSink;
    let nested = dir.path().join(".github").join("workflows");
    let workflow = nested.join("release.yml");
    let path = workflow.to_str().ok_or("temp path is utf-8")?;
    // The scaffold's only mkdir surface: the workflow's parent directory.
    sink.write(path, "on: { push: { branches: [main] } }\n")?;
    let landed = std::fs::read_to_string(&workflow)?;
    assert_eq!(landed, "on: { push: { branches: [main] } }\n");
    let error = sink
        .write(path, "clobber")
        .expect_err("create-only: the sink refuses a second write");
    assert_eq!(error.code(), ConfigError::INIT_CODE);
    assert_eq!(
        std::fs::read_to_string(&workflow)?,
        "on: { push: { branches: [main] } }\n",
        "the refusal leaves the file untouched"
    );
    Ok(())
}

#[test]
fn the_atomic_sink_writes_the_root_config_atomically() -> Result<(), BoxError> {
    let dir = tempfile::tempdir()?;
    let sink = AtomicScaffoldSink;
    let config = dir.path().join(INIT_CONFIG_TARGET);
    sink.write(config.to_str().ok_or("temp path is utf-8")?, "# skeleton\n")?;
    assert_eq!(std::fs::read_to_string(&config)?, "# skeleton\n");
    Ok(())
}
