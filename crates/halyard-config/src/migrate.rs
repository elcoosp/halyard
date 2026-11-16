//! Foreign release-tool migration (M4.7, §16.2 `migrate`): read a
//! foreign release tool's configuration, map what the `halyard.toml` v1
//! surface can express, and report the semantic differences — the
//! report ([`MigrationReport`]) is plain data, the command renders it.
//! Step 1 surveys and validates a draft `halyard.toml` (it rides the
//! report as a string, accepted by the strict loader before it is
//! surfaced); step 2 writes it: the pure planner ([`plan_draft_write`])
//! picks the draft from the report and the runner ([`run_draft_write`])
//! lands it through the injected [`DraftSink`] — atomic, create-only,
//! fail-closed on every guess (an existing target is never clobbered;
//! two configured tools are a refusal, never a pick).
//!
//! The seam ([`MigrationSource`]) is the extension point the migrate
//! slices fill — all four §16.2 tools are registered (ADR-0022 +
//! ADR-0024): release-please, changesets, knope and release-plz, one
//! source per tool, each declaring the files it reads and mapping them
//! onto the frozen v1 model. [`default_sources`] is the registry the
//! shell surveys; [`migrate`] runs any slice of sources over the
//! discovered files, so a downstream caller can migrate one tool in
//! isolation.
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
//! release-please manifest-releaser documentation and config schema,
//! the changesets config documentation, knope's configuration pages
//! (config, packages, Release step) and release-plz's configuration
//! reference — the quotes and the open VERIFY items live in
//! `docs/research/migrate-config-grammars.md` (notably: the changesets
//! git-tag grammar is *not* verified; the report says so instead of
//! guessing).

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;

use serde::{Deserialize, Serialize};

use crate::error::ConfigError;
use crate::loader::load_from_str;
use crate::model::{
    BumpLevel, ChangelogCfg, HalyardConfig, NpmAccess, PackageOverride, PreMajorCfg, RepoCfg,
    TagStyle,
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
/// release-please and changesets; the §16.2 extension adds knope and
/// release-plz, the row's remaining two tools).
pub fn default_sources() -> Vec<Box<dyn MigrationSource>> {
    vec![
        Box::new(ReleasePleaseSource),
        Box::new(ChangesetsSource),
        Box::new(KnopeSource),
        Box::new(ReleasePlzSource),
    ]
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
    /// through the strict loader before it is surfaced. The §16.2 write
    /// step ([`plan_draft_write`] + [`run_draft_write`]) carries this
    /// string verbatim to [`DRAFT_TARGET`]; nothing else ever writes it.
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

/// Looks a foreign key up in a documented-reasons table; unknown keys
/// get the source's own fallback sentence (so the report always names
/// where the grammar came from, never a bare "unmapped").
fn unmapped_from_table(
    foreign_key: &str,
    lookup: &str,
    table: &[(&str, &str)],
    fallback: &str,
) -> UnmappedRow {
    let reason = table
        .iter()
        .find(|(known, _)| *known == lookup)
        .map_or_else(|| fallback.to_owned(), |(_, reason)| (*reason).to_owned());
    unmapped_row(foreign_key, &reason)
}

fn unmapped_from(foreign_key: &str, lookup: &str, table: &[(&str, &str)]) -> UnmappedRow {
    unmapped_from_table(
        foreign_key,
        lookup,
        table,
        "documented release-please option with no v1 equivalent \
         (see docs/research/migrate-config-grammars.md)",
    )
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

        let config = survey_config(unified, breaking, feat);

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

/// The validated draft `halyard.toml` skeleton every release-please
/// survey builds: the translated tag template on `[repo]` and the
/// foreign pre-major mapping on `[versioning]`. A migration never sets
/// a channel (ADR-0022: the draft carries only what the foreign config
/// actually maps).
fn survey_config(unified: Option<String>, breaking: BumpLevel, feat: BumpLevel) -> HalyardConfig {
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
        channel: None,
        channel_branches: None,
    });
    config
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
// knope (the §16.2 extension)
// ---------------------------------------------------------------------------

/// The knope config (`knope.toml` — VERIFIED against the tool's own
/// configuration documentation: the config page's `[package]` /
/// `[packages."<name>"]` shapes and the Release step page's tag rules;
/// quotes in `docs/research/migrate-config-grammars.md`).
#[derive(Debug)]
pub struct KnopeSource;

const KN_CONFIG: &str = "knope.toml";

/// Per-package keys that carry no v1 equivalent, each with the reason
/// the report shows.
const KN_UNMAPPED_PACKAGE: &[(&str, &str)] = &[
    (
        "versioned_files",
        "halyard rewrites manifests and lockfiles per the discovered ecosystem (§9); extra \
         versioned files are not a v1 surface",
    ),
    (
        "changelog",
        "the per-package changelog path has no v1 equivalent — halyard renders `CHANGELOG.md` \
         per package (§9.4)",
    ),
    (
        "scopes",
        "a conventional-commit scope allow-list is not a v1 key — commit types map through \
         `commits.types` (§16.4) and the §9.4 styles",
    ),
    (
        "extra_changelog_sections",
        "halyard renders the §9.4 styles; extra changelog sections are not a v1 key",
    ),
    (
        "assets",
        "release assets upload is a forge-side flow (§11.1); the asset list is not a v1 key",
    ),
];

/// Root keys that carry no v1 equivalent.
const KN_UNMAPPED_ROOT: &[(&str, &str)] = &[
    (
        "github",
        "knope's GitHub token/owner/repo config is the forge seam's concern (§11.4); halyard \
         resolves credentials itself",
    ),
    ("jira", "Jira issue transitions are not a v1 surface"),
    (
        "variables",
        "knope's workflow variables carry no v1 equivalent",
    ),
];

const KN_FALLBACK: &str =
    "documented knope option with no v1 equivalent (see docs/research/migrate-config-grammars.md)";

/// Reads a TOML string key, reporting a type mismatch fail-closed.
fn kn_str(table: &toml::Table, key: &str, path: &str) -> Result<Option<String>, ConfigError> {
    match table.get(key) {
        None | Some(toml::Value::String(_)) => Ok(table
            .get(key)
            .and_then(toml::Value::as_str)
            .map(str::to_owned)),
        Some(other) => Err(ConfigError::Migrate {
            path: path.to_owned(),
            message: format!("`{key}` must be a string, got {other}"),
        }),
    }
}

/// Reads a TOML array-of-strings key, reporting a type mismatch
/// fail-closed.
fn kn_str_array(
    table: &toml::Table,
    key: &str,
    path: &str,
) -> Result<Option<Vec<String>>, ConfigError> {
    match table.get(key) {
        None => Ok(None),
        Some(toml::Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    toml::Value::String(value) => out.push(value.clone()),
                    other => {
                        return Err(ConfigError::Migrate {
                            path: path.to_owned(),
                            message: format!("`{key}` must be an array of strings, got {other}"),
                        });
                    }
                }
            }
            Ok(Some(out))
        }
        Some(other) => Err(ConfigError::Migrate {
            path: path.to_owned(),
            message: format!("`{key}` must be an array of strings, got {other}"),
        }),
    }
}

