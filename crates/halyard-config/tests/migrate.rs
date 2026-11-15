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

// ---------------------------------------------------------------------------
// knope (the §16.2 extension: the VERIFIED grammar lives in
// docs/research/migrate-config-grammars.md — the config pages quote the
// `[package]`/`[packages]` shapes, the Release step page quotes the tag
// rules)
// ---------------------------------------------------------------------------

/// The report's knope survey.
fn kn(report: &halyard_config::MigrationReport) -> &ToolSurvey {
    match report.tools.iter().find(|t| t.tool == "knope") {
        Some(survey) => survey,
        None => unreachable!("knope survey present"),
    }
}

/// The report's release-plz survey.
fn plz(report: &halyard_config::MigrationReport) -> &ToolSurvey {
    match report.tools.iter().find(|t| t.tool == "release-plz") {
        Some(survey) => survey,
        None => unreachable!("release-plz survey present"),
    }
}

#[test]
fn default_registry_surveys_all_four_foreign_tools() {
    let sources = default_sources();
    let mut names: Vec<&str> = sources.iter().map(|s| s.tool()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["changesets", "knope", "release-please", "release-plz"],
        "the registry carries the four §16.2 tools (sorted here for the pin)"
    );
}

#[test]
fn knope_absent_surveys_not_configured() {
    let report = migrate(&ForeignFiles::new(), &default_sources()).expect("empty tree surveys");
    let survey = kn(&report);
    assert!(!survey.configured, "absence is a value, not an error");
    assert!(survey.rows.is_empty() && survey.packages.is_empty());
    assert!(
        survey.draft.is_none(),
        "an absent tool drafts nothing (the default survey shape)"
    );
}

#[test]
fn knope_single_package_maps_the_documented_v_tag() {
    let mut files = ForeignFiles::new();
    files.insert(
        "knope.toml",
        r#"
[package]
versioned_files = ["Cargo.toml"]
changelog = "CHANGELOG.md"

[[workflows]]
name = "release"
"#,
    );
    let report = migrate(&files, &default_sources()).expect("knope config surveys");
    let survey = kn(&report);
    assert!(survey.configured);
    assert_eq!(survey.files, vec!["knope.toml".to_owned()]);
    let tag = survey
        .rows
        .iter()
        .find(|r| r.halyard_key == "repo.tag_style")
        .unwrap_or_else(|| unreachable!("the single-package tag rule maps"));
    assert_eq!(tag.value, "v{version}");
    assert!(
        tag.note
            .as_deref()
            .is_some_and(|note| note.contains("v{version}")),
        "the note names the documented tag shape: {tag:?}"
    );
    // The draft carries the mapped tag style (and was validated through
    // the strict loader by render_draft's own contract).
    let draft = survey.draft.as_deref().unwrap_or_default();
    assert!(
        draft.contains("tag_style = \"v{version}\""),
        "the draft pins the tag style: {draft}"
    );
    // The versioned files ride the package survey's notes (halyard
    // rewrites manifests per ecosystem; the foreign list is provenance).
    assert_eq!(survey.packages.len(), 1);
    let package = survey
        .packages
        .first()
        .unwrap_or_else(|| unreachable!("one package"));
    assert!(
        package.notes.iter().any(|note| note.contains("Cargo.toml")),
        "versioned_files surface as provenance notes: {:?}",
        package.notes
    );
}

#[test]
fn knope_packages_map_the_slash_tag_shape() {
    let mut files = ForeignFiles::new();
    files.insert(
        "knope.toml",
        r#"
[packages."left"]
versioned_files = ["Cargo.toml"]
changelog = "left/CHANGELOG.md"

[packages."right"]
versioned_files = ["right/package.json"]
"#,
    );
    let report = migrate(&files, &default_sources()).expect("knope packages survey");
    let survey = kn(&report);
    assert!(survey.configured);
    let tag = survey
        .rows
        .iter()
        .find(|r| r.halyard_key == "repo.tag_style")
        .unwrap_or_else(|| unreachable!("the unified multi-package tag maps"));
    assert_eq!(tag.value, "{name}/v{version}");
    // Every configured package gets a survey row with the same resolved
    // template (the documented multi-package rule is uniform).
    assert_eq!(survey.packages.len(), 2);
    for package in &survey.packages {
        assert_eq!(package.tag, "{name}/v{version}");
        assert_eq!(
            package.component.as_deref(),
            Some(package.path.as_str()),
            "the knope package name is the component"
        );
    }
}

