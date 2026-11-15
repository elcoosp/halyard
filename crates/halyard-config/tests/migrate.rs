//! The migration engine's contract (M4.7 step 1, §16.2): the
//! `MigrationSource` seam maps a foreign release tool's configuration
//! onto the `halyard.toml` v1 surface and reports the semantic
//! differences — the report is data, nothing is written, and every
//! survey is deterministic (I3). The foreign grammars are VERIFIED
//! against the release-please manifest-releaser doc + config schema and
//! the changesets config page (`docs/research/migrate-config-grammars.md`).

use std::collections::BTreeMap;

use halyard_config::migrate::{
    ForeignFiles, MigrationSource, ToolSurvey, default_sources, migrate,
};
use halyard_config::{ConfigError, load_from_str};

// ---------------------------------------------------------------------------
// Fixtures (the VERIFIED grammars)
// ---------------------------------------------------------------------------

/// A realistic release-please manifest config: the JSON the release-please
/// docs document (`release-please-config.json`, `packages` required).
fn release_please_config() -> &'static str {
    r#"{
  "$schema": "https://raw.githubusercontent.com/googleapis/release-please/main/schemas/config.json",
  "packages": {
    "crates/p1": {
      "package-name": "p1-crate",
      "changelog-path": "docs/CHANGELOG.md"
    },
    "crates/p2": {}
  },
  "bump-minor-pre-major": true,
  "bump-patch-for-minor-pre-major": true,
  "plugins": ["node-workspace"],
  "draft": true
}"#
}

fn release_please_manifest() -> &'static str {
    r#"{
  "crates/p1": "1.2.3",
  "crates/p2": "0.5.0"
}"#
}

fn files_with_release_please() -> ForeignFiles {
    let mut files = ForeignFiles::new();
    files.insert("release-please-config.json", release_please_config());
    files.insert(".release-please-manifest.json", release_please_manifest());
    files
}

/// The report's release-please survey (tool rows are name-sorted, so
/// selecting by name is the only honest lookup). The helper runs
/// outside a `#[test]` fn, so the unwrap/expect allowances do not
/// apply here — a missing survey is an `unreachable!` diagnosis.
fn rp(report: &halyard_config::MigrationReport) -> &ToolSurvey {
    match report.tools.iter().find(|t| t.tool == "release-please") {
        Some(survey) => survey,
        None => unreachable!("release-please survey present"),
    }
}

/// The report's changesets survey.
fn cs(report: &halyard_config::MigrationReport) -> &ToolSurvey {
    match report.tools.iter().find(|t| t.tool == "changesets") {
        Some(survey) => survey,
        None => unreachable!("changesets survey present"),
    }
}

// ---------------------------------------------------------------------------
// The seam
// ---------------------------------------------------------------------------

/// A test-local source proves the trait is a real downstream extension
/// point: `default_sources()` is a registry, not a closed enum.
#[derive(Debug)]
struct TestSource;

impl MigrationSource for TestSource {
    fn tool(&self) -> &'static str {
        "test-tool"
    }

    fn config_paths(&self) -> &'static [&'static str] {
        &["test-tool.json"]
    }

    fn survey(&self, files: &ForeignFiles) -> Result<ToolSurvey, ConfigError> {
        let configured = files.get("test-tool.json").is_some();
        Ok(ToolSurvey {
            tool: self.tool().to_owned(),
            configured,
            files: files.paths().cloned().collect(),
            rows: Vec::new(),
            unmapped: Vec::new(),
            packages: Vec::new(),
            draft: None,
        })
    }
}

#[test]
fn custom_source_runs_through_the_same_seam() {
    let mut files = ForeignFiles::new();
    files.insert("test-tool.json", "{}");
    let sources: Vec<Box<dyn MigrationSource>> = vec![Box::new(TestSource)];
    let report = migrate(&files, &sources).expect("custom source surveys");
    assert_eq!(report.tools.len(), 1);
    let survey = report.tools.first().expect("one survey");
    assert_eq!(survey.tool, "test-tool");
    assert!(survey.configured);
}

#[test]
fn report_is_sorted_by_tool_name() {
    let report =
        migrate(&ForeignFiles::new(), &default_sources()).expect("empty tree surveys every source");
    let names: Vec<&str> = report.tools.iter().map(|t| t.tool.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted);
}

