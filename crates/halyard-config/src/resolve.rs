//! Layered resolution (§16.4): built-in defaults < `halyard.toml` <
//! `package.metadata.halyard` / `package.json#halyard` < env `HALYARD_*`
//! < CLI flags, plus the `config_hash` that feeds the cache key (§8.7).
//!
//! Merging is deterministic: scalar sections override per-key from the
//! higher layer; `[[package]]`/`[[link]]` tables concatenate low→high
//! (later entries win when both match a package); `[commits.types]`
//! entries overlay the built-in mapping.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::error::ConfigError;
use crate::model::{
    BumpLevel, CascadePolicy, ChangelogCfg, CommitMode, ForgeCfg, ForgeKind, HalyardConfig,
    MergePolicy, NpmAccess, PackageOverride, PublishCfg, RepoCfg, VersionPolicy,
};
use halyard_core::TagTemplate;

/// One resolution layer, ordered low → high.
#[derive(Clone, Debug)]
pub enum Layer {
    /// Built-in defaults (§16.4). Always the bottom layer; passing it is
    /// optional and idempotent.
    Defaults,
    /// A `halyard.toml` (repo root or `.config/halyard.toml`).
    File(HalyardConfig),
    /// `package.metadata.halyard` / `package.json#halyard`.
    PackageMetadata(HalyardConfig),
    /// `HALYARD_*` environment variables, materialized through
    /// [`env_layer`] (tests inject via [`env_layer_from`]).
    Env(HalyardConfig),
    /// CLI flags mapped into config values (M1.10).
    Cli(HalyardConfig),
}

/// Resolved tag style: `auto` (single-package `v{version}`, monorepo
/// `{name}-v{version}`) or a validated template (appendix A.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum TagStyleSetting {
    /// Auto-detect per workspace shape.
    Auto,
    /// An explicit template.
    Template(TagTemplate),
}

/// The fully resolved configuration (every default materialized; no
/// `Option` wrapping except where "unset" is semantic, e.g.
/// `base_branch = auto`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResolvedConfig {
    /// Base branch; `None` = auto (the remote HEAD).
    pub base_branch: Option<String>,
    /// Tag naming.
    pub tag_style: TagStyleSetting,
    /// Workspace versioning policy (§5.4).
    pub policy: VersionPolicy,
    /// Pre-1.0 breaking remap (§10.2).
    pub pre_major_breaking: BumpLevel,
    /// Pre-1.0 feat remap (§10.2).
    pub pre_major_feat: BumpLevel,
    /// Cascade policy (§10.4).
    pub cascade: CascadePolicy,
    /// Commit type → bump map (built-ins overlaid by config).
    pub commit_types: BTreeMap<String, BumpLevel>,
    /// Merge-commit diffing policy (§8.5).
    pub merge_policy: MergePolicy,
    /// Walk bound for packages with no release base (§8.3).
    pub initial_max_commits: u32,
    /// Changelog style name or template path (§9.4).
    pub changelog_style: Option<String>,
    /// Commit types hidden from notes.
    pub changelog_hidden_types: Vec<String>,
    /// Master publish switch (opt-in, invariant I7).
    pub publish_enabled: bool,
    /// Cargo registry (§13).
    pub publish_cargo_registry: String,
    /// crates.io index visibility wait (§13).
    pub publish_cargo_wait_for_index: String,
    /// npm access level.
    pub publish_npm_access: NpmAccess,
    /// npm provenance attestations (§13.6).
    pub publish_npm_provenance: bool,
    /// Forge adapter (invariant I9).
    pub forge_kind: ForgeKind,
    /// Release-branch commit mode (§11.5).
    pub forge_commit_mode: CommitMode,
    /// Per-package overrides, low→high.
    pub packages: Vec<PackageOverride>,
    /// Explicit polyglot links (§6.5), low→high.
    pub links: Vec<crate::model::LinkCfg>,
}

impl ResolvedConfig {
    /// The built-in defaults as a `ResolvedConfig` (§16.4).
    pub fn default_values() -> Result<Self, ConfigError> {
        resolve(&[Layer::Defaults])
    }
}

/// The built-in `[commits.types]` mapping (§10.1).
fn default_commit_types() -> BTreeMap<String, BumpLevel> {
    let mut map = BTreeMap::new();
    map.insert("feat".to_owned(), BumpLevel::Minor);
    map.insert("fix".to_owned(), BumpLevel::Patch);
    map.insert("perf".to_owned(), BumpLevel::Patch);
    map
}

/// The built-in hidden changelog types (§16.4).
fn default_hidden_types() -> Vec<String> {
    ["chore", "docs", "ci", "test", "style"]
        .iter()
        .map(|t| (*t).to_owned())
        .collect()
}

/// Resolves the layers low → high into a [`ResolvedConfig`].
pub fn resolve(layers: &[Layer]) -> Result<ResolvedConfig, ConfigError> {
    let mut effective = HalyardConfig::default();
    for layer in layers {
        let config = match layer {
            Layer::Defaults => HalyardConfig::default(),
            Layer::File(config)
            | Layer::PackageMetadata(config)
            | Layer::Cli(config)
            | Layer::Env(config) => config.clone(),
        };
        merge_into(&mut effective, config);
    }
    materialize(effective)
}

