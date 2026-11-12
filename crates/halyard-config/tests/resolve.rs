//! Layered resolution tests (M1.8): defaults < file < package metadata <
//! env `HALYARD_*` < CLI (§16.4), `config_hash` stability and the
//! tag-style grammar gate.

use std::collections::BTreeMap;
use std::error::Error;

use halyard_config::model::{BumpLevel, HalyardConfig};
use halyard_config::{
    ConfigError, Layer, ResolvedConfig, TagStyleSetting, config_hash, env_layer_from, resolve,
};

/// A config text with one setting per section, parsed through the loader.
fn file_config() -> Result<HalyardConfig, Box<dyn Error>> {
    let src = r#"
[repo]
base_branch = "develop"

[versioning]
cascade = "always"

[commits.types]
docs = "patch"
"#;
    Ok(halyard_config::load_from_str(src)?)
}

#[test]
fn defaults_match_section_16_4() -> Result<(), Box<dyn Error>> {
    let resolved = resolve(&[])?;
    assert_eq!(resolved.base_branch, None, "base branch defaults to auto");
    assert_eq!(resolved.tag_style, TagStyleSetting::Auto);
    assert_eq!(
        resolved.commit_types.get("feat").copied(),
        Some(BumpLevel::Minor)
    );
    assert_eq!(
        resolved.commit_types.get("fix").copied(),
        Some(BumpLevel::Patch)
    );
    assert_eq!(
        resolved.commit_types.get("perf").copied(),
        Some(BumpLevel::Patch)
    );
    assert_eq!(resolved.initial_max_commits, 10_000);
    assert!(!resolved.publish_enabled, "publish is opt-in (I7)");
    assert_eq!(resolved.publish_cargo_registry, "crates.io");
    assert!(resolved.publish_npm_provenance, "provenance defaults on");
    assert_eq!(resolved.forge_commit_mode, halyard_config::CommitMode::Auto);
    assert_eq!(
        resolved.changelog_hidden_types,
        vec![
            "chore".to_owned(),
            "docs".to_owned(),
            "ci".to_owned(),
            "test".to_owned(),
            "style".to_owned()
        ]
    );
    Ok(())
}

#[test]
fn layers_override_low_to_high() -> Result<(), Box<dyn Error>> {
    let file = file_config()?;
    let cli = HalyardConfig {
        repo: Some(halyard_config::model::RepoCfg {
            base_branch: Some("release".to_owned()),
            tag_style: None,
        }),
        ..HalyardConfig::default()
    };

    let resolved = resolve(&[Layer::Defaults, Layer::File(file.clone()), Layer::Cli(cli)])?;
    assert_eq!(
        resolved.base_branch.as_deref(),
        Some("release"),
        "CLI overrides file"
    );
    assert_eq!(
        resolved.cascade,
        halyard_config::CascadePolicy::Always,
        "file survives when CLI does not touch it"
    );
    assert_eq!(
        resolved.commit_types.get("docs").copied(),
        Some(BumpLevel::Patch),
        "custom commit types merge over the built-ins"
    );
    assert_eq!(
        resolved.commit_types.get("feat").copied(),
        Some(BumpLevel::Minor),
        "built-ins survive the merge"
    );
    Ok(())
}

#[test]
fn env_layer_maps_documented_variables() -> Result<(), Box<dyn Error>> {
    let env = env_layer_from(&|var| match var {
        "HALYARD_BASE_BRANCH" => Some("hotfix".to_owned()),
        "HALYARD_TAG_STYLE" => Some("v{version}".to_owned()),
        "HALYARD_CASCADE" => Some("never".to_owned()),
        _ => None,
    })?;
    let resolved = resolve(&[Layer::Defaults, Layer::Env(env)])?;
    assert_eq!(resolved.base_branch.as_deref(), Some("hotfix"));
    assert_eq!(
        resolved.tag_style,
        TagStyleSetting::Template(halyard_core::TagTemplate::parse("v{version}")?)
    );
    assert_eq!(resolved.cascade, halyard_config::CascadePolicy::Never);
    Ok(())
}

#[test]
fn env_layer_rejects_invalid_values() {
    let err = env_layer_from(&|var| match var {
        "HALYARD_CASCADE" => Some("sometimes".to_owned()),
        _ => None,
    })
    .err()
    .unwrap_or_else(|| unreachable!("HALYARD_CASCADE=sometimes must fail"));
    assert_eq!(err.code(), "E-CFG-ENV");
    assert_eq!(err.code(), ConfigError::ENV_CODE);
}

#[test]
fn invalid_tag_style_is_rejected_at_resolve() {
    let cfg = HalyardConfig {
        repo: Some(halyard_config::model::RepoCfg {
            base_branch: None,
            tag_style: Some(halyard_config::model::TagStyle("v{nope}".to_owned())),
        }),
        ..HalyardConfig::default()
    };
    let err = resolve(&[Layer::Defaults, Layer::File(cfg)])
        .err()
        .unwrap_or_else(|| unreachable!("invalid tag style must fail"));
    assert_eq!(err.code(), "E-CFG-TAG-STYLE");
}

#[test]
fn config_hash_is_stable_and_input_sensitive() -> Result<(), Box<dyn Error>> {
    let file = file_config()?;
    let first = config_hash(&resolve(&[Layer::Defaults, Layer::File(file.clone())])?);
    let second = config_hash(&resolve(&[Layer::Defaults, Layer::File(file)])?);
    assert_eq!(first, second, "I3: byte-identical re-runs");
    assert!(first.starts_with("blake3:"));
    assert_eq!(first.len(), 7 + 64);

    let other = config_hash(&resolve(&[Layer::Defaults])?);
    assert_ne!(first, other, "different config ⇒ different hash");
    Ok(())
}

#[test]
fn package_tables_concatenate_low_to_high() -> Result<(), Box<dyn Error>> {
    let low = HalyardConfig {
        package: vec![halyard_config::model::PackageOverride {
            name: Some("a".to_owned()),
            release: Some(false),
            include: Vec::new(),
            exclude: Vec::new(),
        }],
        ..HalyardConfig::default()
    };
    let high = HalyardConfig {
        package: vec![halyard_config::model::PackageOverride {
            name: Some("b".to_owned()),
            release: Some(true),
            include: Vec::new(),
            exclude: Vec::new(),
        }],
        ..HalyardConfig::default()
    };
    let resolved = resolve(&[
        Layer::Defaults,
        Layer::File(low),
        Layer::PackageMetadata(high),
    ])?;
    let names: Vec<String> = resolved
        .packages
        .iter()
        .filter_map(|p| p.name.clone())
        .collect();
    assert_eq!(names, vec!["a".to_owned(), "b".to_owned()]);
    Ok(())
}

#[test]
fn resolved_config_snapshot() -> Result<(), Box<dyn Error>> {
    let resolved = resolve(&[Layer::Defaults, Layer::File(file_config()?)])?;
    let _: BTreeMap<String, BumpLevel> = resolved.commit_types.clone();
    insta::assert_debug_snapshot!("resolved_config_shape", resolved.commit_types);
    Ok(())
}

#[test]
fn resolved_types_are_exported() -> Result<(), Box<dyn Error>> {
    // Type-level guard: the resolved surface is public API for the CLI.
    let resolved = ResolvedConfig::default_values()?;
    assert_eq!(resolved.forge_kind, halyard_config::ForgeKind::Github);
    Ok(())
}