/// The per-package survey row's provenance note (the foreign list of
/// versioned files).
fn kn_versioned_note(table: &toml::Table, path: &str) -> Result<Option<String>, ConfigError> {
    Ok(kn_str_array(table, "versioned_files", path)?
        .map(|files| format!("knope versioned_files: {}", files.join(", "))))
}

/// One knope package's survey row: the name is the component, the
/// documented tag rule supplies the template.
fn kn_package_survey(
    name: &str,
    table: &toml::Table,
    tag: &str,
    path: &str,
) -> Result<PackageSurvey, ConfigError> {
    Ok(PackageSurvey {
        path: name.to_owned(),
        component: Some(name.to_owned()),
        tag: tag.to_owned(),
        release: true,
        notes: kn_versioned_note(table, path)?.into_iter().collect(),
    })
}

/// Surveys the single `[package]` shape: the Release step's documented
/// tag rule becomes one appendix-A.3 template, the section's keys are
/// reported per the documented-reasons table, and the repository-root
/// survey row carries the versioned files as provenance.
fn kn_survey_single_package(
    pkg: &toml::Table,
    rows: &mut Vec<MappingRow>,
    unmapped: &mut Vec<UnmappedRow>,
    packages: &mut Vec<PackageSurvey>,
) -> Result<String, ConfigError> {
    let tag = "v{version}".to_owned();
    rows.push(MappingRow {
        foreign_key: "package (Release step tag rule)".to_owned(),
        halyard_key: "repo.tag_style".to_owned(),
        value: tag.clone(),
        note: Some(
            "knope's documented single-package tag shape (\"this tag will be v{version}\"); \
             validated against the appendix-A.3 template grammar"
                .to_owned(),
        ),
    });
    packages.push(PackageSurvey {
        // knope's single `[package]` section covers the repository
        // root; the versioned files carry the real paths as provenance
        // notes.
        path: ".".to_owned(),
        component: None,
        tag: tag.clone(),
        release: true,
        notes: kn_versioned_note(pkg, KN_CONFIG)?.into_iter().collect(),
    });
    for key in pkg.keys() {
        unmapped.push(unmapped_from_table(
            key,
            key,
            KN_UNMAPPED_PACKAGE,
            KN_FALLBACK,
        ));
    }
    Ok(tag)
}

/// Surveys the `[packages."<name>"]` shape: the Release step's
/// documented multi-package rule is uniform, so every package's tag is
/// one template.
fn kn_survey_multi_packages(
    pkgs: &toml::Table,
    rows: &mut Vec<MappingRow>,
    unmapped: &mut Vec<UnmappedRow>,
    packages: &mut Vec<PackageSurvey>,
) -> Result<String, ConfigError> {
    let tag = "{name}/v{version}".to_owned();
    rows.push(MappingRow {
        foreign_key: "packages (Release step tag rule)".to_owned(),
        halyard_key: "repo.tag_style".to_owned(),
        value: tag.clone(),
        note: Some(
            "knope's documented multi-package tag shape (\"each package gets its own tag in \
             the format {package_name}/v{version}\"); validated against the appendix-A.3 \
             template grammar"
                .to_owned(),
        ),
    });
    for (name, pkg) in pkgs {
        let pkg = pkg.as_table().ok_or_else(|| ConfigError::Migrate {
            path: KN_CONFIG.to_owned(),
            message: format!("`[packages.\"{name}\"]` must be a table"),
        })?;
        packages.push(kn_package_survey(name, pkg, &tag, KN_CONFIG)?);
        for key in pkg.keys() {
            unmapped.push(unmapped_from_table(
                &format!("packages[{name}].{key}"),
                key,
                KN_UNMAPPED_PACKAGE,
                KN_FALLBACK,
            ));
        }
    }
    Ok(tag)
}

/// The workflows advisory row (the configured names ride the reason) —
/// `None` when the config declares no `[[workflows]]`.
fn kn_workflows_row(value: &toml::Table) -> Result<Option<UnmappedRow>, ConfigError> {
    let Some(workflows) = value.get("workflows") else {
        return Ok(None);
    };
    let items = workflows.as_array().ok_or_else(|| ConfigError::Migrate {
        path: KN_CONFIG.to_owned(),
        message: "`[[workflows]]` must be an array of workflow tables".to_owned(),
    })?;
    let mut names = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let table = item.as_table().ok_or_else(|| ConfigError::Migrate {
            path: KN_CONFIG.to_owned(),
            message: format!("`[[workflows]]` entry {index} must be a table"),
        })?;
        let name = kn_str(table, "name", KN_CONFIG)?.unwrap_or_else(|| "(unnamed)".to_owned());
        names.push(name);
    }
    Ok(Some(unmapped_row(
        "workflows",
        &format!(
            "knope's workflow automation (the PrepareRelease/Release steps) is halyard's own \
             pipeline (§16.2); the configured names carry no config mapping — configured: {}",
            names.join(", ")
        ),
    )))
}