#[test]
fn every_registered_source_declares_distinct_paths() {
    // The shell reads each source's declared files once; overlapping
    // declarations would double-count detection.
    let sources = default_sources();
    let mut seen: BTreeMap<&str, &str> = BTreeMap::new();
    for source in &sources {
        for path in source.config_paths() {
            assert!(
                seen.insert(path, source.tool()).is_none(),
                "path {path} declared twice"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// release-please
// ---------------------------------------------------------------------------

#[test]
fn release_please_maps_the_pre_major_pins() {
    let report = migrate(&files_with_release_please(), &default_sources())
        .expect("release-please config surveys");
    let survey = rp(&report);
    assert!(survey.configured);
    assert_eq!(
        survey.files,
        vec![
            ".release-please-manifest.json",
            "release-please-config.json"
        ]
    );

    let row = survey
        .rows
        .iter()
        .find(|r| r.foreign_key == "bump-minor-pre-major")
        .expect("bump-minor-pre-major row");
    assert_eq!(row.halyard_key, "versioning.pre_major.breaking");
    assert_eq!(row.value, "minor");
    assert!(row.note.as_deref().is_some());

    let row = survey
        .rows
        .iter()
        .find(|r| r.foreign_key == "bump-patch-for-minor-pre-major")
        .expect("bump-patch-for-minor-pre-major row");
    assert_eq!(row.halyard_key, "versioning.pre_major.feat");
    assert_eq!(row.value, "patch");
}

#[test]
fn release_please_defaults_pin_the_release_please_behavior() {
    // The schema's defaults (bump-minor-pre-major: false,
    // bump-patch-for-minor-pre-major: false) differ from halyard's
    // built-in §10.2 defaults — the pins preserve the foreign tool's
    // effective behavior, and the notes say so.
    let mut files = ForeignFiles::new();
    files.insert(
        "release-please-config.json",
        r#"{ "packages": { "crates/p1": {} } }"#,
    );
    let report = migrate(&files, &default_sources()).expect("minimal config surveys");
    let survey = rp(&report);
    let breaking = survey
        .rows
        .iter()
        .find(|r| r.foreign_key == "bump-minor-pre-major (default)")
        .expect("absent bump-minor-pre-major is pinned");
    assert_eq!(breaking.value, "major");
    assert!(
        breaking
            .note
            .as_deref()
            .unwrap_or_default()
            .contains("default")
    );
    let feat = survey
        .rows
        .iter()
        .find(|r| r.foreign_key == "bump-patch-for-minor-pre-major (default)")
        .expect("absent bump-patch-for-minor-pre-major is pinned");
    assert_eq!(feat.value, "minor");
}

#[test]
fn release_please_maps_the_default_tag_template() {
    // include-component-in-tag defaults to true, include-v-in-tag to
    // true, tag-separator to `-` (the documented
    // `<component-name>-v<release-version>` shape) — halyard's own
    // multi-package default template.
    let report = migrate(&files_with_release_please(), &default_sources())
        .expect("release-please config surveys");
    let survey = rp(&report);
    let row = survey
        .rows
        .iter()
        .find(|r| r.halyard_key == "repo.tag_style")
        .expect("tag template row");
    assert_eq!(row.value, "{name}-v{version}");
}

#[test]
fn release_please_tag_switches_remap_the_template() {
    // include-component-in-tag: false → `v{version}`; include-v-in-tag:
    // false → `{name}-{version}`; a custom tag-separator rides the
    // template grammar (appendix A.3).
    let mut files = ForeignFiles::new();
    files.insert(
        "release-please-config.json",
        r#"{ "packages": { "crates/p1": {} }, "include-component-in-tag": false }"#,
    );
    let report = migrate(&files, &default_sources()).expect("no-component config surveys");
    let row = rp(&report)
        .rows
        .iter()
        .find(|r| r.halyard_key == "repo.tag_style")
        .expect("tag template row");
    assert_eq!(row.value, "v{version}");

    let mut files = ForeignFiles::new();
    files.insert(
        "release-please-config.json",
        r#"{ "packages": { "crates/p1": {} }, "include-v-in-tag": false, "tag-separator": "_" }"#,
    );
    let report = migrate(&files, &default_sources()).expect("no-v config surveys");
    let row = rp(&report)
        .rows
        .iter()
        .find(|r| r.halyard_key == "repo.tag_style")
        .expect("tag template row");
    assert_eq!(row.value, "{name}_{version}");
}

#[test]
fn release_please_per_package_tag_disagreement_is_reported_not_guessed() {
    let mut files = ForeignFiles::new();
    files.insert(
        "release-please-config.json",
        r#"{ "packages": { "crates/p1": {}, "crates/p2": { "include-component-in-tag": false } } }"#,
    );
    let report = migrate(&files, &default_sources()).expect("mixed config surveys");
    let survey = rp(&report);
    assert!(
        survey
            .rows
            .iter()
            .all(|r| r.halyard_key != "repo.tag_style"),
        "no repo-wide template can represent the disagreement"
    );
    assert!(
        survey
            .unmapped
            .iter()
            .any(|u| u.foreign_key.contains("include-component-in-tag")
                && u.reason.contains("crates/p1")),
        "the disagreement names the packages: {:?}",
        survey.unmapped
    );
}

#[test]
fn release_please_lists_packages_and_manifest_versions() {
    let report = migrate(&files_with_release_please(), &default_sources())
        .expect("release-please config surveys");
    let survey = rp(&report);
    let p1 = survey
        .packages
        .iter()
        .find(|p| p.path == "crates/p1")
        .expect("p1 package row");
    assert_eq!(p1.component.as_deref(), Some("p1-crate"));
    assert!(p1.release);
    assert!(
        p1.notes.iter().any(|n| n.contains("1.2.3")),
        "the manifest's last release rides the notes: {:?}",
        p1.notes
    );
    assert!(survey.packages.iter().any(|p| p.path == "crates/p2"));
}

#[test]
fn release_please_reports_the_semantic_differences() {
    let report = migrate(&files_with_release_please(), &default_sources())
        .expect("release-please config surveys");
    let survey = rp(&report);
    // The package completeness difference (release-please releases only
    // the listed packages; halyard releases everything discovered).
    assert!(
        survey
            .unmapped
            .iter()
            .any(|u| u.foreign_key == "packages (completeness)")
    );
    // Per-package keys without a v1 equivalent.
    assert!(
        survey
            .unmapped
            .iter()
            .any(|u| u.foreign_key.contains("changelog-path") && u.reason.contains("CHANGELOG"))
    );
    // Documented keys the v1 surface cannot express, each with a reason.
    for key in ["plugins", "draft"] {
        assert!(
            survey.unmapped.iter().any(|u| u.foreign_key == key),
            "key {key} must carry a semantic-difference row: {:?}",
            survey.unmapped
        );
    }
    // `$schema` is an informational pin, never mapped into the draft.
    assert!(
        survey.rows.iter().all(|r| r.foreign_key != "$schema"),
        "the schema pin is not a config mapping"
    );
}

#[test]
fn release_please_draft_is_a_valid_halyard_config() {
    let report = migrate(&files_with_release_please(), &default_sources())
        .expect("release-please config surveys");
    let survey = rp(&report);
    let draft = survey.draft.as_deref().expect("a configured source drafts");
    let parsed = load_from_str(draft).expect("the draft parses as halyard.toml v1");
    let repo = parsed.repo.as_ref().expect("draft carries the tag style");
    assert_eq!(
        repo.tag_style.as_ref().map(|s| s.0.as_str()),
        Some("{name}-v{version}")
    );
    let pre_major = parsed
        .versioning
        .as_ref()
        .and_then(|v| v.pre_major.clone())
        .expect("draft carries the pre-major pins");
    assert_eq!(pre_major.breaking, Some(halyard_config::BumpLevel::Minor));
    assert_eq!(pre_major.feat, Some(halyard_config::BumpLevel::Patch));
}

// ---------------------------------------------------------------------------
// changesets
// ---------------------------------------------------------------------------

#[test]
fn changesets_maps_access_base_branch_and_cascade() {
    let mut files = ForeignFiles::new();
    files.insert(
        ".changeset/config.json",
        r#"{
  "access": "public",
  "baseBranch": "develop",
  "linked": [["@scope/a", "@scope/b"]],
  "ignore": ["@scope/internal-tool"],
  "changelog": ["github", { "repo": "org/repo" }]
}"#,
    );
    let report = migrate(&files, &default_sources()).expect("changesets config surveys");
    let survey = cs(&report);
    assert!(survey.configured);

    let access = survey
        .rows
        .iter()
        .find(|r| r.foreign_key == "access")
        .expect("access row");
    assert_eq!(access.halyard_key, "publish.npm.access");
    assert_eq!(access.value, "public");

    let base = survey
        .rows
        .iter()
        .find(|r| r.foreign_key == "baseBranch")
        .expect("baseBranch row");
    assert_eq!(base.halyard_key, "repo.base_branch");
    assert_eq!(base.value, "develop");

    let cascade = survey
        .rows
        .iter()
        .find(|r| r.halyard_key == "versioning.cascade")
        .expect("changesets always moves dependents");
    assert_eq!(cascade.value, "always");

    let ignore = survey
        .rows
        .iter()
        .find(|r| r.foreign_key == "ignore")
        .expect("ignore row");
    assert!(ignore.value.contains("@scope/internal-tool"));

    for key in ["linked", "changelog"] {
        assert!(
            survey.unmapped.iter().any(|u| u.foreign_key == key),
            "key {key} must carry a semantic-difference row: {:?}",
            survey.unmapped
        );
    }
    assert!(
        survey
            .unmapped
            .iter()
            .any(|u| u.reason.contains("not verified")),
        "the unverified changesets tag grammar stays an honest advisory: {:?}",
        survey.unmapped
    );
}

#[test]
fn changesets_draft_pins_its_defaults_and_exclusions() {
    let mut files = ForeignFiles::new();
    files.insert(
        ".changeset/config.json",
        r#"{ "ignore": ["@scope/internal-tool"] }"#,
    );
    let report = migrate(&files, &default_sources()).expect("changesets config surveys");
    let survey = cs(&report);
    let draft = survey.draft.as_deref().expect("a configured source drafts");
    let parsed = load_from_str(draft).expect("the draft parses as halyard.toml v1");
    // The changesets defaults (`baseBranch: "main"`, `access:
    // "restricted"`) are pinned, and the ignored package becomes a
    // `release = false` override.
    assert_eq!(
        parsed.repo.as_ref().and_then(|r| r.base_branch.clone()),
        Some("main".to_owned())
    );
    let npm = parsed
        .publish
        .as_ref()
        .and_then(|p| p.npm.clone())
        .expect("draft carries the npm access pin");
    assert_eq!(npm.access, Some(halyard_config::NpmAccess::Restricted));
    assert_eq!(parsed.package.len(), 1);
    let override_row = parsed.package.first().expect("one override");
    assert_eq!(override_row.name.as_deref(), Some("@scope/internal-tool"));
    assert_eq!(override_row.release, Some(false));
}

// ---------------------------------------------------------------------------
// Absence, failures, determinism
// ---------------------------------------------------------------------------

#[test]
fn absent_configuration_is_a_value_not_an_error() {
    let report =
        migrate(&ForeignFiles::new(), &default_sources()).expect("an empty tree is a valid survey");
    for survey in &report.tools {
        assert!(!survey.configured);
        assert_eq!(survey.rows, Vec::new());
        assert!(survey.draft.is_none());
    }
}

#[test]
fn a_malformed_foreign_config_fails_closed_with_its_path() {
    let mut files = ForeignFiles::new();
    files.insert("release-please-config.json", "{ not json");
    let error = migrate(&files, &default_sources()).expect_err("malformed JSON refuses the survey");
    assert_eq!(error.code(), ConfigError::MIGRATE_CODE);
    let rendered = error.to_string();
    assert!(
        rendered.contains("release-please-config.json"),
        "the failing file is named: {rendered}"
    );
}

#[test]
fn a_release_please_config_without_packages_refuses() {
    // The release-please schema marks `packages` required
    // (`additionalProperties: false` at the root).
    let mut files = ForeignFiles::new();
    files.insert("release-please-config.json", r#"{ "draft": true }"#);
    let error =
        migrate(&files, &default_sources()).expect_err("a config without `packages` refuses");
    assert_eq!(error.code(), ConfigError::MIGRATE_CODE);
}

#[test]
fn an_unknown_release_please_key_is_reported_not_dropped() {
    let mut files = ForeignFiles::new();
    files.insert(
        "release-please-config.json",
        r#"{ "packages": { "crates/p1": {} }, "signoff": "Signed-off-by: x <x@y.z>" }"#,
    );
    let report = migrate(&files, &default_sources()).expect("config surveys");
    let survey = rp(&report);
    assert!(
        survey
            .unmapped
            .iter()
            .any(|u| u.foreign_key == "signoff" && !u.reason.is_empty()),
        "every documented-but-unmapped key reports: {:?}",
        survey.unmapped
    );
}

#[test]
fn surveys_are_deterministic() {
    let report = migrate(&files_with_release_please(), &default_sources())
        .expect("release-please config surveys");
    let again = migrate(&files_with_release_please(), &default_sources())
        .expect("release-please config surveys");
    assert_eq!(report, again);
}

#[test]
fn foreign_files_lookup_is_exact_and_paths_are_sorted() {
    let mut files = ForeignFiles::new();
    files.insert("z.json", "{}");
    files.insert("a.json", "{}");
    assert_eq!(files.get("a.json"), Some("{}"));
    assert_eq!(files.get("missing.json"), None);
    let paths: Vec<String> = files.paths().cloned().collect();
    assert_eq!(paths, vec!["a.json".to_owned(), "z.json".to_owned()]);
}
