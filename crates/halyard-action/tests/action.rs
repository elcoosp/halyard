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
    // In CI, GitHub Actions sets GITHUB_OUTPUT; the present-env
    // branch is gated by the CLI e2e suite. Skip this assertion in CI.
    if std::env::var("CI").is_ok() {
        return;
    }
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

// ---------------------------------------------------------------------------
// The §12 `pr` mode renderers (ADR-0043): the §11.1 machine pass viewed as
// plain §12 data. The shell maps the forge's MachineReport onto
// `PrPass`; this crate renders the vocabulary and stays forge-free.
// ---------------------------------------------------------------------------

use std::collections::BTreeMap;

use halyard_action::{PrEffect, PrPass, pr_outputs, pr_summary};

/// A prepare-phase pass: the Release PR was opened (the §11.3 first
/// pass), nothing was released.
fn opened_pass(forecast: bool) -> PrPass {
    PrPass {
        pr_effect: Some(PrEffect::Opened {
            number: 7,
            url: "https://github.com/example/monorepo/pull/7".to_owned(),
        }),
        propagated: false,
        released: false,
        release_urls: BTreeMap::new(),
        branch: "halyard/release/main".to_owned(),
        forecast,
    }
}

#[test]
fn pr_outputs_carry_the_release_pr_keys_in_documented_order() -> Result<(), BoxError> {
    let plan = sample_plan();
    let outputs = pr_outputs(&plan, Path::new("out/plan.json"), &opened_pass(false))?;
    let flat: Vec<(String, String)> = outputs
        .iter()
        .map(|(name, value)| (name.to_string(), value.clone()))
        .collect();

    // The documented order: released, published, the Release-PR keys,
    // plan_json, releases_json — then the per-package keys and
    // paths_released (the plan vocabulary's tail).
    let entry = |idx: usize| -> Option<(String, String)> { flat.get(idx).cloned() };
    assert_eq!(entry(0), Some(("released".to_owned(), "false".to_owned())));
    assert_eq!(entry(1), Some(("published".to_owned(), "false".to_owned())));
    assert_eq!(entry(2), Some(("pr_number".to_owned(), "7".to_owned())));
    assert_eq!(
        entry(3),
        Some((
            "pr_url".to_owned(),
            "https://github.com/example/monorepo/pull/7".to_owned()
        ))
    );
    assert_eq!(
        entry(4),
        Some(("plan_json".to_owned(), "out/plan.json".to_owned()))
    );
    assert!(entry(5).is_some_and(|(name, _)| name == "releases_json"));

    // The per-package keys ride the same grammar as the plan mode.
    assert!(
        flat.iter()
            .any(|(name, value)| name == "core--version" && value == "0.5.0"),
        "the per-package version key: {flat:?}"
    );
    Ok(())
}

#[test]
fn pr_outputs_report_minted_release_urls_by_tag() -> Result<(), BoxError> {
    let plan = sample_plan();
    let pass = PrPass {
        pr_effect: None,
        propagated: true,
        released: false,
        release_urls: BTreeMap::from([(
            "core-v0.5.0".to_owned(),
            "https://github.com/example/monorepo/releases/tag/core-v0.5.0".to_owned(),
        )]),
        branch: "halyard/release/main".to_owned(),
        forecast: true,
    };
    let outputs = pr_outputs(&plan, Path::new("out/plan.json"), &pass)?;
    let releases = outputs
        .iter()
        .find(|(name, _)| name.as_str() == "releases_json")
        .map(|(_, value)| value.clone())
        .expect("releases_json is present");
    let rows: serde_json::Value = serde_json::from_str(&releases)?;
    let array = rows.as_array().expect("releases_json is an array");
    let url_of = |name: &str| -> Option<&serde_json::Value> {
        array
            .iter()
            .find(|row| row.get("name") == Some(&serde_json::json!(name)))
            .and_then(|row| row.get("url"))
    };
    // The tag phase's minted release URL fills the row; the row the
    // pass did not touch keeps the empty url (ADR-0037's contract).
    assert_eq!(
        url_of("core"),
        Some(&serde_json::json!(
            "https://github.com/example/monorepo/releases/tag/core-v0.5.0"
        ))
    );
    assert_eq!(url_of("app"), Some(&serde_json::json!("")));
    // The PR keys are absent — the tag phase owned the pass (PROPAGATE).
    assert!(
        outputs.iter().all(|(name, _)| name.as_str() != "pr_number"),
        "no PR keys on the propagated pass"
    );
    Ok(())
}