impl MigrationSource for KnopeSource {
    fn tool(&self) -> &'static str {
        "knope"
    }

    fn config_paths(&self) -> &'static [&'static str] {
        &[KN_CONFIG]
    }

    fn survey(&self, files: &ForeignFiles) -> Result<ToolSurvey, ConfigError> {
        let mut found: Vec<String> = self
            .config_paths()
            .iter()
            .filter(|p| files.get(p).is_some())
            .map(|p| (*p).to_owned())
            .collect();
        found.sort();
        let Some(raw) = files.get(KN_CONFIG) else {
            return Ok(ToolSurvey {
                tool: self.tool().to_owned(),
                configured: false,
                ..ToolSurvey::default()
            });
        };
        let value = toml::from_str::<toml::Table>(raw).map_err(|error| ConfigError::Migrate {
            path: KN_CONFIG.to_owned(),
            message: format!("invalid TOML: {error}"),
        })?;

        let mut rows: Vec<MappingRow> = Vec::new();
        let mut unmapped: Vec<UnmappedRow> = Vec::new();
        let mut survey_packages: Vec<PackageSurvey> = Vec::new();

        // The two documented shapes are exclusive: one `[package]` or
        // named `[packages."<name>"]` sub-tables. Both at once is a
        // broken config, not a guess.
        let shape = match (value.get("package"), value.get("packages")) {
            (Some(_), Some(_)) => {
                return Err(ConfigError::Migrate {
                    path: KN_CONFIG.to_owned(),
                    message: "the knope config cannot declare both `[package]` and `[packages]` \
                              — the Release step's two documented package shapes are exclusive"
                        .to_owned(),
                });
            }
            (Some(pkg), None) => {
                let pkg = pkg.as_table().ok_or_else(|| ConfigError::Migrate {
                    path: KN_CONFIG.to_owned(),
                    message: "`[package]` must be a table".to_owned(),
                })?;
                Some(kn_survey_single_package(
                    pkg,
                    &mut rows,
                    &mut unmapped,
                    &mut survey_packages,
                )?)
            }
            (None, Some(pkgs)) => {
                let pkgs = pkgs.as_table().ok_or_else(|| ConfigError::Migrate {
                    path: KN_CONFIG.to_owned(),
                    message: "`[packages]` must be a table of named package sub-tables".to_owned(),
                })?;
                if pkgs.is_empty() {
                    // An empty `[packages]` table declares no package —
                    // the same advisory as the shapeless config.
                    None
                } else {
                    Some(kn_survey_multi_packages(
                        pkgs,
                        &mut rows,
                        &mut unmapped,
                        &mut survey_packages,
                    )?)
                }
            }
            (None, None) => None,
        };
        if shape.is_none() {
            unmapped.push(unmapped_row(
                "(package section)",
                "the knope config declares no package section — the Release step's tag shape is \
                 unknown; set `repo.tag_style` against this repository's existing tags \
                 (appendix A.3)",
            ));
        }
        if let Some(workflows) = kn_workflows_row(&value)? {
            unmapped.push(workflows);
        }

        // Root keys that are not mapped.
        for key in value.keys() {
            match key.as_str() {
                "package" | "packages" | "workflows" => {}
                _ => unmapped.push(unmapped_from_table(key, key, KN_UNMAPPED_ROOT, KN_FALLBACK)),
            }
        }

        rows.sort_by(|a, b| a.foreign_key.cmp(&b.foreign_key));
        unmapped.sort_by(|a, b| a.foreign_key.cmp(&b.foreign_key));
        survey_packages.sort_by(|a, b| a.path.cmp(&b.path));

        let mut config = HalyardConfig::default();
        if let Some(template) = shape {
            config.repo = Some(RepoCfg {
                base_branch: None,
                tag_style: Some(TagStyle(template)),
            });
        }

        Ok(ToolSurvey {
            tool: self.tool().to_owned(),
            configured: true,
            files: found,
            rows,
            unmapped,
            packages: survey_packages,
            draft: Some(render_draft(&config, KN_CONFIG)?),
        })
    }
}

// ---------------------------------------------------------------------------
// release-plz (the §16.2 extension)
// ---------------------------------------------------------------------------

/// The release-plz config (`release-plz.toml`, `.release-plz.toml`,
/// `.config/release-plz.toml` — VERIFIED against the tool's
/// configuration reference: the documented top-down search order, the
/// `[workspace]` defaults and the Tera templates; quotes in
/// `docs/research/migrate-config-grammars.md`).
#[derive(Debug)]
pub struct ReleasePlzSource;

/// The documented search order: the first existing path wins and the
/// shadowed files are not read.
const PLZ_CONFIGS: &[&str] = &[
    "release-plz.toml",
    ".release-plz.toml",
    ".config/release-plz.toml",
];

/// The documented workspace default for the release/tag name template
/// (the single-crate default `v{{ version }}` rides the same rule
/// halyard's `auto` applies, so the multi-package shape is what a
/// survey can pin without guessing).
const PLZ_DEFAULT_TAG: &str = "{name}-v{version}";