#[test]
fn knope_workflows_and_versioned_files_report_their_differences() {
    let mut files = ForeignFiles::new();
    files.insert(
        "knope.toml",
        r#"
[package]
versioned_files = ["Cargo.toml"]
scopes = ["engine"]
extra_changelog_sections = ["docs"]
assets = [{ path = "dist/halyard", name = "halyard" }]

[[workflows]]
name = "release"

[[workflows]]
name = "verify"
"#,
    );
    let report = migrate(&files, &default_sources()).expect("knope config surveys");
    let survey = kn(&report);
    for key in [
        "versioned_files",
        "scopes",
        "extra_changelog_sections",
        "assets",
    ] {
        assert!(
            survey
                .unmapped
                .iter()
                .any(|u| u.foreign_key == key && !u.reason.is_empty()),
            "`{key}` reports its semantic difference: {:?}",
            survey.unmapped
        );
    }
    let workflows = survey
        .unmapped
        .iter()
        .find(|u| u.foreign_key == "workflows")
        .unwrap_or_else(|| unreachable!("workflows report"));
    assert!(
        workflows.reason.contains("release") && workflows.reason.contains("verify"),
        "the workflow names ride the reason: {workflows:?}"
    );
}

#[test]
fn knope_unparseable_config_fails_closed() {
    let mut files = ForeignFiles::new();
    files.insert("knope.toml", "[package\nversioned_files = [");
    let error = migrate(&files, &default_sources()).expect_err("a broken config refuses");
    match &error {
        ConfigError::Migrate { path, message } => {
            assert_eq!(path, "knope.toml");
            assert_ne!(message, "");
        }
        other => unreachable!("E-CFG-MIGRATE, got {other:?}"),
    }
}

#[test]
fn knope_both_package_shapes_fail_closed() {
    let mut files = ForeignFiles::new();
    files.insert(
        "knope.toml",
        r#"
[package]
versioned_files = ["Cargo.toml"]

[packages."other"]
versioned_files = ["other/Cargo.toml"]
"#,
    );
    let error = migrate(&files, &default_sources()).expect_err("the two shapes are exclusive");
    assert!(
        matches!(&error, ConfigError::Migrate { path, .. } if path == "knope.toml"),
        "E-CFG-MIGRATE naming the file: {error:?}"
    );
}

#[test]
fn knope_packageless_workflow_config_has_no_tag_row() {
    let mut files = ForeignFiles::new();
    files.insert(
        "knope.toml",
        r#"
[[workflows]]
name = "release"
"#,
    );
    let report = migrate(&files, &default_sources()).expect("knope config surveys");
    let survey = kn(&report);
    assert!(survey.configured);
    assert!(
        !survey
            .rows
            .iter()
            .any(|r| r.halyard_key == "repo.tag_style"),
        "no package section means no verified tag shape — no mapping, no guess"
    );
    assert!(
        survey
            .unmapped
            .iter()
            .any(|u| u.foreign_key.contains("package")),
        "the advisory names the package gap: {:?}",
        survey.unmapped
    );
}

// ---------------------------------------------------------------------------
// release-plz (the §16.2 extension: the VERIFIED grammar lives in
// docs/research/migrate-config-grammars.md — the config page quotes the
// file search order, the `[workspace]` defaults and the Tera templates)
// ---------------------------------------------------------------------------

#[test]
fn release_plz_absent_surveys_not_configured() {
    let report = migrate(&ForeignFiles::new(), &default_sources()).expect("empty tree surveys");
    let survey = plz(&report);
    assert!(!survey.configured, "absence is a value, not an error");
    assert!(survey.rows.is_empty() && survey.packages.is_empty());
    assert!(
        survey.draft.is_none(),
        "an absent tool drafts nothing (the default survey shape)"
    );
}

#[test]
fn release_plz_reads_the_documented_search_order_first_match_only() {
    let mut files = ForeignFiles::new();
    files.insert("release-plz.toml", "[workspace]\n");
    files.insert(".release-plz.toml", "[workspace]\n");
    let report = migrate(&files, &default_sources()).expect("release-plz config surveys");
    let survey = plz(&report);
    assert_eq!(
        survey.files,
        vec!["release-plz.toml".to_owned()],
        "the first existing path in the documented order wins; the shadowed file is not read"
    );

    let mut only_dot_config = ForeignFiles::new();
    only_dot_config.insert(".config/release-plz.toml", "[workspace]\n");
    let report = migrate(&only_dot_config, &default_sources()).expect("dot-config surveys");
    assert_eq!(
        plz(&report).files,
        vec![".config/release-plz.toml".to_owned()],
        "the third documented search path is honored"
    );
}

