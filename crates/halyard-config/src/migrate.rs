//! Foreign release-tool migration (M4.7 step 1, §16.2 `migrate`): read a
//! foreign release tool's configuration, map what the `halyard.toml` v1
//! surface can express, and report the semantic differences — the
//! report ([`MigrationReport`]) is plain data, the command renders it
//! and writes nothing (the §16.2 write step lands with its own slice;
//! the draft config rides the report as a string).
//!
//! The seam ([`MigrationSource`]) is the extension point the later
//! migrate slices fill (knope/release-plz per §16.2): one source per
//! tool, each declaring the files it reads and mapping them onto the
//! frozen v1 model. [`default_sources`] is the registry the shell
//! surveys; [`migrate`] runs any slice of sources over the discovered
//! files, so a downstream caller can migrate one tool in isolation.
//!
//! Conventions:
//!
//! * absence is a value — a tool whose files are not present surveys as
//!   `configured: false`, never an error;
//! * a configured-but-unparseable file fails closed with the stable
//!   `E-CFG-MIGRATE` code ([`ConfigError::Migrate`]) naming the file;
//! * foreign keys that the v1 surface cannot express are rows in
//!   [`ToolSurvey::unmapped`] with a reason each — never silently
//!   dropped ("errors teach", §11.6);
//! * the pins preserve the *foreign tool's effective behavior*
//!   (including its documented defaults, which sometimes differ from
//!   halyard's — the notes say so), so the migrated config behaves the
//!   way the old tool did;
//! * every survey is deterministic (I3): `BTreeMap` iteration, sorted
//!   rows, `PartialEq`-comparable output.
//!
//! The foreign grammars are VERIFIED per §0.4 against the
//! release-please manifest-releaser documentation and config schema and
//! the changesets config documentation — the quotes and the open
//! VERIFY items live in `docs/research/migrate-config-grammars.md`
//! (notably: the changesets git-tag grammar is *not* verified; the
//! report says so instead of guessing).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::ConfigError;
use crate::loader::load_from_str;
use crate::model::{
    BumpLevel, HalyardConfig, NpmAccess, PackageOverride, PreMajorCfg, RepoCfg, TagStyle,
};
use halyard_core::TagTemplate;

// ---------------------------------------------------------------------------
// Inputs, outputs and the seam
// ---------------------------------------------------------------------------

/// The foreign configuration files the shell discovered, keyed by their
/// repo-relative path (the paths [`MigrationSource::config_paths`]
/// declares). The shell owns the I/O; the engine stays pure.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ForeignFiles(BTreeMap<String, String>);

impl ForeignFiles {
    /// An empty set.
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Records one discovered file's contents.
    pub fn insert(&mut self, path: impl Into<String>, content: impl Into<String>) {
        self.0.insert(path.into(), content.into());
    }

    /// The contents of one declared path, if it was discovered.
    pub fn get(&self, path: &str) -> Option<&str> {
        self.0.get(path).map(String::as_str)
    }

    /// The discovered paths, sorted (the map's own order).
    pub fn paths(&self) -> impl Iterator<Item = &String> {
        self.0.keys()
    }
}

/// One foreign release tool's migration surface (M4.7): the files it
/// reads and the mapping of its configuration onto the `halyard.toml`
/// v1 model. Object-safe so the shell and tests can drive slices of
/// sources through [`migrate`].
pub trait MigrationSource: std::fmt::Debug {
    /// The stable tool name the report carries (e.g. `release-please`).
    fn tool(&self) -> &'static str;

    /// The repo-relative file paths this tool's configuration lives at.
    /// The shell looks exactly these up and feeds what it finds into
    /// [`MigrationSource::survey`].
    fn config_paths(&self) -> &'static [&'static str];

    /// Surveys the discovered files into the tool's report row. A
    /// missing configuration surveys as `configured: false` (absence is
    /// a value); a configured-but-unparseable file refuses with
    /// [`ConfigError::Migrate`].
    fn survey(&self, files: &ForeignFiles) -> Result<ToolSurvey, ConfigError>;
}

/// The registered sources the shell surveys (`M4.7 step 1` ships
/// release-please and changesets; the later migrate slices add knope /
/// release-plz per §16.2).
pub fn default_sources() -> Vec<Box<dyn MigrationSource>> {
    vec![Box::new(ReleasePleaseSource), Box::new(ChangesetsSource)]
}

/// Runs the given sources over the discovered files. Tool rows are
/// name-sorted so the report cannot depend on registry order (I3).
pub fn migrate(
    files: &ForeignFiles,
    sources: &[Box<dyn MigrationSource>],
) -> Result<MigrationReport, ConfigError> {
    let mut tools = Vec::with_capacity(sources.len());
    for source in sources {
        tools.push(source.survey(files)?);
    }
    tools.sort_by(|a, b| a.tool.cmp(&b.tool));
    Ok(MigrationReport { tools })
}

/// The migration report (M4.7 step 1): one survey per source, the
/// foreign tool's mapped rows, its unmapped semantic differences and a
/// draft `halyard.toml` that was validated through the strict loader —
/// rendered by the command, written to disk by nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationReport {
    /// One survey per registered source, sorted by tool name.
    pub tools: Vec<ToolSurvey>,
}