/// `[workspace]` keys that carry no v1 equivalent, each with the reason
/// the report shows.
const PLZ_UNMAPPED_WORKSPACE: &[(&str, &str)] = &[
    (
        "allow_dirty",
        "release-plz's dirty-worktree refusal is a run-time gate; halyard's shell presents the \
         tree state the caller is in",
    ),
    (
        "changelog_config",
        "deprecated upstream in favor of `[changelog]`; the git-cliff header/body template is \
         not a v1 key",
    ),
    (
        "changelog_path",
        "the per-package changelog path has no v1 equivalent — halyard renders `CHANGELOG.md` \
         per package (§9.4)",
    ),
    (
        "custom_major_increment_regex",
        "regex-triggered bumps are not a v1 key — commit types map through `commits.types` \
         (§16.4)",
    ),
    (
        "custom_minor_increment_regex",
        "regex-triggered bumps are not a v1 key — commit types map through `commits.types` \
         (§16.4)",
    ),
    (
        "dependencies_update",
        "halyard's rewriter updates workspace requirements itself (appendix A.2)",
    ),
    (
        "features_always_increment_minor",
        "release-plz's pre-1.0 feat behavior (the documented default bumps the minor only \
         starting with 1.x releases) does not map onto halyard's §10.2 defaults — review \
         `versioning.pre_major` deliberately",
    ),
    (
        "git_release_body",
        "the forge release body is the §11.1 machine's forge half's concern",
    ),
    (
        "git_release_draft",
        "draft forge releases are a forge-side option (§11.1)",
    ),
    (
        "git_release_enable",
        "forge releases ride the §11.1 machine's forge half; suppression is per-package \
         `release = false` (§16.4)",
    ),
    (
        "git_release_generate_notes",
        "forge-generated release notes are a forge-side option (§11.1)",
    ),
    (
        "git_release_latest",
        "the forge release's latest-flag is a forge-side option (§11.1)",
    ),
    (
        "git_release_name",
        "the release name shape is not a v1 key (§11.1)",
    ),
    (
        "git_release_type",
        "the forge release's prod/pre/auto shape is a forge-side option (§11.1)",
    ),
    (
        "git_only",
        "halyard's publish is opt-in (`publish.enabled`, invariant I7), so \
         tags-without-publish is already the default shape",
    ),
    (
        "max_analyze_commits",
        "halyard attributes the history itself (§8.3); a scan bound is not a v1 key",
    ),
    (
        "pr_body",
        "the §11.3 body renderer owns the Release-PR shape; custom templates are not a v1 key",
    ),
    (
        "pr_branch_prefix",
        "the Release-PR branch shape is the §11.1/§11.2 machine's concern",
    ),
    ("pr_draft", "draft Release PRs are not a v1 key (§11.3)"),
    (
        "pr_labels",
        "Release-PR labels land with the §11.2 branch/label work",
    ),
    (
        "pr_name",
        "the §11.3 body renderer owns the Release-PR shape; custom templates are not a v1 key",
    ),
    (
        "publish_allow_dirty",
        "cargo publish run knobs are the spawned tool's own flags (§13)",
    ),
    (
        "publish_all_features",
        "cargo publish run knobs are the spawned tool's own flags (§13)",
    ),
    (
        "publish_features",
        "cargo publish run knobs are the spawned tool's own flags (§13)",
    ),
    (
        "publish_no_verify",
        "cargo publish run knobs are the spawned tool's own flags (§13)",
    ),
    (
        "publish_timeout",
        "the publish timeout is the spawned tool run's own knob (§13)",
    ),
    (
        "release",
        "halyard releases what discovery finds unless `[[package]] release = false` (§16.4)",
    ),
    (
        "release_always",
        "release-plz's on-push release trigger is CI glue; halyard's shell decides when a \
         release runs",
    ),
    (
        "release_commits",
        "commit filtering is halyard's attribution (§8.3) — the regex dialects differ, so the \
         report does not map them for you",
    ),
    (
        "repo_url",
        "the compare URL derives from the git remote (§9.4)",
    ),
    ("semver_check", "cargo-semver-checks gating is not a v1 key"),
];

/// Root keys outside `[workspace]`/`[[package]]` that carry no v1
/// equivalent.
const PLZ_UNMAPPED_ROOT: &[(&str, &str)] = &[(
    "changelog",
    "the git-cliff template body/header is not a v1 key — halyard renders the §9.4 styles (the \
     documented default format is what the `changelog.style` row pins)",
)];

const PLZ_FALLBACK: &str = "documented release-plz option with no v1 equivalent (see \
                            docs/research/migrate-config-grammars.md)";

/// Translates a documented Tera tag/release-name template onto the
/// appendix-A.3 grammar: `{{ package }}` → `{name}`, `{{ version }}` →
/// `{version}`. `None` when any other Tera variable, block or
/// whitespace variant survives the translation — the migration never
/// half-translates a template it cannot verify.
fn plz_translate_tag_template(raw: &str) -> Option<String> {
    let translated = raw
        .replace("{{ package }}", "{name}")
        .replace("{{ version }}", "{version}");
    let leftover = translated.contains("{{")
        || translated.contains("}}")
        || translated.contains("{%")
        || translated.contains("%}");
    (!leftover && TagTemplate::parse(&translated).is_ok()).then_some(translated)
}

/// Reads a TOML boolean key, reporting a type mismatch fail-closed.
fn plz_bool(table: &toml::Table, key: &str, path: &str) -> Result<Option<bool>, ConfigError> {
    match table.get(key) {
        None => Ok(None),
        Some(toml::Value::Boolean(value)) => Ok(Some(*value)),
        Some(other) => Err(ConfigError::Migrate {
            path: path.to_owned(),
            message: format!("`{key}` must be a boolean, got {other}"),
        }),
    }
}

/// Reads a TOML string key, reporting a type mismatch fail-closed.
fn plz_str(table: &toml::Table, key: &str, path: &str) -> Result<Option<String>, ConfigError> {
    match table.get(key) {
        None => Ok(None),
        Some(toml::Value::String(value)) => Ok(Some(value.clone())),
        Some(other) => Err(ConfigError::Migrate {
            path: path.to_owned(),
            message: format!("`{key}` must be a string, got {other}"),
        }),
    }
}

/// One `[[package]]` entry's resolved inputs: the crate name, the tag
/// template it resolved to (its own override, or the workspace default
/// when it carries none — `None` when no template resolves, which is
/// always paired with an already-reported unmapped row), whether that
/// template came from the override, its release decision, and the raw
/// entry for the per-key scan.
struct PlzPackage {
    name: String,
    resolved_tag: Option<String>,
    tag_from_override: bool,
    release: bool,
    version_group: Option<String>,
    entry: toml::Table,
}

/// The reason an untranslatable Tera template reports (the placeholders
/// the appendix-A.3 grammar accepts).
fn plz_untranslatable_row(foreign_key: &str, raw: &str) -> UnmappedRow {
    unmapped_row(
        foreign_key,
        &format!(
            "the Tera template `{raw}` carries variables the appendix-A.3 grammar cannot \
             express — pick a template built from the documented `{{{{ package }}}}`/`{{{{ \
             version }}}}` placeholders"
        ),
    )
}