#[test]
fn release_plz_default_tag_maps_the_workspace_default_template() {
    let mut files = ForeignFiles::new();
    files.insert("release-plz.toml", "[workspace]\n");
    let report = migrate(&files, &default_sources()).expect("release-plz config surveys");
    let survey = plz(&report);
    let tag = survey
        .rows
        .iter()
        .find(|r| r.halyard_key == "repo.tag_style")
        .unwrap_or_else(|| unreachable!("the documented workspace default maps"));
    assert_eq!(tag.value, "{name}-v{version}");
    assert_eq!(tag.foreign_key, "git_tag_name (workspace default)");
    assert!(
        survey
            .draft
            .as_deref()
            .is_some_and(|d| d.contains("{name}-v{version}")),
        "the draft carries the mapped template: {:?}",
        survey.draft
    );
}

#[test]
fn release_plz_configured_tag_templates_translate_to_the_a3_grammar() {
    for (tera, expected) in [
        ("\"v{{ version }}\"", "v{version}"),
        ("\"{{ package }}-v{{ version }}\"", "{name}-v{version}"),
        ("\"{{ package }}/{{ version }}\"", "{name}/{version}"),
    ] {
        let mut files = ForeignFiles::new();
        files.insert(
            "release-plz.toml",
            format!("[workspace]\ngit_tag_name = {tera}\n"),
        );
        let report = migrate(&files, &default_sources())
            .unwrap_or_else(|error| unreachable!("config surveys: {error}"));
        let survey = plz(&report);
        let tag = survey
            .rows
            .iter()
            .find(|r| r.halyard_key == "repo.tag_style")
            .unwrap_or_else(|| unreachable!("{tera} translates"));
        assert_eq!(tag.value, expected, "{tera} → {expected}");
        assert_eq!(tag.foreign_key, "git_tag_name");
    }
}

#[test]
fn release_plz_foreign_tera_variables_are_unmapped_not_translated() {
    let mut files = ForeignFiles::new();
    files.insert(
        "release-plz.toml",
        "[workspace]\ngit_tag_name = \"{{ package }}-{{ unknown_var }}\"\n",
    );
    let report = migrate(&files, &default_sources()).expect("release-plz config surveys");
    let survey = plz(&report);
    assert!(
        !survey
            .rows
            .iter()
            .any(|r| r.halyard_key == "repo.tag_style"),
        "a template the v1 grammar cannot express is never half-translated"
    );
    assert!(
        survey
            .unmapped
            .iter()
            .any(|u| u.foreign_key == "git_tag_name"),
        "the untranslatable template reports: {:?}",
        survey.unmapped
    );
}

#[test]
fn release_plz_tag_enable_false_refuses_the_tag_mapping() {
    let mut files = ForeignFiles::new();
    files.insert(
        "release-plz.toml",
        "[workspace]\ngit_tag_enable = false\ngit_tag_name = \"v{{ version }}\"\n",
    );
    let report = migrate(&files, &default_sources()).expect("release-plz config surveys");
    let survey = plz(&report);
    assert!(
        !survey
            .rows
            .iter()
            .any(|r| r.halyard_key == "repo.tag_style"),
        "a tool that creates no tags cannot pin a tag style"
    );
    assert!(
        survey
            .unmapped
            .iter()
            .any(|u| u.foreign_key == "git_tag_enable"),
        "the disabled tagging reports: {:?}",
        survey.unmapped
    );
}

#[test]
fn release_plz_release_false_becomes_the_package_override() {
    let mut files = ForeignFiles::new();
    files.insert(
        "release-plz.toml",
        "[workspace]\n\n[[package]]\nname = \"a\"\nrelease = false\n",
    );
    let report = migrate(&files, &default_sources()).expect("release-plz config surveys");
    let survey = plz(&report);
    let release = survey
        .rows
        .iter()
        .find(|r| r.halyard_key == "[[package]].release")
        .unwrap_or_else(|| unreachable!("release = false maps"));
    assert!(
        release.value.contains('a'),
        "the override names the package"
    );
    assert!(
        survey.packages.iter().any(|p| p.path == "a" && !p.release),
        "the package survey carries the foreign release decision: {:?}",
        survey.packages
    );
    let draft = survey.draft.as_deref().unwrap_or_default();
    assert!(
        draft.contains("[[package]]") && draft.contains("release = false"),
        "the draft carries the override: {draft}"
    );
    // The publish master switch is never set by a migration (I7).
    assert!(
        !draft.contains("publish"),
        "publishing stays opt-in: {draft}"
    );
}