/// One foreign tool's survey.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSurvey {
    /// The stable tool name.
    pub tool: String,
    /// Whether any of the tool's declared files were discovered.
    /// Absence is a value, not an error.
    pub configured: bool,
    /// The tool's declared files that were discovered, sorted.
    pub files: Vec<String>,
    /// Foreign keys the v1 surface can express, with the halyard key
    /// and the pinned value (sorted by foreign key).
    pub rows: Vec<MappingRow>,
    /// Foreign keys (or whole behaviors) the v1 surface cannot express,
    /// each with a reason (sorted by foreign key).
    pub unmapped: Vec<UnmappedRow>,
    /// The foreign tool's per-package configuration, one row per
    /// configured package (sorted by path).
    pub packages: Vec<PackageSurvey>,
    /// A draft `halyard.toml` carrying the mapped values — validated
    /// through the strict loader before it is surfaced, and never
    /// written to disk by this slice.
    pub draft: Option<String>,
}

/// One mapped foreign key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappingRow {
    /// The foreign key (a synthetic label for composite/default rows).
    pub foreign_key: String,
    /// The `halyard.toml` key the value maps onto.
    pub halyard_key: String,
    /// The pinned value (rendered).
    pub value: String,
    /// Why this value — in particular when the foreign tool's default
    /// differs from halyard's.
    pub note: Option<String>,
}

/// One unmapped foreign key or behavior.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnmappedRow {
    /// The foreign key (or a synthetic label for whole behaviors).
    pub foreign_key: String,
    /// What the difference means and what to do instead.
    pub reason: String,
}

/// One foreign-configured package.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageSurvey {
    /// The package path (release-please's `packages` key).
    pub path: String,
    /// The foreign tool's component name (`package-name`/`component`),
    /// when configured.
    pub component: Option<String>,
    /// The resolved tag template for this package (appendix A.3).
    pub tag: String,
    /// Whether the foreign tool releases this package.
    pub release: bool,
    /// Informational provenance (e.g. the manifest's last release).
    pub notes: Vec<String>,
}

// ---------------------------------------------------------------------------
// release-please
// ---------------------------------------------------------------------------

/// The release-please manifest config (`release-please-config.json`,
/// `.release-please-manifest.json` — VERIFIED against the
/// manifest-releaser documentation and the config schema).
#[derive(Debug)]
pub struct ReleasePleaseSource;

const RP_CONFIG: &str = "release-please-config.json";
const RP_MANIFEST: &str = ".release-please-manifest.json";

/// Foreign keys that carry no `halyard.toml` equivalent, each with the
/// reason the report shows. Keys outside this table (and outside the
/// mapped set) get the generic reason.
const RP_UNMAPPED_ROOT: &[(&str, &str)] = &[
    (
        "$schema",
        "editor-support schema pin; no halyard equivalent",
    ),
    (
        "always-link-local",
        "workspace-internal requirement linking is halyard's rewriter behavior (appendix A.2)",
    ),
    (
        "bootstrap-sha",
        "release-please's history bootstrap boundary; halyard derives release bases from tags (§8.2)",
    ),
    (
        "changelog-host",
        "the changelog host choice rides the §9.4 styles",
    ),
    (
        "changelog-sections",
        "halyard maps commit types through `commits.types` (§16.4) and the §9.4 styles",
    ),
    (
        "changelog-type",
        "the changelog type choice rides the §9.4 styles",
    ),
    (
        "commit-batch-size",
        "release-please API-budget knob; halyard's §11.5 governor paces the forge",
    ),
    (
        "commit-search-depth",
        "release-please API-budget knob; halyard's §11.5 governor paces the forge",
    ),
    (
        "draft",
        "draft GitHub releases are a forge-side option; halyard's release shell publishes tags and reports the forge half (§11.1)",
    ),
    (
        "draft-pull-request",
        "draft Release PRs are not a v1 key (§11.3)",
    ),
    (
        "exclude-paths",
        "review `[[package]].exclude` (§8.4) — the glob dialects differ, so the report does not map them for you",
    ),
    (
        "extra-files",
        "version rewriting beyond manifests and lockfiles is not a v1 surface (§9)",
    ),
    (
        "extra-label",
        "Release-PR labels land with the §11.2 branch/label work",
    ),
    (
        "force-tag-creation",
        "halyard's tag phase is create-only and idempotent (I1); forced moves are refused fail-closed (E-GIT-TAG-CONFLICT)",
    ),
    (
        "group-pull-request-title-pattern",
        "the §11.3 body renderer owns the Release-PR shape; custom patterns are not a v1 key",
    ),
    (
        "include-commit-authors",
        "not a v1 changelog feature (§9.4)",
    ),
    (
        "include-v-in-release-name",
        "the release-name shape is not a v1 key (§11.1)",
    ),
    (
        "label",
        "Release-PR labels land with the §11.2 branch/label work",
    ),
    (
        "last-release-sha",
        "release-please's pinned last-release commit; halyard derives release bases from tags (§8.2)",
    ),
    (
        "plugins",
        "release-please plugins (node-workspace, linked-versions, …) have no v1 equivalent — the linked-versions core case maps to §5.4 `linked`",
    ),
    (
        "prerelease",
        "release trains (`next`/`rc` channels) land with M5.6",
    ),
    (
        "prerelease-type",
        "release trains (`next`/`rc` channels) land with M5.6",
    ),
    (
        "pull-request-footer",
        "the §11.3 body renderer owns the Release-PR shape; custom patterns are not a v1 key",
    ),
    (
        "pull-request-header",
        "the §11.3 body renderer owns the Release-PR shape; custom patterns are not a v1 key",
    ),
    (
        "pull-request-title-pattern",
        "the §11.3 body renderer owns the Release-PR shape; custom patterns are not a v1 key",
    ),
    (
        "release-as",
        "halyard's planner can pin a release-as bump (E-PLAN-RELEASE-AS); the release-please value shapes differ — set the halyard override deliberately",
    ),
    (
        "release-label",
        "Release-PR labels land with the §11.2 branch/label work",
    ),
    (
        "release-search-depth",
        "release-please API-budget knob; halyard's §11.5 governor paces the forge",
    ),
    (
        "release-type",
        "the release-please language strategy (`node`, `go`, …) — halyard's discovery derives the ecosystem per package (§6)",
    ),
    (
        "separate-pull-requests",
        "halyard opens one Release PR per plan (§11.3)",
    ),
    (
        "sequential-calls",
        "release-please API-budget knob; halyard's §11.5 governor paces the forge",
    ),
    ("signoff", "commit signoff is a forge/PR-side option"),
    (
        "skip-changelog",
        "halyard renders changelogs per §9.4; suppression is not a v1 key",
    ),
    (
        "skip-github-release",
        "halyard's tag phase and forge release half are wired per §11.1 — suppression is per-package `release = false` (§16.4)",
    ),
    ("skip-snapshot", "java snapshot flows are not a v1 surface"),
    ("snapshot-label", "java snapshot flows are not a v1 surface"),
    (
        "version-file",
        "version rewriting beyond manifests and lockfiles is not a v1 surface (§9)",
    ),
    (
        "versioning",
        "release-please's own strategy enum (e.g. `always-bump-minor`); halyard's §5.4 policies (`independent`/`fixed`/`linked`) are expressed differently — review",
    ),
];

