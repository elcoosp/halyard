//! `halyard.toml` v1 configuration types (§16.4).
//!
//! All keys are optional. Serde defaults mirror the documented built-in
//! defaults so a `HalyardConfig` deserialized from an empty table equals the
//! zero-config behaviour. Map-shaped fields use `BTreeMap`/sorted vectors so
//! no configuration output path ever depends on hash-map iteration order
//! (invariant I3).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Root of `halyard.toml` (§16.4). Unknown keys are reported as errors with
/// `did-you-mean` suggestions by the loader (`M1.8`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct HalyardConfig {
    /// Repository-wide options: base branch, tag naming.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<RepoCfg>,
    /// Versioning policy: grouping, pre-major handling, cascade.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub versioning: Option<VersioningCfg>,
    /// Commit-derived bump configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commits: Option<CommitsCfg>,
    /// Changelog rendering options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changelog: Option<ChangelogCfg>,
    /// Publishing options (opt-in; §13).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publish: Option<PublishCfg>,
    /// Forge adapter selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forge: Option<ForgeCfg>,
    /// Per-package overrides, matched by name or path glob (`[[package]]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub package: Vec<PackageOverride>,
    /// Explicit polyglot links (`[[link]]`, §6.5).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub link: Vec<LinkCfg>,
}

/// `[repo]` section.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct RepoCfg {
    /// Base branch plans are computed against. Default: the remote HEAD.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
    /// Tag naming style: `auto`, a tag template such as `{name}-v{version}`,
    /// or `v{version}` (appendix A.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag_style: Option<TagStyle>,
}

/// Tag naming style (appendix A.3): the literal `auto`, or a template
/// containing `{name}` / `{version}` placeholders such as `{name}-v{version}`
/// or `v{version}`. The loader (`M1.8`) validates `auto` and the template
/// grammar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct TagStyle(pub String);

/// `[versioning]` section.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct VersioningCfg {
    /// `independent` (default) | `fixed` | `linked` (§5.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<VersionPolicy>,
    /// Breaking/feat mapping in `0.y.z` releases (§10.2); Cargo-compatible
    /// defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_major: Option<PreMajorCfg>,
    /// Cascade policy for dependents of bumped packages (§10.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cascade: Option<CascadePolicy>,
}

/// Workspace versioning policy (§5.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum VersionPolicy {
    /// Every package versions on its own.
    Independent,
    /// All packages in the group share one version.
    Fixed,
    /// Packages move together at the same bump level.
    Linked,
}

/// Pre-1.0 bump remapping (§10.2): `{ breaking = "minor", feat = "patch" }`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct PreMajorCfg {
    /// Bump applied to breaking commits before 1.0 (default `minor`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breaking: Option<BumpLevel>,
    /// Bump applied to `feat` commits before 1.0 (default `patch`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feat: Option<BumpLevel>,
}

/// Bump magnitude accepted by configuration (subset of `Bump`; `none`
/// disables a mapping).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum BumpLevel {
    /// No bump.
    None,
    /// Patch bump.
    Patch,
    /// Minor bump.
    Minor,
    /// Major bump.
    Major,
}

/// Cascade policy (§10.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum CascadePolicy {
    /// Cascade to normal deps of publishable crates whenever anything
    /// upstream bumps.
    Always,
    /// Cascade only when the dependent's requirement no longer admits the
    /// new version (default for libraries).
    WhenUnsatisfied,
    /// Never cascade automatically.
    Never,
}

/// `[commits]` section.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct CommitsCfg {
    /// Commit type → bump mapping (`feat` → `minor`, …); unmapped types
    /// follow `bump_unknown` semantics (§10.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub types: Option<BTreeMap<String, BumpLevel>>,
    /// How merge commits are diffed (§8.5). Currently only
    /// `first-parent` (the default) is defined.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_policy: Option<MergePolicy>,
    /// Options for packages with no release base yet (§8.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_release: Option<InitialReleaseCfg>,
}

/// Merge-commit diffing policy (§8.5). Only the documented default exists in
/// v1; further values require an ADR.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum MergePolicy {
    /// Diff each commit against its first parent only; merge commits are
    /// never counted as changes themselves.
    FirstParent,
}

/// `initial_release` options (§8.3 complexity bound).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct InitialReleaseCfg {
    /// Maximum history to walk for a package with no tag (default 10 000).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_commits: Option<u32>,
}

/// `[changelog]` section.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct ChangelogCfg {
    /// Built-in style name (`keep-a-changelog`, `conventional`, `compact`,
    /// `github-release-notes`) or a path to a minijinja template (§9.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    /// Commit types hidden from notes (default: `chore`, `docs`, `ci`,
    /// `test`, `style`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden_types: Option<Vec<String>>,
}

/// `[publish]` section (§13). Publishing is opt-in.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct PublishCfg {
    /// Master switch; default `false` (safe-by-default, invariant I7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Cargo publishing options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cargo: Option<CargoPublishCfg>,
    /// npm publishing options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npm: Option<NpmPublishCfg>,
}

/// `publish.cargo` options.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct CargoPublishCfg {
    /// Registry name; default `crates.io`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,
    /// How long to wait for the index to reflect a publish before releasing
    /// dependents, e.g. `"90s"` (§13 visibility wait).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_for_index: Option<String>,
}