/// Merges `higher` on top of `lower` (in place, §16.4 layering).
fn merge_into(lower: &mut HalyardConfig, higher: HalyardConfig) {
    merge_repo(lower, higher.repo);
    merge_versioning(lower, higher.versioning);
    merge_commits(lower, higher.commits);
    merge_changelog(lower, higher.changelog);
    merge_publish(lower, higher.publish);
    merge_forge(lower, higher.forge.as_ref());
    // Tables concatenate low → high.
    lower.package.extend(higher.package);
    lower.link.extend(higher.link);
}

/// Merges the `[repo]` section.
fn merge_repo(lower: &mut HalyardConfig, repo: Option<crate::model::RepoCfg>) {
    let Some(RepoCfg {
        base_branch,
        tag_style,
    }) = repo
    else {
        return;
    };
    if base_branch.is_none() && tag_style.is_none() {
        return;
    }
    let slot = lower.repo.get_or_insert_with(RepoCfg::default);
    if base_branch.is_some() {
        slot.base_branch = base_branch;
    }
    if tag_style.is_some() {
        slot.tag_style = tag_style;
    }
}

/// Merges the `[versioning]` section (incl. nested `pre_major`).
fn merge_versioning(lower: &mut HalyardConfig, versioning: Option<crate::model::VersioningCfg>) {
    let Some(higher) = versioning else {
        return;
    };
    let slot = lower.versioning.get_or_insert_with(Default::default);
    if higher.policy.is_some() {
        slot.policy = higher.policy;
    }
    if higher.cascade.is_some() {
        slot.cascade = higher.cascade;
    }
    let pm = higher.pre_major.unwrap_or_default();
    if pm.breaking.is_none() && pm.feat.is_none() {
        return;
    }
    let pre_major = slot.pre_major.get_or_insert_with(Default::default);
    if pm.breaking.is_some() {
        pre_major.breaking = pm.breaking;
    }
    if pm.feat.is_some() {
        pre_major.feat = pm.feat;
    }
}

/// Merges the `[commits]` section (`types` overlay the existing map).
fn merge_commits(lower: &mut HalyardConfig, commits: Option<crate::model::CommitsCfg>) {
    let Some(higher) = commits else {
        return;
    };
    let slot = lower.commits.get_or_insert_with(Default::default);
    if let Some(types) = higher.types {
        let merged = slot.types.get_or_insert_with(BTreeMap::new);
        for (kind, level) in types {
            merged.insert(kind, level);
        }
    }
    if higher.merge_policy.is_some() {
        slot.merge_policy = higher.merge_policy;
    }
    if let Some(initial) = higher.initial_release {
        let target = slot.initial_release.get_or_insert_with(Default::default);
        if initial.max_commits.is_some() {
            target.max_commits = initial.max_commits;
        }
    }
}

/// Merges the `[changelog]` section.
fn merge_changelog(lower: &mut HalyardConfig, changelog: Option<ChangelogCfg>) {
    let Some(ChangelogCfg {
        style,
        hidden_types,
    }) = changelog
    else {
        return;
    };
    let slot = lower.changelog.get_or_insert_with(Default::default);
    if style.is_some() {
        slot.style = style;
    }
    if hidden_types.is_some() {
        slot.hidden_types = hidden_types;
    }
}

/// Merges the `[publish]` section (incl. `cargo`/`npm` subsections).
fn merge_publish(lower: &mut HalyardConfig, publish: Option<PublishCfg>) {
    let Some(PublishCfg {
        enabled,
        cargo,
        npm,
    }) = publish
    else {
        return;
    };
    let slot = lower.publish.get_or_insert_with(Default::default);
    if enabled.is_some() {
        slot.enabled = enabled;
    }
    if let Some(cargo) = cargo {
        let target = slot.cargo.get_or_insert_with(Default::default);
        if cargo.registry.is_some() {
            target.registry = cargo.registry;
        }
        if cargo.wait_for_index.is_some() {
            target.wait_for_index = cargo.wait_for_index;
        }
    }
    if let Some(npm) = npm {
        let target = slot.npm.get_or_insert_with(Default::default);
        if npm.access.is_some() {
            target.access = npm.access;
        }
        if npm.provenance.is_some() {
            target.provenance = npm.provenance;
        }
    }
}

/// Merges the `[forge]` section.
fn merge_forge(lower: &mut HalyardConfig, forge: Option<&ForgeCfg>) {
    let Some(ForgeCfg { kind, commit_mode }) = forge else {
        return;
    };
    let slot = lower.forge.get_or_insert_with(Default::default);
    if kind.is_some() {
        slot.kind = *kind;
    }
    if commit_mode.is_some() {
        slot.commit_mode = *commit_mode;
    }
}