/// Per-package foreign keys that carry no v1 equivalent (the schema's
/// `ReleaserConfigOptions` minus the mapped/tag keys below).
const RP_UNMAPPED_PACKAGE: &[(&str, &str)] = &[
    (
        "changelog-host",
        "the changelog host choice rides the §9.4 styles",
    ),
    (
        "changelog-path",
        "the per-package changelog path (`CHANGELOG.md` default) has no v1 equivalent — halyard renders one changelog per package (§9.4)",
    ),
    (
        "changelog-type",
        "the changelog type choice rides the §9.4 styles",
    ),
    (
        "component-no-space",
        "the rendered component shape is not a v1 key (§16.4)",
    ),
    ("date-format", "the date format is not a v1 key (§16.4)"),
    (
        "draft",
        "draft GitHub releases are a forge-side option (§11.1)",
    ),
    (
        "draft-pull-request",
        "draft Release PRs are not a v1 key (§11.3)",
    ),
    (
        "exclude-paths",
        "review `[[package]].exclude` (§8.4) — the glob dialects differ, so the report does not map them for you",
    ),
    (
        "extra-files",
        "version rewriting beyond manifests and lockfiles is not a v1 surface (§9)",
    ),
    (
        "extra-label",
        "Release-PR labels land with the §11.2 branch/label work",
    ),
    (
        "force-tag-creation",
        "halyard's tag phase is create-only and idempotent (I1)",
    ),
    (
        "include-commit-authors",
        "not a v1 changelog feature (§9.4)",
    ),
    (
        "include-v-in-release-name",
        "the release-name shape is not a v1 key (§11.1)",
    ),
    (
        "initial-version",
        "the initial version pin is not a v1 key (§16.4)",
    ),
    (
        "prerelease",
        "release trains (`next`/`rc` channels) land with M5.6",
    ),
    ("prerelease-type", "release trains land with M5.6"),
    (
        "pull-request-footer",
        "the §11.3 body renderer owns the Release-PR shape",
    ),
    (
        "pull-request-header",
        "the §11.3 body renderer owns the Release-PR shape",
    ),
    (
        "pull-request-title-pattern",
        "the §11.3 body renderer owns the Release-PR shape",
    ),
    (
        "release-as",
        "halyard's planner can pin a release-as bump (E-PLAN-RELEASE-AS) — set the halyard override deliberately",
    ),
    (
        "release-type",
        "the release-please language strategy — halyard's discovery derives the ecosystem per package (§6)",
    ),
    (
        "separate-pull-requests",
        "halyard opens one Release PR per plan (§11.3)",
    ),
    ("skip-changelog", "halyard renders changelogs per §9.4"),
    (
        "skip-github-release",
        "suppression is per-package `release = false` (§16.4)",
    ),
    ("skip-snapshot", "java snapshot flows are not a v1 surface"),
    ("snapshot-label", "java snapshot flows are not a v1 surface"),
    (
        "version-file",
        "version rewriting beyond manifests and lockfiles is not a v1 surface (§9)",
    ),
    (
        "versioning",
        "release-please's own strategy enum — halyard's §5.4 policies are expressed differently — review",
    ),
];

/// The per-package tag inputs (release-please's documented defaults:
/// `include-component-in-tag` defaults to `true`, `include-v-in-tag`
/// defaults to `true`; `tag-separator` has no documented default — the
/// documented tag shape `<component-name>-v<release-version>` fixes the
/// observed `-`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RpTagInputs {
    include_component: bool,
    include_v: bool,
}

fn rp_tag_template(inputs: RpTagInputs, separator: &str) -> Option<String> {
    let template = match (inputs.include_component, inputs.include_v) {
        (true, true) => format!("{{name}}{separator}v{{version}}"),
        (true, false) => format!("{{name}}{separator}{{version}}"),
        (false, true) => "v{version}".to_owned(),
        (false, false) => "{version}".to_owned(),
    };
    TagTemplate::parse(&template).is_ok().then_some(template)
}

/// Reads a boolean foreign key, reporting a type mismatch fail-closed.
fn rp_bool(
    table: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    path: &str,
) -> Result<Option<bool>, ConfigError> {
    match table.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Bool(value)) => Ok(Some(*value)),
        Some(other) => Err(ConfigError::Migrate {
            path: path.to_owned(),
            message: format!("`{key}` must be a boolean, got {other}"),
        }),
    }
}

