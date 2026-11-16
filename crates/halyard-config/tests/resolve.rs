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

// ---------------------------------------------------------------------
// Prerelease channel (§10.3, M5.6 step 1): `[versioning].channel` is
// validated at resolve time, merges low→high, feeds the plan config and
// the cache hash.

fn versioning_with_channel(channel: Option<&str>) -> HalyardConfig {
    HalyardConfig {
        versioning: Some(halyard_config::model::VersioningCfg {
            policy: None,
            pre_major: None,
            cascade: None,
            channel: channel.map(str::to_owned),
            channel_branches: None,
        }),
        ..HalyardConfig::default()
    }
}

#[test]
fn channel_resolves_into_the_plan_config() -> Result<(), Box<dyn Error>> {
    let resolved = resolve(&[
        Layer::Defaults,
        Layer::File(versioning_with_channel(Some("rc"))),
    ])?;
    let channel = resolved
        .prerelease_channel
        .as_ref()
        .unwrap_or_else(|| unreachable!("channel resolves"));
    assert_eq!(channel.as_str(), "rc");
    let plan_config = resolved.plan_config();
    assert!(plan_config.prerelease_channel.is_some());
    // Absence stays absence: the default plan config has no channel.
    let plain = resolve(&[])?;
    assert!(plain.prerelease_channel.is_none());
    assert!(plain.plan_config().prerelease_channel.is_none());
    Ok(())
}

#[test]
fn invalid_channel_value_is_rejected_at_resolve() {
    let err = resolve(&[
        Layer::Defaults,
        Layer::File(versioning_with_channel(Some("rc."))),
    ])
    .err()
    .unwrap_or_else(|| unreachable!("invalid channel must fail"));
    assert_eq!(err.code(), "E-CFG-CHANNEL");
    assert_eq!(err.code(), ConfigError::CHANNEL_CODE);
}

#[test]
fn channel_merges_low_to_high() -> Result<(), Box<dyn Error>> {
    let resolved = resolve(&[
        Layer::Defaults,
        Layer::File(versioning_with_channel(Some("beta"))),
        Layer::PackageMetadata(versioning_with_channel(Some("rc"))),
    ])?;
    assert_eq!(
        resolved
            .prerelease_channel
            .as_ref()
            .map(halyard_core::bump::PrereleaseChannel::as_str),
        Some("rc"),
        "the higher layer wins"
    );
    // A lower-layer channel survives a higher layer that does not set one.
    let survivor = resolve(&[
        Layer::Defaults,
        Layer::File(versioning_with_channel(Some("beta"))),
        Layer::PackageMetadata(HalyardConfig::default()),
    ])?;
    assert_eq!(
        survivor
            .prerelease_channel
            .as_ref()
            .map(halyard_core::bump::PrereleaseChannel::as_str),
        Some("beta")
    );
    Ok(())
}

#[test]
fn config_hash_includes_the_channel() -> Result<(), Box<dyn Error>> {
    let plain = config_hash(&resolve(&[])?);
    let with_channel = config_hash(&resolve(&[
        Layer::Defaults,
        Layer::File(versioning_with_channel(Some("rc"))),
    ])?);
    assert_ne!(
        plain, with_channel,
        "a channel change invalidates the cache key (§8.7)"
    );
    Ok(())
}

// ---------------------------------------------------------------------
// Branch-bound channels (§10.3 "next branch ⇒ rc", ADR-0025 step 2):
// `channel_branches` binds channels to branch names; the resolution is
// a pure function of the checked-out branch.

fn versioning_with_bindings(bindings: &[(&str, &str)]) -> HalyardConfig {
    HalyardConfig {
        versioning: Some(halyard_config::model::VersioningCfg {
            policy: None,
            pre_major: None,
            cascade: None,
            channel: None,
            channel_branches: Some(
                bindings
                    .iter()
                    .map(|(b, c)| ((*b).to_owned(), (*c).to_owned()))
                    .collect(),
            ),
        }),
        ..HalyardConfig::default()
    }
}

#[test]
fn branch_binding_resolves_and_validates() -> Result<(), Box<dyn Error>> {
    let resolved = resolve(&[
        Layer::Defaults,
        Layer::File(versioning_with_bindings(&[("next", "rc")])),
    ])?;
    let channel = resolved
        .channel_for_branch(Some("next"))
        .unwrap_or_else(|| unreachable!("the branch binding resolves"));
    assert_eq!(channel.as_str(), "rc");
    // Other branches have no binding.
    assert!(resolved.channel_for_branch(Some("main")).is_none());
    // A detached/unborn HEAD has none either.
    assert!(resolved.channel_for_branch(None).is_none());
    Ok(())
}

#[test]
fn branch_binding_wins_over_the_default_channel() -> Result<(), Box<dyn Error>> {
    let mut cfg = versioning_with_bindings(&[("next", "rc")]);
    if let Some(versioning) = &mut cfg.versioning {
        versioning.channel = Some("beta".to_owned());
    }
    let resolved = resolve(&[Layer::Defaults, Layer::File(cfg)])?;
    assert_eq!(
        resolved
            .channel_for_branch(Some("next"))
            .as_ref()
            .map(halyard_core::bump::PrereleaseChannel::as_str),
        Some("rc"),
        "the branch binding wins on its branch"
    );
    assert_eq!(
        resolved
            .channel_for_branch(Some("main"))
            .as_ref()
            .map(halyard_core::bump::PrereleaseChannel::as_str),
        Some("beta"),
        "the unbound default fills the rest"
    );
    Ok(())
}

#[test]
fn invalid_branch_binding_value_is_rejected_at_resolve() {
    let err = resolve(&[
        Layer::Defaults,
        Layer::File(versioning_with_bindings(&[("next", "rc.")])),
    ])
    .err()
    .unwrap_or_else(|| unreachable!("invalid bound channel must fail"));
    assert_eq!(err.code(), "E-CFG-CHANNEL");
    let rendered = err.to_string();
    assert!(
        rendered.contains("next"),
        "the failing branch is named: {rendered}"
    );
}

#[test]
fn branch_bindings_merge_per_key_low_to_high() -> Result<(), Box<dyn Error>> {
    let resolved = resolve(&[
        Layer::Defaults,
        Layer::File(versioning_with_bindings(&[
            ("next", "beta"),
            ("lts", "alpha"),
        ])),
        Layer::PackageMetadata(versioning_with_bindings(&[("next", "rc")])),
    ])?;
    assert_eq!(
        resolved
            .channel_for_branch(Some("next"))
            .as_ref()
            .map(halyard_core::bump::PrereleaseChannel::as_str),
        Some("rc"),
        "the higher layer's binding wins per key"
    );
    assert_eq!(
        resolved
            .channel_for_branch(Some("lts"))
            .as_ref()
            .map(halyard_core::bump::PrereleaseChannel::as_str),
        Some("alpha"),
        "untouched keys survive the merge"
    );
    Ok(())
}