#[test]
fn pr_outputs_released_only_when_a_pass_executed_a_release() -> Result<(), BoxError> {
    let plan = sample_plan();
    // The dry forecast: the machine's minted urls ride, `released`
    // stays honestly false — nothing was executed (I5).
    let forecast = PrPass {
        pr_effect: None,
        propagated: true,
        released: false,
        release_urls: BTreeMap::from([(
            "core-v0.5.0".to_owned(),
            "preview://core-v0.5.0".to_owned(),
        )]),
        branch: "halyard/release/main".to_owned(),
        forecast: true,
    };
    let outputs = pr_outputs(&plan, Path::new("out/plan.json"), &forecast)?;
    let released = outputs
        .iter()
        .find(|(name, _)| name.as_str() == "released")
        .map(|(_, value)| value.clone())
        .expect("released is present");
    assert_eq!(released, "false");

    // The live tag phase: the creation is the release act.
    let live = PrPass {
        released: true,
        forecast: false,
        ..forecast
    };
    let outputs = pr_outputs(&plan, Path::new("out/plan.json"), &live)?;
    let released = outputs
        .iter()
        .find(|(name, _)| name.as_str() == "released")
        .map(|(_, value)| value.clone())
        .expect("released is present");
    assert_eq!(released, "true");
    Ok(())
}

#[test]
fn pr_summary_names_the_pass_and_its_posture() {
    let plan = sample_plan();
    // The opened pass, live.
    let summary = pr_summary(&plan, &opened_pass(false));
    assert!(
        summary.starts_with("## halyard release PR\n"),
        "the §12 heading: {summary}"
    );
    assert!(
        summary.contains("| core | 0.4.1 → 0.5.0 | minor | core-v0.5.0 |"),
        "the release table rides: {summary}"
    );
    assert!(
        summary.contains("PR #7 opened (https://github.com/example/monorepo/pull/7)"),
        "the pass line names the effect and the forge's own URL: {summary}"
    );
    assert!(
        summary.contains("2 planned, 1 skipped"),
        "the honest counts ride: {summary}"
    );
    assert!(
        !summary.contains("forecast"),
        "a live pass is not labelled a forecast: {summary}"
    );

    // The dry forecast says so.
    let forecast_summary = pr_summary(&plan, &opened_pass(true));
    assert!(
        forecast_summary.contains("forecast (nothing executed)"),
        "the forecast posture is stated: {forecast_summary}"
    );
}

#[test]
fn pr_summary_names_the_propagated_and_noop_passes() {
    let plan = sample_plan();
    // PROPAGATE: the tag phase owns the pass; the Release PR lands on
    // the next one (§11.1 step 2).
    let propagated = PrPass {
        pr_effect: None,
        propagated: true,
        released: true,
        release_urls: BTreeMap::from([("core-v0.5.0".to_owned(), "mem://core-v0.5.0".to_owned())]),
        branch: "halyard/release/main".to_owned(),
        forecast: false,
    };
    let summary = pr_summary(&plan, &propagated);
    assert!(
        summary.contains("tag phase owns this pass"),
        "the PROPAGATE posture: {summary}"
    );

    // The one-probe no-op: empty plan, no stale PR (§11.1 step 3).
    let noop = PrPass {
        pr_effect: None,
        propagated: false,
        released: false,
        release_urls: BTreeMap::new(),
        branch: "halyard/release/main".to_owned(),
        forecast: false,
    };
    let summary = pr_summary(&plan, &noop);
    assert!(
        summary.contains("no Release PR (nothing to open, none stale)"),
        "the no-op posture: {summary}"
    );

    // The closed stale PR under an empty plan.
    let closed = PrPass {
        pr_effect: Some(PrEffect::Closed {
            number: 3,
            url: "https://github.com/example/monorepo/pull/3".to_owned(),
        }),
        propagated: false,
        released: false,
        release_urls: BTreeMap::new(),
        branch: "halyard/release/main".to_owned(),
        forecast: false,
    };
    let summary = pr_summary(&plan, &closed);
    assert!(
        summary.contains("PR #3 closed"),
        "the close posture: {summary}"
    );
}