/// `publish.npm` options.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct NpmPublishCfg {
    /// npm access level (`public` | `restricted`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access: Option<NpmAccess>,
    /// Emit npm provenance attestations (§13.6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<bool>,
}

/// npm package access level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum NpmAccess {
    /// Public package.
    Public,
    /// Restricted package.
    Restricted,
}

/// `[forge]` section.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct ForgeCfg {
    /// Forge adapter (invariant I9: an adapter, not an assumption).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ForgeKind>,
    /// How release-branch commits are made (§11.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_mode: Option<CommitMode>,
}

/// Supported forge adapters in the v1 line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum ForgeKind {
    /// GitHub REST adapter (`M4.1`).
    Github,
    /// GitLab adapter (Year 2, `M7`).
    Gitlab,
    /// Gitea/Forgejo adapter (Year 2, `M7`).
    Gitea,
}

/// Release-branch commit mode (§11.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum CommitMode {
    /// Push commits with git (`--force-with-lease` CAS).
    Git,
    /// Create commits through the forge API (signed/verified commits).
    Api,
    /// `api` when an App token is present, `git` otherwise (default).
    Auto,
}

/// `[[package]]` per-package override (§16.4).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct PackageOverride {
    /// Package name this override applies to (match by name; a trailing `*`
    /// or `**` path glob is matched against the package root).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Whether the package is released at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release: Option<bool>,
    /// Files that count as "changes to this package" (§5.3 include filter).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
    /// Changed paths excluded from package attribution (§8.4).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
}

/// `[[link]]` explicit polyglot link (§6.5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct LinkCfg {
    /// Source package name (e.g. a Rust crate).
    pub from: String,
    /// Target package name (e.g. an npm package).
    pub to: String,
    /// Link semantics: `cascade` (default) or `lockstep`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<LinkMode>,
}

/// Polyglot link semantics (§6.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum LinkMode {
    /// Bump the dependent by at least `patch` when the source bumps.
    Cascade,
    /// Source and target always carry the same version.
    Lockstep,
}

/// JSON Schema document (`draft 2020-12`) for the `halyard.toml` v1 format,
/// generated from these types. Used by `xtask schema-check` to keep
/// `schemas/halyard-config-v1.json` in lockstep (M0.7 drift test).
#[cfg(feature = "schema-gen")]
pub fn schema_document() -> Result<serde_json::Value, String> {
    const ID: &str = "https://halyard.dev/schemas/halyard-config-v1.json";
    const NOTE: &str = "Generated by `cargo run -p xtask -- schema-check --write` from the \
                        halyard-config types. Do not edit by hand; an ADR is required for \
                        format changes.";
    let mut generator =
        schemars::SchemaGenerator::new(schemars::generate::SchemaSettings::draft2020_12());
    let schema = generator.root_schema_for::<HalyardConfig>();
    let mut value = serde_json::to_value(&schema)
        .map_err(|e| format!("config schema serialization failed: {e}"))?;
    let obj = value
        .as_object_mut()
        .ok_or_else(|| "config root schema is not a JSON object".to_owned())?;
    obj.insert("$id".to_owned(), serde_json::Value::String(ID.to_owned()));
    obj.insert(
        "$comment".to_owned(),
        serde_json::Value::String(NOTE.to_owned()),
    );
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_table_parses_to_all_defaults() {
        let cfg: HalyardConfig = toml::from_str("").expect("empty table parses");
        assert_eq!(cfg, HalyardConfig::default());
    }

    #[test]
    fn documented_example_roundtrips() {
        let src = r#"
[repo]
base_branch = "main"
tag_style = "auto"

[versioning]
policy = "independent"
cascade = "when-unsatisfied"

[versioning.pre_major]
breaking = "minor"
feat = "patch"

[commits]
merge_policy = "first-parent"

[commits.types]
feat = "minor"
fix = "patch"

[changelog]
style = "keep-a-changelog"
hidden_types = ["chore", "docs"]

[publish]
enabled = false

[publish.cargo]
registry = "crates.io"
wait_for_index = "90s"

[publish.npm]
access = "public"
provenance = true

[forge]
kind = "github"
commit_mode = "auto"

[[package]]
name = "internal-tool"
release = false
include = ["src/**", "Cargo.toml"]
exclude = ["**/*.md"]

[[link]]
from = "my-crate"
to = "@scope/my-crate-node"
mode = "lockstep"
"#;
        let cfg: HalyardConfig = toml::from_str(src).expect("documented example parses");
        let repo = cfg.repo.as_ref().expect("repo section");
        assert_eq!(repo.base_branch.as_deref(), Some("main"));
        assert_eq!(
            cfg.versioning.as_ref().and_then(|v| v.cascade),
            Some(CascadePolicy::WhenUnsatisfied)
        );
        assert_eq!(cfg.package.len(), 1);
        assert_eq!(cfg.link.len(), 1);
        let out = toml::to_string(&cfg).expect("config serializes");
        let reparsed: HalyardConfig = toml::from_str(&out).expect("serialized config re-parses");
        assert_eq!(reparsed, cfg);
    }
}
