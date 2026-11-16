//! Layered resolution tests (M1.8): defaults < file < package metadata <
//! env `HALYARD_*` < CLI (§16.4), `config_hash` stability and the
//! tag-style grammar gate.

use std::collections::BTreeMap;
use std::error::Error;

use halyard_config::model::{BumpLevel, HalyardConfig, NpmAccess};
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
    assert_eq!(
        resolved.publish_npm_provenance, None,
        "provenance unset is semantic (ADR-0033)"
    );
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
            channel: None,
        }],
        ..HalyardConfig::default()
    };
    let high = HalyardConfig {
        package: vec![halyard_config::model::PackageOverride {
            name: Some("b".to_owned()),
            release: Some(true),
            include: Vec::new(),
            exclude: Vec::new(),
            channel: None,
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
            maintenance_branches: None,
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
            maintenance_branches: None,
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
// ---------------------------------------------------------------------
// Per-package channel overrides (§10.3, M5.6 step 3 / ADR-0026): a
// `[[package]] channel = "rc"` entry names that package's train; the
// map resolves and validates at resolve time like the repo-wide key.

/// A config with one `[[package]]` override.
fn package_override(name: Option<&str>, channel: Option<&str>) -> HalyardConfig {
    HalyardConfig {
        package: vec![halyard_config::model::PackageOverride {
            name: name.map(str::to_owned),
            release: None,
            include: Vec::new(),
            exclude: Vec::new(),
            channel: channel.map(str::to_owned),
        }],
        ..HalyardConfig::default()
    }
}

#[test]
fn package_override_channel_resolves_into_the_plan_config() -> Result<(), Box<dyn Error>> {
    let resolved = resolve(&[
        Layer::Defaults,
        Layer::File(package_override(Some("app"), Some("rc"))),
    ])?;
    let channel = resolved
        .package_channels
        .get("app")
        .unwrap_or_else(|| unreachable!("the override resolves"));
    assert_eq!(channel.as_str(), "rc");
    // The map rides `plan_config` (validated at resolve, total there).
    assert!(
        resolved.plan_config().package_channels.contains_key("app"),
        "the override reaches the planner"
    );
    // The default carries no overrides.
    let plain = resolve(&[])?;
    assert!(plain.package_channels.is_empty());
    assert!(plain.plan_config().package_channels.is_empty());
    Ok(())
}

#[test]
fn package_override_channel_requires_a_name() {
    // A channel on a name-less (path-glob) override cannot be applied:
    // fail closed instead of guessing the target.
    let err = resolve(&[
        Layer::Defaults,
        Layer::File(package_override(None, Some("rc"))),
    ])
    .err()
    .unwrap_or_else(|| unreachable!("a name-less override must fail"));
    assert_eq!(err.code(), "E-CFG-CHANNEL");
    let rendered = err.to_string();
    assert!(
        rendered.contains("name"),
        "the missing name is named as the cause: {rendered}"
    );
}

#[test]
fn invalid_package_override_channel_is_rejected_at_resolve() {
    let err = resolve(&[
        Layer::Defaults,
        Layer::File(package_override(Some("app"), Some("rc."))),
    ])
    .err()
    .unwrap_or_else(|| unreachable!("invalid override channel must fail"));
    assert_eq!(err.code(), "E-CFG-CHANNEL");
    let rendered = err.to_string();
    assert!(
        rendered.contains("app"),
        "the failing package is named: {rendered}"
    );
}

#[test]
fn package_override_channels_merge_low_to_high() -> Result<(), Box<dyn Error>> {
    // `[[package]]` tables concatenate low→high: later entries win when
    // both match a package (the documented merge rule).
    let mut higher = package_override(Some("app"), Some("rc"));
    higher.package.push(halyard_config::model::PackageOverride {
        name: Some("lib".to_owned()),
        release: None,
        include: Vec::new(),
        exclude: Vec::new(),
        channel: Some("beta".to_owned()),
    });
    let resolved = resolve(&[
        Layer::Defaults,
        Layer::File(package_override(Some("app"), Some("beta"))),
        Layer::PackageMetadata(higher),
    ])?;
    let channels: std::collections::BTreeMap<_, _> = resolved
        .package_channels
        .iter()
        .map(|(name, channel)| (name.as_str(), channel.as_str()))
        .collect();
    assert_eq!(
        channels.get("app").copied(),
        Some("rc"),
        "the higher layer's entry wins"
    );
    assert_eq!(channels.get("lib").copied(), Some("beta"));
    Ok(())
}

#[test]
fn config_hash_includes_the_package_override_channel() -> Result<(), Box<dyn Error>> {
    let plain = config_hash(&resolve(&[])?);
    let overridden = config_hash(&resolve(&[
        Layer::Defaults,
        Layer::File(package_override(Some("app"), Some("rc"))),
    ])?);
    assert_ne!(
        plain, overridden,
        "an override change invalidates the cache key (§8.7)"
    );
    Ok(())
}

#[test]
fn package_override_wins_over_the_branch_bound_channel() -> Result<(), Box<dyn Error>> {
    // Precedence (ADR-0026): the per-package override is the most
    // explicit policy — it beats the branch binding the way it beats
    // the unbound default; the planner resolves the effective channel
    // per package.
    let mut cfg = versioning_with_bindings(&[("next", "rc")]);
    cfg.package = package_override(Some("app"), Some("beta")).package;
    let resolved = resolve(&[Layer::Defaults, Layer::File(cfg)])?;
    assert_eq!(
        resolved
            .channel_for_branch(Some("next"))
            .as_ref()
            .map(halyard_core::bump::PrereleaseChannel::as_str),
        Some("rc"),
        "the branch binding resolves the default"
    );
    let plan_config = resolved.plan_config();
    assert_eq!(
        plan_config
            .package_channels
            .get("app")
            .map(halyard_core::bump::PrereleaseChannel::as_str),
        Some("beta"),
        "the override rides the plan config next to the default"
    );
    Ok(())
}

// ---------------------------------------------------------------------
// Maintenance-line release trains (§10.3, M5.6 / ADR-0027):
// `[versioning].maintenance_branches = ["1.x", "1.2.x"]` — each entry
// names the branch AND the line it pins; validated at resolve time.

fn versioning_with_maintenance(entries: &[&str]) -> HalyardConfig {
    HalyardConfig {
        versioning: Some(halyard_config::model::VersioningCfg {
            policy: None,
            pre_major: None,
            cascade: None,
            channel: None,
            channel_branches: None,
            maintenance_branches: Some(entries.iter().map(|entry| (*entry).to_owned()).collect()),
        }),
        ..HalyardConfig::default()
    }
}

#[test]
fn maintenance_branches_resolve_into_lines() -> Result<(), Box<dyn Error>> {
    let resolved = resolve(&[
        Layer::Defaults,
        Layer::File(versioning_with_maintenance(&["1.x", "1.2.x"])),
    ])?;
    assert_eq!(
        resolved
            .maintenance_line_for(Some("1.2.x"))
            .map(|line| line.to_string()),
        Some("1.2.x".to_owned()),
        "the pinned minor wins over the bare major line"
    );
    assert_eq!(
        resolved
            .maintenance_line_for(Some("1.x"))
            .map(|line| line.to_string()),
        Some("1.x".to_owned())
    );
    assert!(
        resolved.maintenance_line_for(Some("main")).is_none(),
        "an unlisted branch plans free"
    );
    assert!(
        resolved.maintenance_line_for(None).is_none(),
        "a detached HEAD plans free"
    );
    Ok(())
}

#[test]
fn invalid_maintenance_branch_entry_is_rejected_at_resolve() {
    let err = resolve(&[
        Layer::Defaults,
        Layer::File(versioning_with_maintenance(&["1.y"])),
    ])
    .err()
    .unwrap_or_else(|| unreachable!("an invalid line must fail"));
    assert_eq!(err.code(), "E-CFG-MAINTENANCE");
    let rendered = err.to_string();
    assert!(
        rendered.contains("1.y"),
        "the offending entry is named: {rendered}"
    );
}

#[test]
fn maintenance_branches_ride_the_config_hash() -> Result<(), Box<dyn Error>> {
    let plain = config_hash(&resolve(&[])?);
    let pinned = config_hash(&resolve(&[
        Layer::Defaults,
        Layer::File(versioning_with_maintenance(&["1.x"])),
    ])?);
    assert_ne!(
        plain, pinned,
        "a maintenance pin invalidates the cache key (§8.7)"
    );
    Ok(())
}

#[test]
fn maintenance_branches_merge_low_to_high() -> Result<(), Box<dyn Error>> {
    // Per-key overlay like `channel_branches`: untouched branches keep
    // the lower layer's line.
    let resolved = resolve(&[
        Layer::Defaults,
        Layer::File(versioning_with_maintenance(&["1.x"])),
        Layer::PackageMetadata(versioning_with_maintenance(&["1.2.x"])),
    ])?;
    assert_eq!(
        resolved
            .maintenance_line_for(Some("1.x"))
            .map(|line| line.to_string()),
        Some("1.x".to_owned()),
        "untouched branches survive the merge"
    );
    assert_eq!(
        resolved
            .maintenance_line_for(Some("1.2.x"))
            .map(|line| line.to_string()),
        Some("1.2.x".to_owned())
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// The npm publish options (ADR-0033): `unset` is semantic — the
// spawned tool's own trusted-publishing posture governs (`--provenance`
// is only passed on an explicit `true`, `--no-provenance` only on an
// explicit `false`), so the resolved config carries the `Option` states
// through, never a guessed default. Written first (red), implemented
// second.
// ---------------------------------------------------------------------------

fn publish_npm_config(access: Option<&str>, provenance: Option<bool>) -> HalyardConfig {
    use halyard_config::model::{NpmPublishCfg, PublishCfg};
    let level = match access {
        Some("restricted") => Some(NpmAccess::Restricted),
        Some(_) => Some(NpmAccess::Public),
        None => None,
    };
    let npm = NpmPublishCfg {
        access: level,
        provenance,
    };
    let publish = PublishCfg {
        enabled: None,
        cargo: None,
        npm: Some(npm),
    };
    HalyardConfig {
        publish: Some(publish),
        ..HalyardConfig::default()
    }
}

#[test]
fn unset_npm_publish_options_stay_unset() -> Result<(), Box<dyn Error>> {
    // No `[publish.npm]` at all: both options resolve `None` — the run
    // must not silently invent `--access`/`--provenance` flags the user
    // never asked for.
    let resolved = resolve(&[Layer::Defaults])?;
    assert_eq!(resolved.publish_npm_access, None);
    assert_eq!(resolved.publish_npm_provenance, None);
    Ok(())
}

#[test]
fn explicit_npm_publish_options_resolve_verbatim() -> Result<(), Box<dyn Error>> {
    let resolved = resolve(&[
        Layer::Defaults,
        Layer::File(publish_npm_config(Some("public"), Some(true))),
    ])?;
    assert_eq!(resolved.publish_npm_access, Some(NpmAccess::Public));
    assert_eq!(resolved.publish_npm_provenance, Some(true));
    Ok(())
}

#[test]
fn npm_provenance_opt_out_resolves_false() -> Result<(), Box<dyn Error>> {
    // `provenance = false` is the documented self-hosted escape hatch
    // (`--no-provenance`); it must survive as an explicit `Some(false)`,
    // not collapse into the unset state.
    let resolved = resolve(&[
        Layer::Defaults,
        Layer::File(publish_npm_config(Some("restricted"), Some(false))),
    ])?;
    assert_eq!(resolved.publish_npm_access, Some(NpmAccess::Restricted));
    assert_eq!(resolved.publish_npm_provenance, Some(false));
    Ok(())
}

#[test]
fn npm_publish_options_layer_high_over_low() -> Result<(), Box<dyn Error>> {
    let resolved = resolve(&[
        Layer::Defaults,
        Layer::File(publish_npm_config(Some("public"), Some(true))),
        Layer::PackageMetadata(publish_npm_config(Some("restricted"), Some(false))),
    ])?;
    assert_eq!(resolved.publish_npm_access, Some(NpmAccess::Restricted));
    assert_eq!(resolved.publish_npm_provenance, Some(false));
    Ok(())
}