fn rp_str(
    table: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    path: &str,
) -> Result<Option<String>, ConfigError> {
    match table.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(value)) => Ok(Some(value.clone())),
        Some(other) => Err(ConfigError::Migrate {
            path: path.to_owned(),
            message: format!("`{key}` must be a string, got {other}"),
        }),
    }
}

fn unmapped_row(foreign_key: &str, reason: &str) -> UnmappedRow {
    UnmappedRow {
        foreign_key: foreign_key.to_owned(),
        reason: reason.to_owned(),
    }
}

fn unmapped_from(foreign_key: &str, lookup: &str, table: &[(&str, &str)]) -> UnmappedRow {
    let reason = table
        .iter()
        .find(|(known, _)| *known == lookup)
        .map_or_else(
            || {
                "documented release-please option with no v1 equivalent \
                 (see docs/research/migrate-config-grammars.md)"
                    .to_owned()
            },
            |(_, reason)| (*reason).to_owned(),
        );
    unmapped_row(foreign_key, &reason)
}

/// The `halyard.toml` rendering of one bump level.
fn bump_name(level: BumpLevel) -> &'static str {
    match level {
        BumpLevel::None => "none",
        BumpLevel::Patch => "patch",
        BumpLevel::Minor => "minor",
        BumpLevel::Major => "major",
    }
}

/// The pre-major pin rows (release-please's documented defaults differ
/// from halyard's §10.2 — the notes say which side is the foreign
/// default) plus the pinned levels the draft needs.
fn rp_pre_major_rows(
    table: &serde_json::Map<String, serde_json::Value>,
) -> Result<(Vec<MappingRow>, BumpLevel, BumpLevel), ConfigError> {
    let mut rows: Vec<MappingRow> = Vec::new();
    let bump_minor = rp_bool(table, "bump-minor-pre-major", RP_CONFIG)?;
    let (breaking_level, breaking_key, breaking_note): (BumpLevel, &str, &str) = match bump_minor {
        Some(true) => (
            BumpLevel::Minor,
            "bump-minor-pre-major",
            "matches halyard's built-in §10.2 default; pinned explicitly",
        ),
        Some(false) => (
            BumpLevel::Major,
            "bump-minor-pre-major",
            "release-please's default (false) bumps pre-1.0 breaking changes to a new major — \
             halyard's built-in default is `minor`, so this pin preserves the release-please \
             behavior",
        ),
        None => (
            BumpLevel::Major,
            "bump-minor-pre-major (default)",
            "release-please's documented default (false) — halyard's built-in default is \
             `minor`, so this pin preserves the release-please behavior",
        ),
    };
    rows.push(MappingRow {
        foreign_key: breaking_key.to_owned(),
        halyard_key: "versioning.pre_major.breaking".to_owned(),
        value: bump_name(breaking_level).to_owned(),
        note: Some(breaking_note.to_owned()),
    });
    let bump_patch = rp_bool(table, "bump-patch-for-minor-pre-major", RP_CONFIG)?;
    let (feat_level, feat_key, feat_note): (BumpLevel, &str, &str) = match bump_patch {
        Some(true) => (
            BumpLevel::Patch,
            "bump-patch-for-minor-pre-major",
            "matches halyard's built-in §10.2 default; pinned explicitly",
        ),
        Some(false) => (
            BumpLevel::Minor,
            "bump-patch-for-minor-pre-major",
            "release-please's default (false) leaves pre-1.0 feat bumps at minor — halyard's \
             built-in default is `patch`, so this pin preserves the release-please behavior",
        ),
        None => (
            BumpLevel::Minor,
            "bump-patch-for-minor-pre-major (default)",
            "release-please's documented default (false) — halyard's built-in default is \
             `patch`, so this pin preserves the release-please behavior",
        ),
    };
    rows.push(MappingRow {
        foreign_key: feat_key.to_owned(),
        halyard_key: "versioning.pre_major.feat".to_owned(),
        value: bump_name(feat_level).to_owned(),
        note: Some(feat_note.to_owned()),
    });
    Ok((rows, breaking_level, feat_level))
}

/// The tag-switch survey's outputs: the mapped rows, the unmapped
/// rows, the unified tag template (when one represents every package)
/// and the per-package resolved templates keyed by package path.
type RpTagSurvey<'a> = (
    Vec<MappingRow>,
    Vec<UnmappedRow>,
    Option<String>,
    BTreeMap<&'a String, String>,
);