// ---------------------------------------------------------------------------
// The §12 `check` mode's renderer (ADR-0044): the title verdict beside
// the pending forecast — the same release table, a verdict line. The
// outputs ride the plan vocabulary unchanged (check mode reuses
// `plan_outputs`); the verdict is the mode's own surface.
// ---------------------------------------------------------------------------

use halyard_action::{CheckRow, CheckVerdict, TitleOutcome, check_summary};

#[test]
fn check_summary_names_the_verdict_and_the_conveyed_bumps() {
    let plan = sample_plan();
    let verdict = CheckVerdict {
        title: Some(TitleOutcome::Accepted {
            kind: "feat".to_owned(),
            level: "minor".to_owned(),
            predictions: vec![
                CheckRow {
                    package: "core".to_owned(),
                    from: "0.4.1".to_owned(),
                    bump: "minor".to_owned(),
                },
                CheckRow {
                    package: "app".to_owned(),
                    from: "1.0.0".to_owned(),
                    bump: "minor".to_owned(),
                },
            ],
        }),
    };
    let summary = check_summary(&plan, &verdict);
    assert!(
        summary.starts_with("## halyard check\n"),
        "the §12 heading: {summary}"
    );
    assert!(
        summary.contains("| core | 0.4.1 → 0.5.0 | minor | core-v0.5.0 |"),
        "the release table rides: {summary}"
    );
    assert!(
        summary.contains("title accepted: feat → minor"),
        "the verdict line: {summary}"
    );
    assert!(
        summary.contains("conveyed core minor, app minor"),
        "the conveyed bumps, plan order: {summary}"
    );
}

#[test]
fn check_summary_renders_the_rejected_and_no_title_postures() {
    let plan = sample_plan();
    let rejected = CheckVerdict {
        title: Some(TitleOutcome::Rejected {
            message: "not a conventional commit — expected `type: description`".to_owned(),
        }),
    };
    let summary = check_summary(&plan, &rejected);
    assert!(
        summary.contains("title rejected: not a conventional commit"),
        "the diagnosis rides: {summary}"
    );

    let none = CheckVerdict { title: None };
    let summary = check_summary(&plan, &none);
    assert!(
        summary.contains("no title to lint — the pending forecast is the verdict"),
        "the no-title posture: {summary}"
    );
    assert!(
        summary.contains("2 planned, 1 skipped"),
        "the counts ride in every posture: {summary}"
    );
}

// ---------------------------------------------------------------------------
// The §12 `release` mode's renderer (ADR-0046): the §11.1 tag phase's
// pass view — the git-side tags, the push posture and the forge half —
// through the same seam. The outputs ride the plan vocabulary (no PR
// keys — the release mode opens none); `released` is the pass's honest
// execution value. Written first (red on the unresolved API),
// implemented second.
// ---------------------------------------------------------------------------

use std::path::Path as ReleasePath;

use halyard_action::{ForgePosture, ReleasePass, release_outputs, release_summary};

/// A live release pass: two tags created, the push phase pushed them,
/// the forge half minted one URL (the other release's URL rides the
/// next machine pass — the honest per-tag map carries only what ran).
/// The created tags ride the tag phase's own name-sorted row order
/// (I3 — `app-v1.1.0` sorts before `core-v0.5.0`).
fn live_release_pass() -> ReleasePass {
    ReleasePass {
        released: true,
        forecast: false,
        created_tags: vec!["app-v1.1.0".to_owned(), "core-v0.5.0".to_owned()],
        release_urls: BTreeMap::from([(
            "core-v0.5.0".to_owned(),
            "https://github.com/example/monorepo/releases/tag/core-v0.5.0".to_owned(),
        )]),
        forge: ForgePosture::Ran {
            repo: "example/monorepo".to_owned(),
            releases: 1,
        },
    }
}