/// Reads the `[[package]]` entries (the reference marks `name`
/// required): each entry's tag override translates onto the appendix-A.3
/// grammar, falling back to the workspace default when it carries none.
fn plz_read_packages(
    value: &toml::Table,
    path: &str,
    fill: Option<&str>,
    unmapped: &mut Vec<UnmappedRow>,
) -> Result<Vec<PlzPackage>, ConfigError> {
    let Some(entries) = value.get("package") else {
        return Ok(Vec::new());
    };
    let entries = entries.as_array().ok_or_else(|| ConfigError::Migrate {
        path: path.to_owned(),
        message: "[[package]] must be an array of package tables".to_owned(),
    })?;
    let mut plz_packages = Vec::with_capacity(entries.len());
    for entry in entries {
        let entry = entry.as_table().ok_or_else(|| ConfigError::Migrate {
            path: path.to_owned(),
            message: "[[package]] entries must be tables".to_owned(),
        })?;
        let name = plz_str(entry, "name", path)?.ok_or_else(|| ConfigError::Migrate {
            path: path.to_owned(),
            message: "every [[package]] entry must carry `name` (the reference marks it \
                      required)"
                .to_owned(),
        })?;
        let override_tag = plz_str(entry, "git_tag_name", path)?.and_then(|raw| {
            plz_translate_tag_template(&raw).or_else(|| {
                unmapped.push(plz_untranslatable_row(
                    &format!("packages[{name}].git_tag_name"),
                    &raw,
                ));
                None
            })
        });
        let (resolved_tag, tag_from_override) = match override_tag {
            Some(template) => (Some(template), true),
            None => (fill.map(str::to_owned), false),
        };
        let release = plz_bool(entry, "release", path)?.unwrap_or(true);
        let version_group = plz_str(entry, "version_group", path)?;
        plz_packages.push(PlzPackage {
            name,
            resolved_tag,
            tag_from_override,
            release,
            version_group,
            entry: entry.clone(),
        });
    }
    plz_packages.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(plz_packages)
}

/// The tag-style row: the workspace template (or the documented
/// default), overridden per package — a unified resolution pins
/// `repo.tag_style`; a disagreement is a reported semantic difference,
/// never a pick. The unified template also lands on the draft.
fn plz_tag_row(
    workspace: &toml::Table,
    path: &str,
    plz_packages: &[PlzPackage],
    fill: Option<&String>,
    used_default: bool,
    unmapped: &mut Vec<UnmappedRow>,
    draft: &mut HalyardConfig,
) -> Result<Option<MappingRow>, ConfigError> {
    let tag_enabled = plz_bool(workspace, "git_tag_enable", path)?;
    if tag_enabled == Some(false) {
        unmapped.push(unmapped_row(
            "git_tag_enable",
            "release-plz creates no tags when false — set `repo.tag_style` deliberately if this \
             repository tags releases another way",
        ));
        return Ok(None);
    }
    let Some(fill) = fill else {
        // The workspace template was untranslatable (its row is already
        // reported); nothing unifies.
        return Ok(None);
    };
    let unified = if plz_packages.iter().any(|p| p.resolved_tag.is_none()) {
        // An unresolvable per-package template already reported its
        // row; nothing unifies.
        None
    } else if plz_packages.is_empty() {
        Some(fill.clone())
    } else {
        let templates: Vec<&String> = plz_packages
            .iter()
            .filter_map(|p| p.resolved_tag.as_ref())
            .collect();
        let first: Option<&String> = templates.first().copied();
        if templates.iter().all(|t| Some(*t) == first) {
            first.cloned()
        } else {
            let shapes: Vec<String> = plz_packages
                .iter()
                .filter_map(|p| {
                    p.resolved_tag
                        .as_ref()
                        .map(|template| format!("{}: {template}", p.name))
                })
                .collect();
            unmapped.push(unmapped_row(
                "git_tag_name (per-package disagreement)",
                &format!(
                    "the packages disagree about the tag shape ({}); halyard v1 carries one \
                     repo-wide `repo.tag_style` — pick one deliberately after review",
                    shapes.join(", ")
                ),
            ));
            None
        }
    };
    Ok(unified.map(|template| {
        let any_override = plz_packages
            .iter()
            .any(|p| p.tag_from_override && p.resolved_tag.is_some());
        let (foreign_key, note) = if used_default && !any_override {
            (
                "git_tag_name (workspace default)",
                "release-plz's documented workspace default; halyard's `auto` resolves the \
                 same rule (single-package repos release `v{version}`)",
            )
        } else {
            (
                "git_tag_name",
                "translated from the release-plz Tera template; validated against the \
                 appendix-A.3 template grammar",
            )
        };
        draft.repo = Some(RepoCfg {
            base_branch: None,
            tag_style: Some(TagStyle(template.clone())),
        });
        MappingRow {
            foreign_key: foreign_key.to_owned(),
            halyard_key: "repo.tag_style".to_owned(),
            value: template,
            note: Some(note.to_owned()),
        }
    }))
}

/// The changelog rows: the documented default format pins the v1 style;
/// an explicit `changelog_update = false` reports the difference
/// instead.
fn plz_changelog_rows(
    workspace: &toml::Table,
    path: &str,
    rows: &mut Vec<MappingRow>,
    unmapped: &mut Vec<UnmappedRow>,
    draft: &mut HalyardConfig,
) -> Result<(), ConfigError> {
    let changelog_update = plz_bool(workspace, "changelog_update", path)?;
    match changelog_update {
        None | Some(true) => {
            let foreign_key = if changelog_update.is_none() {
                "changelog_update (default)"
            } else {
                "changelog_update"
            };
            rows.push(MappingRow {
                foreign_key: foreign_key.to_owned(),
                halyard_key: "changelog.style".to_owned(),
                value: "keep-a-changelog".to_owned(),
                note: Some(
                    "release-plz's documented default (Keep a Changelog format) — matches \
                     halyard's built-in default; pinned explicitly"
                        .to_owned(),
                ),
            });
            draft.changelog = Some(ChangelogCfg {
                style: Some("keep-a-changelog".to_owned()),
                hidden_types: None,
            });
        }
        Some(false) => {
            unmapped.push(unmapped_row(
                "changelog_update",
                "release-plz leaves changelogs untouched when false — halyard renders \
                 changelogs per §9.4, so set `changelog.style` deliberately",
            ));
        }
    }
    Ok(())
}