/// Materializes the effective config against the built-in defaults.
fn materialize(effective: HalyardConfig) -> Result<ResolvedConfig, ConfigError> {
    let repo = effective.repo.unwrap_or_default();
    let tag_style = match repo.tag_style.as_ref().map(|t| t.0.as_str()) {
        None | Some("auto") => TagStyleSetting::Auto,
        Some(template) => TagTemplate::parse(template)
            .map(TagStyleSetting::Template)
            .map_err(|error| ConfigError::TagStyle {
                value: template.to_owned(),
                reason: error.to_string(),
            })?,
    };
    let versioning = effective.versioning.unwrap_or_default();
    let pre_major = versioning.pre_major.unwrap_or_default();
    let commits = effective.commits.unwrap_or_default();
    let mut commit_types = default_commit_types();
    for (kind, level) in commits.types.unwrap_or_default() {
        commit_types.insert(kind, level);
    }
    let initial_max_commits = commits
        .initial_release
        .and_then(|i| i.max_commits)
        .unwrap_or(10_000);
    let changelog = effective.changelog.unwrap_or_default();
    let publish = effective.publish.unwrap_or_default();
    let cargo = publish.cargo.unwrap_or_default();
    let npm = publish.npm.unwrap_or_default();
    let forge = effective.forge.unwrap_or_default();

    Ok(ResolvedConfig {
        base_branch: repo.base_branch,
        tag_style,
        policy: versioning.policy.unwrap_or(VersionPolicy::Independent),
        pre_major_breaking: pre_major.breaking.unwrap_or(BumpLevel::Minor),
        pre_major_feat: pre_major.feat.unwrap_or(BumpLevel::Patch),
        cascade: versioning.cascade.unwrap_or(CascadePolicy::WhenUnsatisfied),
        commit_types,
        merge_policy: commits.merge_policy.unwrap_or(MergePolicy::FirstParent),
        initial_max_commits,
        changelog_style: changelog.style,
        changelog_hidden_types: changelog.hidden_types.unwrap_or_else(default_hidden_types),
        publish_enabled: publish.enabled.unwrap_or(false),
        publish_cargo_registry: cargo.registry.unwrap_or_else(|| "crates.io".to_owned()),
        publish_cargo_wait_for_index: cargo.wait_for_index.unwrap_or_else(|| "90s".to_owned()),
        publish_npm_access: npm.access.unwrap_or(NpmAccess::Public),
        publish_npm_provenance: npm.provenance.unwrap_or(true),
        forge_kind: forge.kind.unwrap_or(ForgeKind::Github),
        forge_commit_mode: forge.commit_mode.unwrap_or(CommitMode::Auto),
        packages: effective.package,
        links: effective.link,
    })
}

/// The documented `HALYARD_*` environment mapping (§16.4 layer 4):
/// `HALYARD_BASE_BRANCH`, `HALYARD_TAG_STYLE`, `HALYARD_CASCADE`,
/// `HALYARD_POLICY`. Unset variables are skipped.
pub fn env_layer() -> Result<HalyardConfig, ConfigError> {
    env_layer_from(&|var| std::env::var(var).ok())
}

/// [`env_layer`] with an injectable lookup (deterministic tests; no
/// process-environment access in tests).
pub fn env_layer_from(
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<HalyardConfig, ConfigError> {
    let mut config = HalyardConfig::default();
    let mut repo = RepoCfg::default();
    if let Some(value) = lookup("HALYARD_BASE_BRANCH") {
        repo.base_branch = Some(value);
    }
    if let Some(value) = lookup("HALYARD_TAG_STYLE") {
        repo.tag_style = Some(crate::model::TagStyle(value));
    }
    if repo.base_branch.is_some() || repo.tag_style.is_some() {
        config.repo = Some(repo);
    }
    if let Some(value) = lookup("HALYARD_CASCADE") {
        let cascade = match value.as_str() {
            "always" => CascadePolicy::Always,
            "when-unsatisfied" => CascadePolicy::WhenUnsatisfied,
            "never" => CascadePolicy::Never,
            other => {
                return Err(ConfigError::Env {
                    var: "HALYARD_CASCADE".to_owned(),
                    message: format!(
                        "{other:?} is not one of `always`, `when-unsatisfied`, `never`"
                    ),
                });
            }
        };
        let slot = config.versioning.get_or_insert_with(Default::default);
        slot.cascade = Some(cascade);
    }
    if let Some(value) = lookup("HALYARD_POLICY") {
        let policy = match value.as_str() {
            "independent" => VersionPolicy::Independent,
            "fixed" => VersionPolicy::Fixed,
            "linked" => VersionPolicy::Linked,
            other => {
                return Err(ConfigError::Env {
                    var: "HALYARD_POLICY".to_owned(),
                    message: format!("{other:?} is not one of `independent`, `fixed`, `linked`"),
                });
            }
        };
        let slot = config.versioning.get_or_insert_with(Default::default);
        slot.policy = Some(policy);
    }
    Ok(config)
}

/// blake3 over the canonical JSON of the resolved config (§8.7
/// `config_hash`; canonical form per [`halyard_core::canonical_json`]).
pub fn config_hash(resolved: &ResolvedConfig) -> String {
    let value = serde_json::to_value(resolved).unwrap_or(serde_json::Value::Null);
    format!(
        "blake3:{}",
        halyard_core::blake3_hex(halyard_core::canonical_json(&value).as_bytes())
    )
}
