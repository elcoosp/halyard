//! The cargo-registry config-resolution contract (M5.2 continued, ADR-0021): a
//! `publish.registry` name other than the default resolves through the
//! cargo configuration — the `[registries.<name>]` `index` key and the
//! `CARGO_REGISTRIES_<NAME>_INDEX` environment override, VERIFIED
//! against the Cargo Book "Registries" page (the verbatim quotes live
//! in `docs/research/publish-tooling.md`). The resolution is the
//! routing map's input: a sparse-HTTP index yields a probeable base
//! (`halyard_registry::CratesIo::with_index_base`), anything else is an
//! honest decline that names the fix — never a guess.

use halyard_eco_cargo::registry_config::{
    RegistryIndexLookup, registry_env_fragment, registry_index, resolve_registry_index,
    sparse_index_base,
};

type BoxError = Box<dyn std::error::Error>;

fn sparse(url: &str) -> RegistryIndexLookup {
    RegistryIndexLookup::Sparse(url.to_owned())
}

fn unsupported(fragment: &str) -> RegistryIndexLookup {
    RegistryIndexLookup::Unsupported(fragment.to_owned())
}

#[test]
fn sparse_base_strips_the_scheme_and_trailing_slash() {
    assert_eq!(
        sparse_index_base("sparse+https://corp.example/index/").as_deref(),
        Some("https://corp.example/index")
    );
    // http is legal for loopback tests and intranet mirrors.
    assert_eq!(
        sparse_index_base("sparse+http://127.0.0.1:8080/index").as_deref(),
        Some("http://127.0.0.1:8080/index")
    );
    // Idempotent on an already-stripped base (the with_index_base
    // convention: trailing slashes never survive).
    assert_eq!(
        sparse_index_base("sparse+https://corp.example/index").as_deref(),
        Some("https://corp.example/index")
    );
}

#[test]
fn a_bare_https_url_is_a_git_index_not_a_sparse_probe() {
    // The Cargo Book: the `index` key is "a URL to a git repository
    // with the registry's index or a Cargo sparse registry URL with
    // the sparse+ prefix". A bare https URL is the git form — the
    // sparse-HTTP probe cannot speak it.
    assert_eq!(sparse_index_base("https://corp.example/git/index"), None);
    assert_eq!(sparse_index_base("http://corp.example/git/index"), None);
}

#[test]
fn other_index_shapes_are_not_probeable() {
    assert_eq!(sparse_index_base("git@corp.example:git/index"), None);
    assert_eq!(sparse_index_base("ssh://git@corp.example/index"), None);
    assert_eq!(sparse_index_base("file:///srv/index"), None);
    assert_eq!(sparse_index_base("registry+dir:///srv/index"), None);
    assert_eq!(sparse_index_base(""), None);
}

#[test]
fn resolve_reads_the_registries_table() {
    let config = "[registries.corp]\nindex = \"sparse+https://corp.example/index/\"\n";
    assert_eq!(
        resolve_registry_index("corp", &[("cfg", config)], None),
        sparse("https://corp.example/index")
    );
}

#[test]
fn later_config_overrides_earlier() {
    // Cargo's config hierarchy: files closer to the project override
    // the home-level ones. `configs` is passed in ascending precedence.
    let home = "[registries.corp]\nindex = \"sparse+https://home.example/index\"\n";
    let project = "[registries.corp]\nindex = \"sparse+https://corp.example/index\"\n";
    assert_eq!(
        resolve_registry_index("corp", &[("home", home), ("project", project)], None),
        sparse("https://corp.example/index")
    );
}

#[test]
fn the_env_index_overrides_every_file() {
    let config = "[registries.corp]\nindex = \"sparse+https://corp.example/index\"\n";
    assert_eq!(
        resolve_registry_index(
            "corp",
            &[("cfg", config)],
            Some("sparse+https://env.example/index")
        ),
        sparse("https://env.example/index")
    );
    // The env override can resolve a name no file mentions.
    assert_eq!(
        resolve_registry_index("corp", &[], Some("sparse+https://env.example/index")),
        sparse("https://env.example/index")
    );
}

#[test]
fn an_env_index_set_but_empty_fails_closed() {
    // The auth seam's empty-token rule at the index scale: a
    // present-but-empty override is a misconfiguration, refused with
    // the variable named (never silently read as unset).
    let lookup = resolve_registry_index("corp", &[], Some(""));
    match lookup {
        RegistryIndexLookup::Unsupported(message) => {
            assert!(
                message.contains("CARGO_REGISTRIES_CORP_INDEX"),
                "the variable is named: {message}"
            );
        }
        other => unreachable!("empty env must be Unsupported, got {other:?}"),
    }
}

#[test]
fn a_git_index_is_an_honest_decline_naming_the_fix() {
    let config = "[registries.corp]\nindex = \"https://corp.example/git/index\"\n";
    let lookup = resolve_registry_index("corp", &[("cfg", config)], None);
    match lookup {
        RegistryIndexLookup::Unsupported(message) => {
            assert!(
                message.contains("git") && message.contains("sparse+"),
                "the decline names the protocol and the fix: {message}"
            );
            assert!(
                message.contains("https://corp.example/git/index"),
                "the configured URL is quoted: {message}"
            );
        }
        other => unreachable!("a git index must be Unsupported, got {other:?}"),
    }
}