/// The `[[package]].release` row for the release-disabled entries (the
/// one key the v1 surface expresses exactly) + the draft overrides.
fn plz_release_rows(
    plz_packages: &[PlzPackage],
    rows: &mut Vec<MappingRow>,
    draft: &mut HalyardConfig,
) {
    let declined: Vec<&str> = plz_packages
        .iter()
        .filter(|p| !p.release)
        .map(|p| p.name.as_str())
        .collect();
    if declined.is_empty() {
        return;
    }
    rows.push(MappingRow {
        foreign_key: "[[package]].release".to_owned(),
        halyard_key: "[[package]].release".to_owned(),
        value: format!("false for: {}", declined.join(", ")),
        note: Some(
            "release-plz's per-package `release = false` overrides become halyard's own (§16.4)"
                .to_owned(),
        ),
    });
    for name in &declined {
        draft.package.push(PackageOverride {
            name: Some((*name).to_owned()),
            release: Some(false),
            include: Vec::new(),
            exclude: Vec::new(),
        });
    }
}

/// The per-package survey rows (the foreign tool's own view) + the
/// per-key unmapped rows.
fn plz_survey_packages(
    plz_packages: &[PlzPackage],
    fill: Option<&str>,
    unmapped: &mut Vec<UnmappedRow>,
) -> Vec<PackageSurvey> {
    plz_packages
        .iter()
        .map(|package| {
            for key in package.entry.keys() {
                match key.as_str() {
                    "name" | "git_tag_name" | "release" | "version_group" => {}
                    _ => unmapped.push(unmapped_from_table(
                        &format!("packages[{}].{key}", package.name),
                        key,
                        &[],
                        PLZ_FALLBACK,
                    )),
                }
            }
            PackageSurvey {
                // release-plz's `[[package]].name` is the crate name —
                // the survey carries it as the row key.
                path: package.name.clone(),
                component: None,
                tag: package
                    .resolved_tag
                    .clone()
                    .unwrap_or_else(|| fill.unwrap_or_default().to_owned()),
                release: package.release,
                notes: Vec::new(),
            }
        })
        .collect()
}

/// The `version_group` review row (halyard's linked policy is
/// workspace-wide, so a group is never a pin) — `None` when no entry
/// carries one.
fn plz_version_group_row(plz_packages: &[PlzPackage]) -> Option<UnmappedRow> {
    let groups: Vec<String> = plz_packages
        .iter()
        .filter_map(|p| {
            p.version_group
                .as_ref()
                .map(|group| format!("{} → {group}", p.name))
        })
        .collect();
    if groups.is_empty() {
        return None;
    }
    Some(unmapped_row(
        "version_group",
        &format!(
            "release-plz's version groups are package subsets; halyard's \
             `versioning.policy = \"linked\"` is workspace-wide (§5.4) — review the groups \
             before pinning (configured: {})",
            groups.join(", ")
        ),
    ))
}

/// The publish advisory row: the master switch is never set by a
/// migration (invariant I7).
fn plz_publish_row(workspace: &toml::Table, path: &str) -> Result<UnmappedRow, ConfigError> {
    let publish = plz_bool(workspace, "publish", path)?;
    Ok(unmapped_row(
        "publish",
        if publish == Some(false) {
            "release-plz leaves publishing off; halyard publishing stays opt-in anyway \
             (`publish.enabled` is never set by a migration — invariant I7)"
        } else {
            "release-plz publishes by default; halyard publishing stays opt-in \
             (`publish.enabled` is never set by a migration — invariant I7)"
        },
    ))
}