/// The tag-switch survey: the top-level defaults plus the per-package
/// overrides resolved into appendix-A.3 templates. A unified template
/// becomes the `repo.tag_style` row (returned for the draft); a
/// per-package disagreement is an unmapped row — never guessed.
fn rp_tag_survey<'a>(
    table: &serde_json::Map<String, serde_json::Value>,
    packages: &'a serde_json::Map<String, serde_json::Value>,
) -> Result<RpTagSurvey<'a>, ConfigError> {
    let mut rows: Vec<MappingRow> = Vec::new();
    let mut unmapped: Vec<UnmappedRow> = Vec::new();
    // The tag inputs: top-level defaults, overridable per package.
    let top_component = rp_bool(table, "include-component-in-tag", RP_CONFIG)?;
    let top_v = rp_bool(table, "include-v-in-tag", RP_CONFIG)?;
    let top_separator = rp_str(table, "tag-separator", RP_CONFIG)?;
    let default_inputs = RpTagInputs {
        include_component: top_component.unwrap_or(true),
        include_v: top_v.unwrap_or(true),
    };
    let default_separator = top_separator.clone().unwrap_or_else(|| "-".to_owned());
    let default_template = rp_tag_template(default_inputs, &default_separator);

    // Per-package tag resolution; a disagreement cannot be a
    // repo-wide `repo.tag_style`, so it is reported, never guessed.
    let mut per_package_tags: BTreeMap<&String, String> = BTreeMap::new();
    for (path, opts) in packages {
        let opts = opts.as_object().ok_or_else(|| ConfigError::Migrate {
            path: RP_CONFIG.to_owned(),
            message: format!("`packages[{path}]` must be an object"),
        })?;
        let component = rp_bool(opts, "include-component-in-tag", RP_CONFIG)?;
        let include_v = rp_bool(opts, "include-v-in-tag", RP_CONFIG)?;
        let separator = rp_str(opts, "tag-separator", RP_CONFIG)?;
        let inputs = RpTagInputs {
            include_component: component.unwrap_or(default_inputs.include_component),
            include_v: include_v.unwrap_or(default_inputs.include_v),
        };
        let separator = separator.unwrap_or_else(|| default_separator.clone());
        match rp_tag_template(inputs, &separator) {
            Some(template) => {
                per_package_tags.insert(path, template);
            }
            None => {
                unmapped.push(unmapped_row(
                    &format!("packages[{path}].tag-separator"),
                    "the tag-separator value does not fit the appendix-A.3 template grammar",
                ));
            }
        }
    }
    let unified: Option<String> = {
        let mut iter = per_package_tags.values();
        let first = iter.next().cloned();
        if iter.all(|t| Some(t) == first.as_ref()) {
            first
        } else {
            None
        }
    };
    if let Some(template) = &unified {
        let note = if default_template.as_ref() == Some(template) {
            "release-please's documented defaults \
             (`include-component-in-tag: true`, `include-v-in-tag: true`, separator `-`) \
             — halyard's own multi-package default template"
        } else {
            "resolved from the release-please tag switches; validated against the \
             appendix-A.3 template grammar"
        };
        rows.push(MappingRow {
            foreign_key: "include-component-in-tag / include-v-in-tag / tag-separator".to_owned(),
            halyard_key: "repo.tag_style".to_owned(),
            value: template.clone(),
            note: Some(note.to_owned()),
        });
    } else {
        let shapes: Vec<String> = per_package_tags
            .iter()
            .map(|(path, template)| format!("{path}: {template}"))
            .collect();
        unmapped.push(unmapped_row(
            "include-component-in-tag (per-package disagreement)",
            &format!(
                "the packages disagree about the tag shape ({}); halyard v1 carries one \
                 repo-wide `repo.tag_style` — pick one deliberately after review",
                shapes.join(", ")
            ),
        ));
    }
    Ok((rows, unmapped, unified, per_package_tags))
}

/// The per-package survey rows (the foreign tool's own view) and their
/// unmapped keys; the manifest's last-release versions ride the notes.
fn rp_package_survey(
    packages: &serde_json::Map<String, serde_json::Value>,
    manifest: &BTreeMap<String, String>,
    per_package_tags: &BTreeMap<&String, String>,
) -> Result<(Vec<PackageSurvey>, Vec<UnmappedRow>), ConfigError> {
    let mut unmapped: Vec<UnmappedRow> = Vec::new();
    let mut survey_packages: Vec<PackageSurvey> = Vec::new();
    for (path, opts) in packages {
        let opts = opts.as_object().ok_or_else(|| ConfigError::Migrate {
            path: RP_CONFIG.to_owned(),
            message: format!("`packages[{path}]` must be an object"),
        })?;
        let component = match rp_str(opts, "package-name", RP_CONFIG)? {
            Some(name) => Some(name),
            None => rp_str(opts, "component", RP_CONFIG)?,
        };
        let mut notes = Vec::new();
        if let Some(version) = manifest.get(path) {
            notes.push(format!("release-please last release: {version}"));
        }
        survey_packages.push(PackageSurvey {
            path: path.clone(),
            component,
            tag: per_package_tags.get(path).cloned().unwrap_or_default(),
            release: true,
            notes,
        });
        // Per-package unmapped keys.
        for key in opts.keys() {
            match key.as_str() {
                "package-name"
                | "component"
                | "include-component-in-tag"
                | "include-v-in-tag"
                | "tag-separator" => {}
                _ => unmapped.push(unmapped_from(
                    &format!("packages[{path}].{key}"),
                    key,
                    RP_UNMAPPED_PACKAGE,
                )),
            }
        }
    }
    Ok((survey_packages, unmapped))
}

