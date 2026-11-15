//! The npm publish-registry resolution contract (M5.2 continued,
//! ADR-0021): where the §13 probe must look for one npm package, read
//! from the configuration the spawned `pnpm publish` itself reads —
//! the per-package `publishConfig.registry` (the documented
//! publish-time override), the `NPM_CONFIG_REGISTRY` environment
//! override, the `.npmrc` `registry`/`@scope:registry` keys, and the
//! documented default. The verbatim quotes live in
//! `docs/research/publish-tooling.md`; the fail-closed rules mirror
//! the cargo side (`halyard_eco_cargo::registry_config`).

use halyard_eco_npm::publish_registry::{
    PublishRegistryBase, resolve_publish_registry, scoped_env_name_matches,
    scoped_registry_env_variable,
};

type BoxError = Box<dyn std::error::Error>;

fn pkg_json(publish_config: Option<&str>) -> Vec<u8> {
    let publish = publish_config
        .map(|registry| format!("\"publishConfig\": {{\"registry\": \"{registry}\"}},"))
        .unwrap_or_default();
    format!(
        "{{\n  \"name\": \"p1\",\n  \"version\": \"0.1.0\",\n  {publish}\n  \"main\": \
         \"lib.js\"\n}}\n"
    )
    .into_bytes()
}

#[test]
fn no_configuration_resolves_the_documented_default() {
    assert_eq!(
        resolve_publish_registry("p1", Some(&pkg_json(None)), &[], None, None),
        PublishRegistryBase::Default
    );
}

#[test]
fn publish_config_registry_overrides_everything() {
    // The npm docs: "use the publishConfig dictionary described below
    // to override the registry config param at publish-time" — the
    // publish-specific, per-package override beats env and npmrc.
    assert_eq!(
        resolve_publish_registry(
            "p1",
            Some(&pkg_json(Some("https://corp.example/registry"))),
            &[(".npmrc", "registry=https://npmrc.example/")],
            Some("https://env.example/"),
            None,
        ),
        PublishRegistryBase::Custom("https://corp.example/registry".to_owned())
    );
}

#[test]
fn the_env_registry_overrides_the_npmrc_files() {
    // "Any environment variables that start with npm_config_ will be
    // interpreted as a configuration parameter" — env ranks above the
    // npmrc files.
    assert_eq!(
        resolve_publish_registry(
            "p1",
            Some(&pkg_json(None)),
            &[(".npmrc", "registry=https://npmrc.example/")],
            Some("https://env.example/"),
            None
        ),
        PublishRegistryBase::Custom("https://env.example".to_owned())
    );
}

#[test]
fn an_env_registry_set_but_empty_fails_closed() {
    // The house empty-value rule (the auth seam's token, the cargo
    // side's index override): present-but-empty is a doomed
    // resolution, refused with the variable named.
    let lookup = resolve_publish_registry("p1", Some(&pkg_json(None)), &[], Some(""), None);
    match lookup {
        PublishRegistryBase::Unsupported(message) => {
            assert!(
                message.contains("NPM_CONFIG_REGISTRY"),
                "the variable is named: {message}"
            );
        }
        other => unreachable!("empty env must be Unsupported, got {other:?}"),
    }
}

#[test]
fn the_project_npmrc_overrides_the_user_npmrc() {
    // npmrc files are passed in ascending precedence: the per-project
    // file wins over the per-user one (npm's config resolution).
    let user = "registry=https://user.example/";
    let project = "registry=https://project.example/";
    assert_eq!(
        resolve_publish_registry(
            "p1",
            Some(&pkg_json(None)),
            &[("user .npmrc", user), ("project .npmrc", project)],
            None,
            None
        ),
        PublishRegistryBase::Custom("https://project.example".to_owned())
    );
}

#[test]
fn a_scoped_package_resolves_the_scope_registry_key() {
    // npm config's scoped-registry grammar: `@scope:registry` governs
    // a `@scope/name` package, beating the plain `registry` key.
    let npmrc = "@myorg:registry=https://myorg.example/\nregistry=https://other.example/\n";
    assert_eq!(
        resolve_publish_registry(
            "@myorg/pkg",
            Some(&pkg_json(None)),
            &[(".npmrc", npmrc)],
            None,
            None
        ),
        PublishRegistryBase::Custom("https://myorg.example".to_owned())
    );
    // An unscoped package ignores the scoped key.
    assert_eq!(
        resolve_publish_registry(
            "pkg",
            Some(&pkg_json(None)),
            &[(".npmrc", npmrc)],
            None,
            None
        ),
        PublishRegistryBase::Custom("https://other.example".to_owned())
    );
}