impl MigrationSource for ReleasePlzSource {
    fn tool(&self) -> &'static str {
        "release-plz"
    }

    fn config_paths(&self) -> &'static [&'static str] {
        PLZ_CONFIGS
    }

    fn survey(&self, files: &ForeignFiles) -> Result<ToolSurvey, ConfigError> {
        // The documented top-down search order: the first existing path
        // wins, the shadowed paths are not read.
        let found: Vec<String> = PLZ_CONFIGS
            .iter()
            .filter(|p| files.get(p).is_some())
            .map(|p| (*p).to_owned())
            .take(1)
            .collect();
        let Some(path) = found.first().cloned() else {
            return Ok(ToolSurvey {
                tool: self.tool().to_owned(),
                configured: false,
                ..ToolSurvey::default()
            });
        };
        let Some(raw) = files.get(&path) else {
            // Defensive: the shell only feeds discovered files, so this
            // is the same honest not-configured shape.
            return Ok(ToolSurvey {
                tool: self.tool().to_owned(),
                configured: false,
                ..ToolSurvey::default()
            });
        };
        let value = toml::from_str::<toml::Table>(raw).map_err(|error| ConfigError::Migrate {
            path: path.clone(),
            message: format!("invalid TOML: {error}"),
        })?;

        let mut rows: Vec<MappingRow> = Vec::new();
        let mut unmapped: Vec<UnmappedRow> = Vec::new();
        let mut draft = HalyardConfig::default();

        // The `[workspace]` table (release-plz applies it to every
        // package; per-package overrides follow).
        let empty_workspace = toml::Table::new();
        let workspace = match value.get("workspace") {
            None => &empty_workspace,
            Some(workspace) => workspace.as_table().ok_or_else(|| ConfigError::Migrate {
                path: path.clone(),
                message: "[workspace] must be a table".to_owned(),
            })?,
        };

        // The workspace template (or the documented default) is what a
        // package without its own override resolves to. `None` = the
        // configured template was untranslatable (its row is reported
        // below) — the survey never half-translates.
        let ws_tag_raw = plz_str(workspace, "git_tag_name", &path)?;
        let used_default = ws_tag_raw.is_none();
        let fill: Option<String> = match &ws_tag_raw {
            None => Some(PLZ_DEFAULT_TAG.to_owned()),
            Some(raw) => plz_translate_tag_template(raw).or_else(|| {
                unmapped.push(plz_untranslatable_row("git_tag_name", raw));
                None
            }),
        };

        // The `[[package]]` entries, the tag-style resolution, the
        // release/changelog rows and the survey rows.
        let plz_packages = plz_read_packages(&value, &path, fill.as_deref(), &mut unmapped)?;
        let tag_row = plz_tag_row(
            workspace,
            &path,
            &plz_packages,
            fill.as_ref(),
            used_default,
            &mut unmapped,
            &mut draft,
        )?;
        if let Some(row) = tag_row {
            rows.push(row);
        }
        plz_release_rows(&plz_packages, &mut rows, &mut draft);
        plz_changelog_rows(workspace, &path, &mut rows, &mut unmapped, &mut draft)?;
        unmapped.push(plz_publish_row(workspace, &path)?);
        if let Some(group_row) = plz_version_group_row(&plz_packages) {
            unmapped.push(group_row);
        }
        let mut survey_packages =
            plz_survey_packages(&plz_packages, fill.as_deref(), &mut unmapped);

        // Workspace keys that are not mapped.
        for key in workspace.keys() {
            match key.as_str() {
                "git_tag_enable" | "git_tag_name" | "changelog_update" | "publish" => {}
                _ => unmapped.push(unmapped_from_table(
                    key,
                    key,
                    PLZ_UNMAPPED_WORKSPACE,
                    PLZ_FALLBACK,
                )),
            }
        }
        // Root keys outside `[workspace]`/`[[package]]` that are not
        // mapped.
        for key in value.keys() {
            match key.as_str() {
                "workspace" | "package" => {}
                _ => unmapped.push(unmapped_from_table(
                    key,
                    key,
                    PLZ_UNMAPPED_ROOT,
                    PLZ_FALLBACK,
                )),
            }
        }

        rows.sort_by(|a, b| a.foreign_key.cmp(&b.foreign_key));
        unmapped.sort_by(|a, b| a.foreign_key.cmp(&b.foreign_key));
        survey_packages.sort_by(|a, b| a.path.cmp(&b.path));

        Ok(ToolSurvey {
            tool: self.tool().to_owned(),
            configured: true,
            files: found,
            rows,
            unmapped,
            packages: survey_packages,
            draft: Some(render_draft(&draft, &path)?),
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

// ---------------------------------------------------------------------------
// The §16.2 write step (M4.7 step 2): the validated draft lands at
// `halyard.toml` through the injected sink — the pure planner picks the
// draft, the runner refuses an existing target and writes atomically.
// ---------------------------------------------------------------------------

/// The write target: the migration always lands `halyard.toml` at the
/// repo root (the directory the CLI runs in) — the v1 loader's own
/// conventional path.
pub const DRAFT_TARGET: &str = "halyard.toml";

/// The write-effect seam (M4.7 step 2, §16.2): where the validated
/// draft lands. Object-safe and injected like the engine's other
/// effect seams — the shell wires [`AtomicDraftSink`] (the real
/// filesystem), tests wire [`MemoryDraftSink`] (the recording fake).
pub trait DraftSink: std::fmt::Debug {
    /// Whether the target path already exists. The overwrite gate reads
    /// this before any write; the migration never clobbers a config.
    fn exists(&self, path: &str) -> Result<bool, ConfigError>;

    /// Creates `path` with `contents`, atomically (temp sibling →
    /// fsync → rename) and create-only: an existing target is refused
    /// with [`ConfigError::Migrate`], never overwritten. A refused or
    /// failed write leaves no partial file behind.
    fn write(&self, path: &str, contents: &str) -> Result<(), ConfigError>;
}

/// Temp-file counter (process-unique; combined with the pid), mirroring
/// the edit engine's atomic protocol (§9.1).
static DRAFT_TEMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The production sink: the real filesystem with the §9.1 atomic
/// protocol — temp sibling in the target's directory, `fsync`, rename.
/// The target's parent directory must already exist (the migration
/// writes `halyard.toml` into the repo root, never mkdirs on the user's
/// behalf); the exists gate is re-checked at write time.
#[derive(Clone, Copy, Debug, Default)]
pub struct AtomicDraftSink;

impl DraftSink for AtomicDraftSink {
    fn exists(&self, path: &str) -> Result<bool, ConfigError> {
        Ok(std::path::Path::new(path).exists())
    }

    fn write(&self, path: &str, contents: &str) -> Result<(), ConfigError> {
        let path = std::path::Path::new(path);
        let refuse = |message: String| ConfigError::Migrate {
            path: path.display().to_string(),
            message,
        };
        if path.exists() {
            return Err(refuse(
                "the target already exists — the migration never overwrites a configuration; \
                 merge the draft into it by hand and remove the foreign config"
                    .to_owned(),
            ));
        }
        // A bare `halyard.toml` has an empty parent path — that *is*
        // the current directory (the CLI `-C` chdir lands here).
        let dir = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        if !dir.is_dir() {
            return Err(refuse(
                "the target's directory does not exist — the migration writes into the \
                 repository root and never creates directories on the user's behalf"
                    .to_owned(),
            ));
        }
        let temp = dir.join(format!(
            ".halyard-migrate-{}-{}.tmp",
            std::process::id(),
            DRAFT_TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let write_result = (|| -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            file.write_all(contents.as_bytes())?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temp, path)
        })();
        if let Err(source) = write_result {
            let _ = std::fs::remove_file(&temp);
            return Err(refuse(format!(
                "the atomic write failed (no partial file is left behind): {source}"
            )));
        }
        // Best-effort directory durability (POSIX); a filesystem that
        // refuses the dir fsync does not fail an already-renamed write.
        #[cfg(unix)]
        if let Ok(dir_file) = std::fs::File::open(dir) {
            let _ = dir_file.sync_all();
        }
        Ok(())
    }
}

/// The state a [`MemoryDraftSink`] records behind its lock.
#[derive(Debug, Default)]
struct MemorySinkState {
    /// Every `(path, contents)` write, in order.
    writes: Vec<(String, String)>,
    /// The latest content per path (the observable end state).
    latest: BTreeMap<String, String>,
}

/// The reference fake for the seam (the engine's convention: one
/// deterministic fake beside the real effect): models pre-existing
/// files, records every write in order, and observes the end state —
/// `Debug`-printable so a failing test shows what landed.
#[derive(Debug, Default)]
pub struct MemoryDraftSink {
    existing: BTreeSet<String>,
    state: std::sync::Mutex<MemorySinkState>,
}

impl MemoryDraftSink {
    /// An empty repo: no pre-existing files, nothing written.
    pub fn new() -> Self {
        Self::default()
    }

    /// Models one pre-existing file (the overwrite gate's subject).
    pub fn with_existing(path: impl Into<String>) -> Self {
        let mut sink = Self::new();
        sink.existing.insert(path.into());
        sink
    }

    /// The recorded writes, in order (`(path, contents)` pairs).
    pub fn writes(&self) -> Vec<(String, String)> {
        self.lock().writes.clone()
    }

    /// The latest content written to `path`, if any. A modeled
    /// pre-existing file that no write touched reads as `None` (the
    /// sink never fabricates its content).
    pub fn content(&self, path: &str) -> Option<String> {
        self.lock().latest.get(path).cloned()
    }

    /// Locks the state, recovering the data if a panic poisoned it
    /// (no panic path exists inside the critical section).
    fn lock(&self) -> std::sync::MutexGuard<'_, MemorySinkState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl DraftSink for MemoryDraftSink {
    fn exists(&self, path: &str) -> Result<bool, ConfigError> {
        Ok(self.existing.contains(path) || self.lock().latest.contains_key(path))
    }

    fn write(&self, path: &str, contents: &str) -> Result<(), ConfigError> {
        if self.exists(path)? {
            return Err(ConfigError::Migrate {
                path: path.to_owned(),
                message: "the target already exists — the memory sink mirrors the create-only \
                          production contract"
                    .to_owned(),
            });
        }
        let mut state = self.lock();
        state.writes.push((path.to_owned(), contents.to_owned()));
        state.latest.insert(path.to_owned(), contents.to_owned());
        Ok(())
    }
}

/// The write plan (M4.7 step 2): which draft lands where — plain data,
/// computed before any side effect (validate-then-act, §15.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftWritePlan {
    /// The tool whose draft was selected.
    pub tool: String,
    /// The target path (always [`DRAFT_TARGET`]).
    pub path: String,
    /// The validated draft, verbatim from the report (what the human
    /// reviewed is exactly what lands — no re-serialization drift).
    pub contents: String,
}

/// The deterministic write decision over the report (M4.7 step 2):
///
/// * no configured tool, or a configured tool whose draft is empty
///   (nothing mappable) → `None` — absence is a value, the caller
///   reports an honest no-op;
/// * exactly one configured tool with a non-empty draft → that draft,
///   verbatim;
/// * two or more → [`ConfigError::Migrate`] naming every candidate:
///   each tool drafts a different `halyard.toml` and the migration
///   never guesses which shape wins.
pub fn plan_draft_write(report: &MigrationReport) -> Result<Option<DraftWritePlan>, ConfigError> {
    let candidates: Vec<&ToolSurvey> = report
        .tools
        .iter()
        .filter(|survey| {
            survey.configured && survey.draft.as_deref().is_some_and(|d| !d.is_empty())
        })
        .collect();
    match candidates.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(DraftWritePlan {
            tool: one.tool.clone(),
            path: DRAFT_TARGET.to_owned(),
            contents: one.draft.clone().unwrap_or_default(),
        })),
        many => {
            let names: Vec<&str> = many.iter().map(|survey| survey.tool.as_str()).collect();
            Err(ConfigError::Migrate {
                path: DRAFT_TARGET.to_owned(),
                message: format!(
                    "multiple foreign tools are configured ({}) — each drafts a different \
                     halyard.toml and the migration never guesses which shape wins; compare \
                     the drafts in the report and remove or merge the foreign configs first",
                    names.join(", ")
                ),
            })
        }
    }
}