impl MigrationSource for ReleasePleaseSource {
    fn tool(&self) -> &'static str {
        "release-please"
    }

    fn config_paths(&self) -> &'static [&'static str] {
        &[RP_CONFIG, RP_MANIFEST]
    }

    fn survey(&self, files: &ForeignFiles) -> Result<ToolSurvey, ConfigError> {
        let mut found: Vec<String> = self
            .config_paths()
            .iter()
            .filter(|p| files.get(p).is_some())
            .map(|p| (*p).to_owned())
            .collect();
        found.sort();
        let Some(config_raw) = files.get(RP_CONFIG) else {
            return Ok(ToolSurvey {
                tool: self.tool().to_owned(),
                configured: false,
                ..ToolSurvey::default()
            });
        };
        let config: serde_json::Value =
            serde_json::from_str(config_raw).map_err(|error| ConfigError::Migrate {
                path: RP_CONFIG.to_owned(),
                message: format!("invalid JSON: {error}"),
            })?;
        let table = config.as_object().ok_or_else(|| ConfigError::Migrate {
            path: RP_CONFIG.to_owned(),
            message: "the release-please config must be a JSON object".to_owned(),
        })?;
        let packages = table.get("packages").ok_or_else(|| ConfigError::Migrate {
            path: RP_CONFIG.to_owned(),
            message: "the `packages` map is required (the release-please config schema marks it \
                      required, `additionalProperties: false` at the root)"
                .to_owned(),
        })?;
        let packages = packages.as_object().ok_or_else(|| ConfigError::Migrate {
            path: RP_CONFIG.to_owned(),
            message: "`packages` must be an object of package-path → options".to_owned(),
        })?;

        let (mut rows, breaking, feat) = rp_pre_major_rows(table)?;
        let (mut tag_rows, mut unmapped, unified, per_package_tags) =
            rp_tag_survey(table, packages)?;
        rows.append(&mut tag_rows);

        // Packages: the survey rows (the foreign tool's own view).
        let manifest = parse_manifest(files)?;
        let (mut survey_packages, mut package_unmapped) =
            rp_package_survey(packages, &manifest, &per_package_tags)?;
        unmapped.append(&mut package_unmapped);
        // Manifest paths outside `packages` are stale state the report
        // surfaces instead of dropping.
        let outside: Vec<String> = manifest
            .keys()
            .filter(|p| !packages.contains_key(*p))
            .cloned()
            .collect();
        if !outside.is_empty() {
            unmapped.push(unmapped_row(
                ".release-please-manifest.json (outside `packages`)",
                &format!(
                    "the manifest tracks {} — release-please no longer releases them; halyard \
                     releases what discovery finds unless `[[package]] release = false` (§16.4)",
                    outside.join(", ")
                ),
            ));
        }
        unmapped.push(unmapped_row(
            "packages (completeness)",
            "release-please releases only the listed `packages`; halyard discovers every package \
             and releases unless `[[package]] release = false` (§16.4) — set the overrides for \
             members that must not release",
        ));

        // Root keys that are not mapped.
        for key in table.keys() {
            match key.as_str() {
                "packages"
                | "bump-minor-pre-major"
                | "bump-patch-for-minor-pre-major"
                | "include-component-in-tag"
                | "include-v-in-tag"
                | "tag-separator" => {}
                _ => unmapped.push(unmapped_from(key, key, RP_UNMAPPED_ROOT)),
            }
        }

        rows.sort_by(|a, b| a.foreign_key.cmp(&b.foreign_key));
        unmapped.sort_by(|a, b| a.foreign_key.cmp(&b.foreign_key));
        survey_packages.sort_by(|a, b| a.path.cmp(&b.path));

        let mut config = HalyardConfig::default();
        if let Some(template) = unified {
            config.repo = Some(RepoCfg {
                base_branch: None,
                tag_style: Some(TagStyle(template)),
            });
        }
        config.versioning = Some(crate::model::VersioningCfg {
            policy: None,
            pre_major: Some(PreMajorCfg {
                breaking: Some(breaking),
                feat: Some(feat),
            }),
            cascade: None,
        });

        Ok(ToolSurvey {
            tool: self.tool().to_owned(),
            configured: true,
            files: found,
            rows,
            unmapped,
            packages: survey_packages,
            draft: Some(render_draft(&config, RP_CONFIG)?),
        })
    }
}