#[test]
fn release_outputs_carry_the_tag_phase_keys_in_documented_order() -> Result<(), BoxError> {
    let plan = sample_plan();
    let pass = live_release_pass();
    let outputs = release_outputs(&plan, ReleasePath::new("out/plan.json"), &pass)?;
    let flat: Vec<(String, String)> = outputs
        .iter()
        .map(|(name, value)| (name.to_string(), value.clone()))
        .collect();
    let entry = |idx: usize| -> Option<(String, String)> { flat.get(idx).cloned() };
    // The documented order: released, published, plan_json,
    // releases_json — no `pr_number`/`pr_url` (the release mode opens
    // no Release PR) — then the per-package keys and `paths_released`.
    assert_eq!(entry(0), Some(("released".to_owned(), "true".to_owned())));
    assert_eq!(entry(1), Some(("published".to_owned(), "false".to_owned())));
    assert_eq!(
        entry(2),
        Some(("plan_json".to_owned(), "out/plan.json".to_owned()))
    );
    assert!(entry(3).is_some_and(|(name, _)| name == "releases_json"));
    assert!(
        outputs.iter().all(|(name, _)| name.as_str() != "pr_number"),
        "the release mode opens no Release PR — no PR keys: {flat:?}"
    );
    assert!(
        flat.iter()
            .any(|(name, value)| name == "core--tag" && value == "core-v0.5.0"),
        "the per-package tag key: {flat:?}"
    );
    Ok(())
}

#[test]
fn release_outputs_fill_the_minted_urls_and_keep_the_honest_released_value() -> Result<(), BoxError>
{
    let plan = sample_plan();
    // Live pass: the forge-minted URL fills the core row by tag; the
    // app row keeps the empty url (its forge release lands on the next
    // pass or was already present — nothing invented).
    let outputs = release_outputs(
        &plan,
        ReleasePath::new("out/plan.json"),
        &live_release_pass(),
    )?;
    let releases = outputs
        .iter()
        .find(|(name, _)| name.as_str() == "releases_json")
        .map(|(_, value)| value.clone())
        .expect("releases_json is present");
    let rows: serde_json::Value = serde_json::from_str(&releases)?;
    let array = rows.as_array().expect("releases_json is an array");
    let url_of = |name: &str| -> Option<&serde_json::Value> {
        array
            .iter()
            .find(|row| row.get("name") == Some(&serde_json::json!(name)))
            .and_then(|row| row.get("url"))
    };
    assert_eq!(
        url_of("core"),
        Some(&serde_json::json!(
            "https://github.com/example/monorepo/releases/tag/core-v0.5.0"
        ))
    );
    assert_eq!(url_of("app"), Some(&serde_json::json!("")));

    // The dry forecast: `released` stays false — nothing was executed
    // (I5), even though the git-side rows are the forecast's own.
    let forecast = ReleasePass {
        released: false,
        forecast: true,
        created_tags: vec!["core-v0.5.0".to_owned()],
        release_urls: BTreeMap::from([(
            "core-v0.5.0".to_owned(),
            "preview://core-v0.5.0".to_owned(),
        )]),
        forge: ForgePosture::Skipped {
            reason: "dry run — the forge half is forecast, not executed (I5)".to_owned(),
        },
    };
    let outputs = release_outputs(&plan, ReleasePath::new("out/plan.json"), &forecast)?;
    let released_value = outputs
        .iter()
        .find(|(name, _)| name.as_str() == "released")
        .map(|(_, value)| value.clone())
        .expect("released is present");
    assert_eq!(released_value, "false");
    Ok(())
}