#[test]
fn an_npmrc_registry_set_but_empty_is_a_named_gap() {
    let npmrc = "registry=\n";
    let lookup = resolve_publish_registry(
        "p1",
        Some(&pkg_json(None)),
        &[(".npmrc", npmrc)],
        None,
        None,
    );
    match lookup {
        PublishRegistryBase::Unsupported(message) => {
            assert!(
                message.contains("registry") && message.contains(".npmrc"),
                "the gap names the key and the file kind: {message}"
            );
        }
        other => unreachable!("empty npmrc registry must be Unsupported, got {other:?}"),
    }
}

#[test]
fn a_non_http_publish_registry_is_an_honest_decline() {
    for url in [
        "git+ssh://corp.example/registry",
        "file:///srv/registry",
        "corp.example",
    ] {
        let lookup = resolve_publish_registry("p1", Some(&pkg_json(Some(url))), &[], None, None);
        match lookup {
            PublishRegistryBase::Unsupported(message) => {
                assert!(
                    message.contains(url),
                    "the decline quotes the value: {message}"
                );
            }
            other => unreachable!("non-http {url:?} must be Unsupported, got {other:?}"),
        }
    }
}

#[test]
fn npmrc_comments_and_spacing_are_grammar() {
    let npmrc = "# comment\n; also a comment\n\nregistry = https://spaced.example/\n";
    assert_eq!(
        resolve_publish_registry(
            "p1",
            Some(&pkg_json(None)),
            &[(".npmrc", npmrc)],
            None,
            None
        ),
        PublishRegistryBase::Custom("https://spaced.example".to_owned())
    );
}

#[test]
fn an_unparsable_package_json_yields_the_fallback_chain() {
    // A manifest that cannot be parsed carries no publishConfig — the
    // npmrc chain still resolves (the spawn reads the same files).
    assert_eq!(
        resolve_publish_registry("p1", Some(b"{not json"), &[], None, None),
        PublishRegistryBase::Default
    );
    assert_eq!(
        resolve_publish_registry(
            "p1",
            Some(b"{not json"),
            &[(".npmrc", "registry=https://npmrc.example/")],
            None,
            None
        ),
        PublishRegistryBase::Custom("https://npmrc.example".to_owned())
    );
}

#[test]
fn a_missing_manifest_is_the_fallback_chain() {
    assert_eq!(
        resolve_publish_registry("p1", None, &[], None, None),
        PublishRegistryBase::Default
    );
}

#[test]
fn trailing_slashes_never_survive_the_base() {
    // The Npm client's with_registry_base trims trailing slashes; the
    // resolution is idempotent with it.
    assert_eq!(
        resolve_publish_registry(
            "p1",
            Some(&pkg_json(Some("https://corp.example/registry/"))),
            &[],
            None,
            None
        ),
        PublishRegistryBase::Custom("https://corp.example/registry".to_owned())
    );
}

#[test]
fn the_lookup_renders_honestly() {
    assert_eq!(
        PublishRegistryBase::Default.to_string(),
        "the default npm registry"
    );
    assert!(
        PublishRegistryBase::Custom("https://corp.example".to_owned())
            .to_string()
            .contains("https://corp.example")
    );
    assert!(
        PublishRegistryBase::Unsupported("broken".to_owned())
            .to_string()
            .contains("broken")
    );
}