/// What the write step did (plain data, I3). The default is the honest
/// no-op: nothing configured, nothing written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftWriteOutcome {
    /// No foreign configuration was detected — absence is a value, the
    /// write is an honest no-op.
    #[default]
    NothingConfigured,
    /// The validated draft was written to [`DRAFT_TARGET`].
    Written,
}

/// The write step's outcome report (M4.7 step 2): the command's JSON
/// carries it as the `write` block when `--write` ran; nothing-configured
/// is a reported outcome, not an error.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftWriteReport {
    /// The tool whose draft landed (empty when nothing was written).
    pub tool: String,
    /// The path written (empty when nothing was written).
    pub path: String,
    /// The draft's byte length (0 when nothing was written).
    pub bytes: usize,
    /// What happened.
    pub outcome: DraftWriteOutcome,
}

/// Runs the write plan over the sink: refuses an existing target
/// (validate-then-act — the gate reads before any write) and writes
/// atomically. `None` plans produce the honest
/// [`DraftWriteOutcome::NothingConfigured`] without touching the sink.
pub fn run_draft_write(
    plan: Option<DraftWritePlan>,
    sink: &dyn DraftSink,
) -> Result<DraftWriteReport, ConfigError> {
    let Some(plan) = plan else {
        return Ok(DraftWriteReport {
            outcome: DraftWriteOutcome::NothingConfigured,
            ..DraftWriteReport::default()
        });
    };
    if sink.exists(&plan.path)? {
        return Err(ConfigError::Migrate {
            path: plan.path.clone(),
            message: format!(
                "the target already exists — the migration never overwrites a configuration; \
                 merge the {} draft into it by hand and remove the foreign config",
                plan.tool
            ),
        });
    }
    sink.write(&plan.path, &plan.contents)?;
    Ok(DraftWriteReport {
        tool: plan.tool,
        bytes: plan.contents.len(),
        path: plan.path,
        outcome: DraftWriteOutcome::Written,
    })
}