#[test]
fn release_summary_names_the_tags_the_push_and_the_forge_posture() {
    let plan = sample_plan();
    let summary = release_summary(&plan, &live_release_pass());
    assert!(
        summary.starts_with("## halyard release\n"),
        "the §12 heading: {summary}"
    );
    assert!(
        summary.contains("| core | 0.4.1 → 0.5.0 | minor | core-v0.5.0 |"),
        "the release table rides: {summary}"
    );
    assert!(
        summary.contains("created app-v1.1.0, core-v0.5.0"),
        "the created tags, name-sorted (I3): {summary}"
    );
    assert!(
        summary.contains("forge releases in example/monorepo (1 minted)"),
        "the forge posture: {summary}"
    );
    assert!(
        summary.contains("2 planned, 1 skipped"),
        "the honest counts ride: {summary}"
    );
    assert!(
        !summary.contains("forecast"),
        "a live pass is not labelled a forecast: {summary}"
    );

    // The dry forecast says so, and the honest skip names its reason.
    let forecast = ReleasePass {
        released: false,
        forecast: true,
        created_tags: vec!["core-v0.5.0".to_owned()],
        release_urls: BTreeMap::new(),
        forge: ForgePosture::Skipped {
            reason: "dry run — the forge half is forecast, not executed (I5)".to_owned(),
        },
    };
    let dry_summary = release_summary(&plan, &forecast);
    assert!(
        dry_summary.contains("forecast (nothing executed)"),
        "the forecast posture is stated: {dry_summary}"
    );
    assert!(
        dry_summary.contains(
            "forge releases skipped (dry run — the forge half is forecast, not executed (I5))"
        ),
        "the skip reason rides verbatim: {dry_summary}"
    );
}

// ---------------------------------------------------------------------------
// The §11.1 step-3 content effect on the §12 `auto` view (ADR-0047's
// `content_commit` machine input): the machine's own two content
// verdicts ride the plain pass view, and the pass line renders them.
// The plan binds the prepare phase to the auto command —
// docs/HALYARD_ARCHITECTURE.md §11.1: "On every push to the base branch
// the same command (`halyard release --auto`) runs" with step 3 "apply
// EditSet in memory → build tree → compare tree OID with remote release
// branch tip (`ls-remote`, 0 REST); if equal: update PR body only if
// plan_hash differs; else commit & `push --force-with-lease`". Written
// first (red on the unknown `content_effect` field), implemented second.
// ---------------------------------------------------------------------------

#[test]
fn auto_pass_line_renders_the_content_effect_of_the_prepare_half() {
    use std::collections::BTreeMap;

    use halyard_action::{AutoPass, ContentEffect, ForgePosture, PrEffect, auto_summary};

    let plan = sample_plan();
    let pass = |content: Option<ContentEffect>| AutoPass {
        released: false,
        propagated: false,
        forecast: true,
        created_tags: Vec::new(),
        pr_effect: Some(PrEffect::Opened {
            number: 0,
            url: "preview://halyard/release/main".to_owned(),
        }),
        content_effect: content,
        release_urls: BTreeMap::new(),
        forge: ForgePosture::Skipped {
            reason: "dry run — the forge half is forecast, not executed (I5)".to_owned(),
        },
    };

    // The prepare phase committed the content through the forge (the
    // live `api` mode, or the dry forecast's preview oid): the pass
    // line names it before the PR it enables (the §11.1 order).
    let committed = auto_summary(
        &plan,
        &pass(Some(ContentEffect::Committed {
            oid: "blake3:5fa1".to_owned(),
        })),
    );
    assert!(
        committed.contains(" · content tree committed (blake3:5fa1) · PR #0 opened"),
        "the committed content rides the pass line before the PR: {committed}"
    );

    // The I1 match (the git-side half pushed the content under the
    // §11.2 lease, or the branch was already current): the machine
    // spent no content call, and the line says so.
    let matched = auto_summary(
        &plan,
        &pass(Some(ContentEffect::Matched {
            tree_oid: "3fa1b2c4d5e6f7a8".to_owned(),
        })),
    );
    assert!(
        matched.contains(
            " · the release branch already carries the content tree 3fa1b2c4d5e6f7a8 · PR #0 opened"
        ),
        "the matched content rides the pass line: {matched}"
    );

    // No content effect (PROPAGATE pass, empty plan, the close probe):
    // the line keeps its documented shape — no content piece.
    let none = auto_summary(&plan, &pass(None));
    assert!(
        !none.contains("content tree"),
        "the absent effect renders nothing: {none}"
    );
}