/// Parses the optional release-please manifest
/// (`.release-please-manifest.json`): an object of package path → last
/// released version. Absent is a value; a malformed manifest refuses
/// fail-closed.
fn parse_manifest(files: &ForeignFiles) -> Result<BTreeMap<String, String>, ConfigError> {
    let Some(raw) = files.get(RP_MANIFEST) else {
        return Ok(BTreeMap::new());
    };
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|error| ConfigError::Migrate {
            path: RP_MANIFEST.to_owned(),
            message: format!("invalid JSON: {error}"),
        })?;
    let table = value.as_object().ok_or_else(|| ConfigError::Migrate {
        path: RP_MANIFEST.to_owned(),
        message: "the manifest must be a JSON object of package path → version".to_owned(),
    })?;
    let mut out = BTreeMap::new();
    for (path, version) in table {
        let Some(version) = version.as_str() else {
            return Err(ConfigError::Migrate {
                path: RP_MANIFEST.to_owned(),
                message: format!("`{path}` must carry a version string"),
            });
        };
        out.insert(path.clone(), version.to_owned());
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// changesets
// ---------------------------------------------------------------------------

/// The changesets config (`.changeset/config.json` — VERIFIED against
/// the changesets config documentation; the git-tag grammar is *not*
/// verified and stays an honest advisory).
#[derive(Debug)]
pub struct ChangesetsSource;

const CS_CONFIG: &str = ".changeset/config.json";

/// Foreign keys that carry no `halyard.toml` equivalent, each with the
/// reason the report shows.
const CS_UNMAPPED: &[(&str, &str)] = &[
    (
        "bumpVersionsWithWorkspaceProtocolOnly",
        "workspace-protocol handling is halyard's rewriter behavior (appendix A.2)",
    ),
    (
        "changedFilePatterns",
        "halyard attributes changes via `[[package]].include`/`exclude` globs (§8.4)",
    ),
    (
        "changelog",
        "changesets' changelog generator writes entries its own way; halyard renders the §9.4 \
         styles — pick `changelog.style`",
    ),
    (
        "commit",
        "changesets' per-release commit strategy; halyard derives bumps from the git history \
         itself (§10.1)",
    ),
    (
        "format",
        "changelog formatting (`auto`/`prettier`/…) is not a v1 key",
    ),
    (
        "fixed",
        "changesets fixed groups are package subsets; halyard's `versioning.policy = \"fixed\"` \
         is workspace-wide (§5.4) — review the groups before pinning",
    ),
    (
        "linked",
        "changesets linked groups are package subsets; halyard's `versioning.policy = \"linked\"` \
         is workspace-wide (§5.4) — review the groups before pinning",
    ),
    (
        "privatePackages",
        "changesets skips private packages by default; halyard releases what discovery finds — \
         set `[[package]] release = false` (§16.4)",
    ),
    (
        "snapshot",
        "changesets snapshot (pre) releases land with halyard's M5.6 release trains",
    ),
    (
        "___experimentalUnsafeOptions_WILL_CHANGE_IN_PATCH",
        "changesets experimental knobs with no v1 equivalent",
    ),
];

/// The `access` row's inputs (changesets default `restricted`): the
/// pinned value and its note. Absent is a value, not an error.
fn cs_access(
    table: &serde_json::Map<String, serde_json::Value>,
) -> Result<Option<(String, String)>, ConfigError> {
    Ok(match table.get("access") {
        None | Some(serde_json::Value::Null) => Some((
            "restricted".to_owned(),
            "changesets' documented default; npm restricted access applies to scoped packages \
             — unset or set `public` deliberately (publishing stays opt-in: the draft never \
             sets `publish.enabled`)"
                .to_owned(),
        )),
        Some(serde_json::Value::String(value)) if value == "public" || value == "restricted" => {
            Some((
                value.clone(),
                "publishing stays opt-in: the draft never sets `publish.enabled`".to_owned(),
            ))
        }
        Some(serde_json::Value::String(other)) => {
            return Err(ConfigError::Migrate {
                path: CS_CONFIG.to_owned(),
                message: format!("`access` must be `public` or `restricted`, got {other:?}"),
            });
        }
        Some(other) => {
            return Err(ConfigError::Migrate {
                path: CS_CONFIG.to_owned(),
                message: format!("`access` must be a string, got {other}"),
            });
        }
    })
}

/// The `baseBranch` row's inputs (changesets default `main`).
fn cs_base_branch(
    table: &serde_json::Map<String, serde_json::Value>,
) -> Result<Option<(String, String)>, ConfigError> {
    Ok(match table.get("baseBranch") {
        None | Some(serde_json::Value::Null) => Some((
            "main".to_owned(),
            "changesets' documented default — halyard's default is the remote HEAD, so this \
             pin preserves the changesets behavior"
                .to_owned(),
        )),
        Some(serde_json::Value::String(value)) => {
            Some((value.clone(), "changesets' comparison branch".to_owned()))
        }
        Some(other) => {
            return Err(ConfigError::Migrate {
                path: CS_CONFIG.to_owned(),
                message: format!("`baseBranch` must be a string, got {other}"),
            });
        }
    })
}

/// The cascade row: changesets always moves dependents when a package
/// releases (`updateInternalDependencies` picks the rewritten range's
/// level) — halyard's closest v1 shape is `cascade = "always"`.
fn cs_cascade_row(
    table: &serde_json::Map<String, serde_json::Value>,
) -> Result<MappingRow, ConfigError> {
    let update_level = match table.get("updateInternalDependencies") {
        None | Some(serde_json::Value::Null) => "patch".to_owned(),
        Some(serde_json::Value::String(value)) => value.clone(),
        Some(other) => {
            return Err(ConfigError::Migrate {
                path: CS_CONFIG.to_owned(),
                message: format!("`updateInternalDependencies` must be a string, got {other}"),
            });
        }
    };
    Ok(MappingRow {
        foreign_key: "updateInternalDependencies (default patch)".to_owned(),
        halyard_key: "versioning.cascade".to_owned(),
        value: "always".to_owned(),
        note: Some(format!(
            "changesets moves dependents on every release (the key picks the rewritten \
             range's level: `{update_level}`); halyard's cascade bumps dependents (§10.4) and \
             its rewriter preserves operator+precision (appendix A.2) — review the shape"
        )),
    })
}

/// The `ignore` list (sorted; the one changesets key the v1 surface
/// expresses exactly — per-package `release = false` overrides).
fn cs_ignored(
    table: &serde_json::Map<String, serde_json::Value>,
) -> Result<Vec<String>, ConfigError> {
    Ok(match table.get("ignore") {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(serde_json::Value::Array(values)) => {
            let mut names = Vec::with_capacity(values.len());
            for value in values {
                let Some(name) = value.as_str() else {
                    return Err(ConfigError::Migrate {
                        path: CS_CONFIG.to_owned(),
                        message: "`ignore` must be an array of package names".to_owned(),
                    });
                };
                names.push(name.to_owned());
            }
            names.sort();
            names
        }
        Some(other) => {
            return Err(ConfigError::Migrate {
                path: CS_CONFIG.to_owned(),
                message: format!("`ignore` must be an array, got {other}"),
            });
        }
    })
}

/// Every documented-but-unmapped key reports its semantic difference,
/// plus the honest advisory about the unverified changesets tag
/// grammar (see the research note).
fn cs_unmapped_rows(table: &serde_json::Map<String, serde_json::Value>) -> Vec<UnmappedRow> {
    let mut unmapped: Vec<UnmappedRow> = Vec::new();
    for key in table.keys() {
        match key.as_str() {
            "access" | "baseBranch" | "updateInternalDependencies" | "ignore" => {}
            _ => {
                let reason = CS_UNMAPPED
                    .iter()
                    .find(|(known, _)| *known == key)
                    .map_or_else(
                        || {
                            format!(
                                "changesets option with no v1 equivalent — see \
                                 docs/research/migrate-config-grammars.md ({key})"
                            )
                        },
                        |(_, reason)| (*reason).to_owned(),
                    );
                // The linked/fixed reasons gain the concrete groups.
                if (key == "linked" || key == "fixed")
                    && let Some(groups) = table.get(key).and_then(|v| v.as_array())
                {
                    let rendered: Vec<String> = groups
                        .iter()
                        .map(|group| {
                            let names: Vec<&str> = group
                                .as_array()
                                .map(|members| {
                                    members.iter().filter_map(|name| name.as_str()).collect()
                                })
                                .unwrap_or_default();
                            names.join(", ")
                        })
                        .collect();
                    unmapped.push(unmapped_row(
                        key,
                        &format!("{reason} — configured groups: [{}]", rendered.join(" | ")),
                    ));
                } else {
                    unmapped.push(unmapped_row(key, &reason));
                }
            }
        }
    }
    unmapped.push(unmapped_row(
        "(changesets tags)",
        "changesets' cross-repo git-tag grammar was not verified against its documentation \
         in this slice — set `repo.tag_style` to match this repository's existing tags \
         (appendix A.3)",
    ));
    unmapped
}

impl MigrationSource for ChangesetsSource {
    fn tool(&self) -> &'static str {
        "changesets"
    }

    fn config_paths(&self) -> &'static [&'static str] {
        &[CS_CONFIG]
    }

    fn survey(&self, files: &ForeignFiles) -> Result<ToolSurvey, ConfigError> {
        let mut found: Vec<String> = self
            .config_paths()
            .iter()
            .filter(|p| files.get(p).is_some())
            .map(|p| (*p).to_owned())
            .collect();
        found.sort();
        let Some(raw) = files.get(CS_CONFIG) else {
            return Ok(ToolSurvey {
                tool: self.tool().to_owned(),
                configured: false,
                ..ToolSurvey::default()
            });
        };
        let value: serde_json::Value =
            serde_json::from_str(raw).map_err(|error| ConfigError::Migrate {
                path: CS_CONFIG.to_owned(),
                message: format!("invalid JSON: {error}"),
            })?;
        let table = value.as_object().ok_or_else(|| ConfigError::Migrate {
            path: CS_CONFIG.to_owned(),
            message: "the changesets config must be a JSON object".to_owned(),
        })?;

        let mut rows: Vec<MappingRow> = Vec::new();
        let mut unmapped: Vec<UnmappedRow> = Vec::new();

        // `access` (changesets default `restricted`) → publish.npm.access.
        if let Some((value, note)) = cs_access(table)? {
            rows.push(MappingRow {
                foreign_key: "access".to_owned(),
                halyard_key: "publish.npm.access".to_owned(),
                value,
                note: Some(note),
            });
        }

        // `baseBranch` (changesets default `main`) → repo.base_branch.
        if let Some((value, note)) = cs_base_branch(table)? {
            rows.push(MappingRow {
                foreign_key: "baseBranch".to_owned(),
                halyard_key: "repo.base_branch".to_owned(),
                value,
                note: Some(note),
            });
        }

        rows.push(cs_cascade_row(table)?);

        // `ignore` → per-package `release = false` overrides (the one
        // changesets key the v1 surface expresses exactly).
        let ignored = cs_ignored(table)?;
        if !ignored.is_empty() {
            rows.push(MappingRow {
                foreign_key: "ignore".to_owned(),
                halyard_key: "[[package]].release".to_owned(),
                value: format!("false for: {}", ignored.join(", ")),
                note: Some(
                    "changesets' excluded packages become release = false overrides (§16.4)"
                        .to_owned(),
                ),
            });
        }

        unmapped.extend(cs_unmapped_rows(table));

        rows.sort_by(|a, b| a.foreign_key.cmp(&b.foreign_key));
        unmapped.sort_by(|a, b| a.foreign_key.cmp(&b.foreign_key));

        // The draft: the pinned defaults + the mapped values. Publishing
        // stays opt-in (`publish.enabled` is never set by a migration).
        let mut config = HalyardConfig::default();
        if let Some((value, _)) = cs_base_branch(table)? {
            config.repo = Some(RepoCfg {
                base_branch: Some(value),
                tag_style: None,
            });
        }
        if let Some((value, _)) = cs_access(table)? {
            let parsed_access = match value.as_str() {
                "public" => NpmAccess::Public,
                _ => NpmAccess::Restricted,
            };
            config.publish = Some(crate::model::PublishCfg {
                enabled: None,
                cargo: None,
                npm: Some(crate::model::NpmPublishCfg {
                    access: Some(parsed_access),
                    provenance: None,
                }),
            });
        }
        for name in &ignored {
            config.package.push(PackageOverride {
                name: Some(name.clone()),
                release: Some(false),
                include: Vec::new(),
                exclude: Vec::new(),
            });
        }

        Ok(ToolSurvey {
            tool: self.tool().to_owned(),
            configured: true,
            files: found,
            rows,
            unmapped,
            packages: Vec::new(),
            draft: Some(render_draft(&config, CS_CONFIG)?),
        })
    }
}

// ---------------------------------------------------------------------------
// Draft rendering
// ---------------------------------------------------------------------------

/// Serializes the draft `HalyardConfig` and validates it through the
/// strict loader before it is surfaced (the draft is a report string —
/// the §16.2 write step is a later slice's job, and the validation is
/// the proof the draft is exactly what `halyard.toml` v1 accepts).
fn render_draft(config: &HalyardConfig, path: &str) -> Result<String, ConfigError> {
    if *config == HalyardConfig::default() {
        return Ok(String::new());
    }
    let rendered = toml::to_string_pretty(config).map_err(|error| ConfigError::Migrate {
        path: path.to_owned(),
        message: format!("the draft config does not serialize: {error}"),
    })?;
    load_from_str(&rendered).map_err(|error| ConfigError::Migrate {
        path: path.to_owned(),
        message: format!("the draft config is not a valid halyard.toml v1: {error}"),
    })?;
    Ok(rendered)
}