#[test]
fn an_entry_without_an_index_is_a_named_gap() {
    // A `[registries.corp]` table that carries only a token: the name
    // exists but nothing probeable is configured — a distinct,
    // honest decline (not "unknown": the config gap is specific).
    let config = "[registries.corp]\ntoken = \"t\"\n";
    let lookup = resolve_registry_index("corp", &[("cfg", config)], None);
    match lookup {
        RegistryIndexLookup::Unsupported(message) => {
            assert!(
                message.contains("no index") && message.contains("corp"),
                "the gap names the entry and the key: {message}"
            );
        }
        other => unreachable!("a token-only entry must be Unsupported, got {other:?}"),
    }
}

#[test]
fn quoted_key_names_resolve() {
    // Dotted registry names are legal TOML quoted keys (cargo's env
    // rule maps the dots to underscores on the variable side).
    let config = "[registries.\"my.corp\"]\nindex = \"sparse+https://corp.example/index\"\n";
    assert_eq!(
        resolve_registry_index("my.corp", &[("cfg", config)], None),
        sparse("https://corp.example/index")
    );
}

#[test]
fn a_malformed_config_is_refused_not_ignored() {
    // Fail closed: a config that cannot be parsed might have carried
    // the entry — reading it as "unknown" would half-honor a broken
    // configuration.
    let lookup = resolve_registry_index("corp", &[("cfg", "not toml [")], None);
    match lookup {
        RegistryIndexLookup::Unsupported(message) => {
            assert!(
                message.contains("parsable") || message.contains("parse"),
                "the decline names the parse failure: {message}"
            );
        }
        other => unreachable!("malformed TOML must be Unsupported, got {other:?}"),
    }
}

#[test]
fn an_unknown_name_stays_unknown() {
    assert_eq!(
        resolve_registry_index("corp", &[], None),
        RegistryIndexLookup::Unknown
    );
    // A config naming *another* registry says nothing about `corp`.
    let config = "[registries.other]\nindex = \"sparse+https://other.example/index\"\n";
    assert_eq!(
        resolve_registry_index("corp", &[("cfg", config)], None),
        RegistryIndexLookup::Unknown
    );
}

#[test]
fn an_empty_or_blank_name_is_refused() {
    for name in ["", "  ", "\t"] {
        let lookup = resolve_registry_index(name, &[], None);
        match lookup {
            RegistryIndexLookup::Unsupported(message) => {
                assert!(
                    message.contains("name"),
                    "the decline names the name problem: {message}"
                );
            }
            other => unreachable!("name {name:?} must be Unsupported, got {other:?}"),
        }
    }
}

#[test]
fn the_env_fragment_follows_the_documented_rule() {
    // The auth seam's twin (`registry_env_fragment` in
    // `halyard_publish::auth`): uppercase, dots and dashes converted
    // to underscores (the Cargo config reference's env-variable rule).
    assert_eq!(registry_env_fragment("corp"), "CORP");
    assert_eq!(registry_env_fragment("my-corp"), "MY_CORP");
    assert_eq!(registry_env_fragment("my.corp.io"), "MY_CORP_IO");
}

#[test]
fn the_shell_reads_the_config_hierarchy_from_disk() -> Result<(), BoxError> {
    let dir = tempfile::tempdir()?;
    let home = dir.path().join("home");
    let project = dir.path().join("project");
    std::fs::create_dir_all(home.join(".cargo"))?;
    std::fs::create_dir_all(project.join(".cargo"))?;
    std::fs::write(
        home.join(".cargo/config.toml"),
        "[registries.corp]\nindex = \"sparse+https://home.example/index\"\n",
    )?;
    std::fs::write(
        project.join(".cargo/config.toml"),
        "[registries.corp]\nindex = \"sparse+https://corp.example/index\"\n",
    )?;
    let lookup = registry_index(
        "corp",
        &[
            home.join(".cargo/config.toml"),
            project.join(".cargo/config.toml"),
        ],
    );
    assert_eq!(lookup, sparse("https://corp.example/index"));

    // A missing project file falls back to the home-level entry.
    std::fs::remove_file(project.join(".cargo/config.toml"))?;
    let lookup = registry_index(
        "corp",
        &[
            home.join(".cargo/config.toml"),
            project.join(".cargo/config.toml"),
        ],
    );
    assert_eq!(lookup, sparse("https://home.example/index"));
    Ok(())
}

#[test]
fn the_shell_refuses_a_malformed_file_with_its_path() -> Result<(), BoxError> {
    let dir = tempfile::tempdir()?;
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join(".cargo"))?;
    let path = project.join(".cargo/config.toml");
    std::fs::write(&path, "not toml [")?;
    let lookup = registry_index("corp", std::slice::from_ref(&path));
    match lookup {
        RegistryIndexLookup::Unsupported(message) => {
            assert!(
                message.contains(path.to_string_lossy().as_ref()),
                "the decline names the file: {message}"
            );
        }
        other => unreachable!("malformed config must be Unsupported, got {other:?}"),
    }
    Ok(())
}

#[test]
fn unsupported_and_unknown_render_honestly() {
    // Plain-data lookups carry their message in Display (the renderer
    // surfaces them verbatim in the E-PUB-REGISTRY row).
    let lookup = unsupported("the index for registry corp is a git index");
    assert!(lookup.to_string().contains("git index"));
    assert_eq!(RegistryIndexLookup::Unknown.to_string(), "unknown registry");
    assert!(
        sparse("https://a/index")
            .to_string()
            .contains("https://a/index")
    );
}

// The `unreachable!` sites above are the test-context convention the
// workspace's clippy.toml allows (`allow-unwrap-in-tests` & co.): a
// contract test asserting an unexpected shape must fail loudly, not
// silently pass.