#[test]
fn the_shell_reads_manifest_and_npmrc_from_disk() -> Result<(), BoxError> {
    let dir = tempfile::tempdir()?;
    let project = dir.path().join("project");
    std::fs::create_dir_all(&project)?;
    std::fs::write(
        project.join("package.json"),
        pkg_json(Some("https://corp.example/registry")),
    )?;
    std::fs::write(
        project.join(".npmrc"),
        "registry=https://project.example/\n",
    )?;
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home)?;
    std::fs::write(home.join(".npmrc"), "registry=https://user.example/\n")?;
    let lookup = halyard_eco_npm::publish_registry::publish_registry_base(
        &project,
        &home,
        "p1",
        "package.json",
    );
    assert_eq!(
        lookup,
        PublishRegistryBase::Custom("https://corp.example/registry".to_owned())
    );

    // Without the publishConfig key the project npmrc wins over the
    // user npmrc.
    std::fs::write(project.join("package.json"), pkg_json(None))?;
    let lookup = halyard_eco_npm::publish_registry::publish_registry_base(
        &project,
        &home,
        "p1",
        "package.json",
    );
    assert_eq!(
        lookup,
        PublishRegistryBase::Custom("https://project.example".to_owned())
    );

    // A missing project npmrc falls back to the user npmrc.
    std::fs::remove_file(project.join(".npmrc"))?;
    let lookup = halyard_eco_npm::publish_registry::publish_registry_base(
        &project,
        &home,
        "p1",
        "package.json",
    );
    assert_eq!(
        lookup,
        PublishRegistryBase::Custom("https://user.example".to_owned())
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// The scoped-registry environment variable (the routing completion,
// ADR-0021): the env form of the `@scope:registry` key. VERIFIED by
// probe (npm 11.19.1) — the variable name is the literal config-key
// suffix `npm_config_@<scope>:registry` (case-insensitive); the
// underscore intermediate form (`npm_config_<scope>_registry`) is NOT
// the scoped key (npm reads it as an unknown `myscope-registry` key and
// warns). The quotes and the seven probes live in
// `docs/research/publish-tooling.md`.
// ---------------------------------------------------------------------------

#[test]
fn the_scoped_env_variable_is_the_literal_key_suffix() {
    assert_eq!(
        scoped_registry_env_variable("myorg"),
        "npm_config_@myorg:registry"
    );
    assert_eq!(
        scoped_registry_env_variable("my-scope"),
        "npm_config_@my-scope:registry"
    );
}

#[test]
fn the_scoped_env_name_matches_case_insensitively() {
    // "Config values are case-insensitive, so NPM_CONFIG_FOO=bar will
    // work the same" — pinned by probe 4 (uppercase literal form).
    assert!(scoped_env_name_matches(
        std::ffi::OsStr::new("npm_config_@myorg:registry"),
        "myorg"
    ));
    assert!(scoped_env_name_matches(
        std::ffi::OsStr::new("NPM_CONFIG_@MYORG:REGISTRY"),
        "myorg"
    ));
    assert!(!scoped_env_name_matches(
        std::ffi::OsStr::new("npm_config_myorg_registry"),
        "myorg"
    ));
    assert!(!scoped_env_name_matches(
        std::ffi::OsStr::new("npm_config_@other:registry"),
        "myorg"
    ));
    assert!(!scoped_env_name_matches(
        std::ffi::OsStr::new("npm_config_registry"),
        "myorg"
    ));
}

#[test]
fn the_scoped_env_variable_overrides_the_npmrc_scoped_key() {
    // Probe 5: the env var and the .npmrc key are the same config key —
    // env ranks above the files.
    assert_eq!(
        resolve_publish_registry(
            "@myorg/pkg",
            Some(&pkg_json(None)),
            &[(".npmrc", "@myorg:registry=https://npmrc.example/")],
            None,
            Some("https://env.example/")
        ),
        PublishRegistryBase::Custom("https://env.example".to_owned())
    );
}

#[test]
fn the_scoped_env_variable_beats_every_plain_source() {
    // The scoped key governs a scoped package regardless of the source
    // tier (probe 7 + the cross-tier probe): `@scope:registry` beats
    // `registry` even when the plain value comes from the env tier.
    assert_eq!(
        resolve_publish_registry(
            "@myorg/pkg",
            Some(&pkg_json(None)),
            &[(".npmrc", "registry=https://npmrc.example/")],
            Some("https://plain-env.example/"),
            Some("https://scoped-env.example/")
        ),
        PublishRegistryBase::Custom("https://scoped-env.example".to_owned())
    );
}

#[test]
fn a_scoped_npmrc_key_beats_a_plain_env_registry() {
    // The cross-tier probe: npm resolves the scoped KEY first — a
    // `@scope:registry` value from an .npmrc file beats a plain
    // `NPM_CONFIG_REGISTRY` env value for that scope's packages.
    assert_eq!(
        resolve_publish_registry(
            "@myorg/pkg",
            Some(&pkg_json(None)),
            &[(".npmrc", "@myorg:registry=https://npmrc.example/")],
            Some("https://plain-env.example/"),
            None
        ),
        PublishRegistryBase::Custom("https://npmrc.example".to_owned())
    );
}

#[test]
fn a_scoped_env_registry_set_but_empty_fails_closed() {
    let lookup = resolve_publish_registry(
        "@myorg/pkg",
        Some(&pkg_json(None)),
        &[(".npmrc", "@myorg:registry=https://npmrc.example/")],
        None,
        Some("  "),
    );
    match lookup {
        PublishRegistryBase::Unsupported(message) => {
            assert!(
                message.contains("npm_config_@myorg:registry"),
                "the variable is named: {message}"
            );
        }
        other => unreachable!("empty scoped env must be Unsupported, got {other:?}"),
    }
}

#[test]
fn an_unscoped_package_has_no_scoped_env_source() {
    // The caller scans the environment for the package's own scope
    // only; an unscoped package takes None and never reads a scoped
    // variable.
    assert_eq!(
        resolve_publish_registry(
            "pkg",
            Some(&pkg_json(None)),
            &[],
            None,
            Some("https://scoped-env.example/")
        ),
        PublishRegistryBase::Default
    );
}

#[test]
fn a_non_http_scoped_env_value_is_an_honest_decline() {
    let lookup = resolve_publish_registry(
        "@myorg/pkg",
        Some(&pkg_json(None)),
        &[],
        None,
        Some("git+ssh://corp.example/registry"),
    );
    match lookup {
        PublishRegistryBase::Unsupported(message) => {
            assert!(
                message.contains("npm_config_@myorg:registry") && message.contains("http"),
                "the decline names the variable and the fix: {message}"
            );
        }
        other => unreachable!("non-HTTP scoped env must be Unsupported, got {other:?}"),
    }
}