#[test]
fn release_plz_changelog_defaults_to_keep_a_changelog() {
    let mut files = ForeignFiles::new();
    files.insert("release-plz.toml", "[workspace]\n");
    let report = migrate(&files, &default_sources()).expect("release-plz config surveys");
    let survey = plz(&report);
    let style = survey
        .rows
        .iter()
        .find(|r| r.halyard_key == "changelog.style")
        .unwrap_or_else(|| unreachable!("the documented changelog default maps"));
    assert_eq!(style.value, "keep-a-changelog");

    let mut off = ForeignFiles::new();
    off.insert(
        "release-plz.toml",
        "[workspace]\nchangelog_update = false\n",
    );
    let report = migrate(&off, &default_sources()).expect("release-plz config surveys");
    let survey = plz(&report);
    assert!(
        !survey
            .rows
            .iter()
            .any(|r| r.halyard_key == "changelog.style"),
        "changelog updates disabled means no style pin"
    );
}

#[test]
fn release_plz_publish_stays_unmapped_and_opt_in() {
    let mut files = ForeignFiles::new();
    files.insert("release-plz.toml", "[workspace]\npublish = true\n");
    let report = migrate(&files, &default_sources()).expect("release-plz config surveys");
    let survey = plz(&report);
    let publish = survey
        .unmapped
        .iter()
        .find(|u| u.foreign_key == "publish")
        .unwrap_or_else(|| unreachable!("publish reports its difference"));
    assert!(
        publish.reason.contains("opt-in"),
        "the reason names the halyard convention: {publish:?}"
    );
}

#[test]
fn release_plz_per_package_tag_disagreement_is_reported_never_guessed() {
    let mut files = ForeignFiles::new();
    files.insert(
        "release-plz.toml",
        "[workspace]\n\n[[package]]\nname = \"a\"\ngit_tag_name = \"a-v{{ version }}\"\n\n\
         [[package]]\nname = \"b\"\ngit_tag_name = \"v{{ version }}\"\n",
    );
    let report = migrate(&files, &default_sources()).expect("release-plz config surveys");
    let survey = plz(&report);
    assert!(
        !survey
            .rows
            .iter()
            .any(|r| r.halyard_key == "repo.tag_style"),
        "a per-package disagreement cannot pin one repo-wide tag_style"
    );
    assert!(
        survey
            .unmapped
            .iter()
            .any(|u| u.foreign_key.contains("git_tag_name")),
        "the disagreement reports: {:?}",
        survey.unmapped
    );
    assert_eq!(survey.packages.len(), 2, "both packages survey");
}

#[test]
fn release_plz_unparseable_config_fails_closed() {
    let mut files = ForeignFiles::new();
    files.insert("release-plz.toml", "[workspace\nbroken = ");
    let error = migrate(&files, &default_sources()).expect_err("a broken config refuses");
    assert!(
        matches!(&error, ConfigError::Migrate { path, .. } if path == "release-plz.toml"),
        "E-CFG-MIGRATE naming the file: {error:?}"
    );
}

#[test]
fn release_plz_package_entry_without_a_name_fails_closed() {
    let mut files = ForeignFiles::new();
    files.insert("release-plz.toml", "[[package]]\nrelease = false\n");
    let error = migrate(&files, &default_sources()).expect_err("the schema marks name required");
    assert!(
        matches!(&error, ConfigError::Migrate { path, .. } if path == "release-plz.toml"),
        "E-CFG-MIGRATE naming the file: {error:?}"
    );
}

#[test]
fn release_plz_version_group_is_a_review_row_not_a_policy_pin() {
    let mut files = ForeignFiles::new();
    files.insert(
        "release-plz.toml",
        "[workspace]\n\n[[package]]\nname = \"a\"\nversion_group = \"group1\"\n",
    );
    let report = migrate(&files, &default_sources()).expect("release-plz config surveys");
    let survey = plz(&report);
    let group = survey
        .unmapped
        .iter()
        .find(|u| u.foreign_key == "version_group")
        .unwrap_or_else(|| unreachable!("version_group reports"));
    assert!(
        group.reason.contains("linked"),
        "the reason points at halyard's linked policy: {group:?}"
    );
}

#[test]
fn release_plz_and_knope_surveys_are_deterministic() {
    let mut files = ForeignFiles::new();
    files.insert(
        "knope.toml",
        "[package]\nversioned_files = [\"Cargo.toml\"]\n",
    );
    files.insert(
        "release-plz.toml",
        "[workspace]\n\n[[package]]\nname = \"a\"\nrelease = false\n",
    );
    let report = migrate(&files, &default_sources()).expect("the configs survey");
    let again = migrate(&files, &default_sources()).expect("the configs survey again");
    assert_eq!(report, again, "I3: the survey is deterministic");
}
