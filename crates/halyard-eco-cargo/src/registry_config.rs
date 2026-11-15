//! The cargo-registry config resolution (M5.2 continued, ADR-0021): which index
//! URL a `publish.registry` name resolves to, read from the cargo
//! configuration the spawned `cargo publish --registry <name>` itself
//! reads — the probe and the tool can never disagree about where a
//! registry lives.
//!
//! The grammar is VERIFIED against the Cargo Book "Registries" page
//! (verbatim quotes recorded in `docs/research/publish-tooling.md`):
//!
//! > To use a registry other than crates.io, the name and index URL of
//! > the registry must be added to a `.cargo/config.toml` file. The
//! > registries table has a key for each registry
//!
//! > The `index` key should be a URL to a git repository with the
//! > registry's index or a Cargo sparse registry URL with the `sparse+`
//! > prefix.
//!
//! and the environment override
//!
//! > `CARGO_REGISTRIES_MY_REGISTRY_INDEX=https://my-intranet:8080/git/index`
//!
//! The resolution therefore reads, in ascending precedence:
//!
//! 1. `$CARGO_HOME/config.toml` (default `$HOME/.cargo/config.toml`)
//!    and its legacy `config` sibling,
//! 2. `<project>/.cargo/config.toml` and `<project>/.cargo/config` —
//!    closer directories override, and per directory the file WITHOUT
//!    the extension wins when both exist (the Cargo Book:
//!    "If both files exist, Cargo will use the file without the
//!    extension" — the routing-completion quote, verbatim in
//!    `docs/research/publish-tooling.md`; the earlier draft's
//!    "deprecated upstream, out of scope" claim was wrong — the page
//!    never says deprecated, and the legacy file is what an operator
//!    with the old layout actually reads),
//! 3. the `CARGO_REGISTRIES_<NAME>_INDEX` environment variable, which
//!    overrides every file (the env-variable rule: uppercase, dots and
//!    dashes converted to underscores — the same rule
//!    `halyard_publish::auth`'s token resolution documents).
//!
//! Only the `sparse+` HTTP form yields a probeable index — the resume
//! probe speaks the sparse-HTTP protocol, not the git protocol. A git
//! index or any other URL is an honest [`RegistryIndexLookup::
//! Unsupported`] decline that names the fix (§15.4 "errors teach");
//! a config file that cannot be parsed is refused, never read as "the
//! entry is absent" (that would half-honor a broken configuration).
//!
//! The consumer is the CLI shell's routing map: a [`RegistryIndexLookup::Sparse`]
//! base builds a `halyard_registry::CratesIo::with_index_base` client,
//! joining the §13 resume/visibility machinery for the custom
//! registry; every other outcome refuses the run before any call
//! (`E-PUB-REGISTRY`, zero side effects).

use std::path::{Path, PathBuf};

/// The outcome of resolving one registry name through the cargo
/// configuration. Plain wire-shape data (§5.8): the CLI shell maps each
/// variant onto the routing map or its honest decline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryIndexLookup {
    /// A sparse-HTTP index base (the `sparse+` scheme stripped, the
    /// trailing slash trimmed) — probeable with
    /// `halyard_registry::CratesIo::with_index_base`.
    Sparse(
        /// The probeable index base URL.
        String,
    ),
    /// The name exists (or its environment override does) but the
    /// configuration yields nothing probeable. The message names the
    /// protocol gap and the fix — surfaced verbatim in the
    /// `E-PUB-REGISTRY` row.
    Unsupported(
        /// What is wrong and what to do instead.
        String,
    ),
    /// No configuration names the registry (no `[registries.<name>]`
    /// entry anywhere in the hierarchy and no env override).
    Unknown,
}

impl std::fmt::Display for RegistryIndexLookup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sparse(base) => write!(f, "sparse index base {base}"),
            Self::Unsupported(message) => write!(f, "{message}"),
            Self::Unknown => write!(f, "unknown registry"),
        }
    }
}

/// The classified `index` URL of one `[registries.<name>]` entry (or
/// env override) — the Cargo Book's two accepted forms plus everything
/// else, each mapped to its honest outcome.
enum IndexUrl {
    /// A `sparse+http(s)://…` URL — the probeable form.
    Sparse,
    /// A bare `http(s)://…` URL — the git-protocol index form (the
    /// probe cannot speak it).
    Git,
    /// Anything else (`ssh://`, `git@host:path`, `file://`, a bare
    /// path, an empty value).
    Other,
}

/// Classifies one raw `index` value.
fn classify_index_url(url: &str) -> IndexUrl {
    if let Some(rest) = url.strip_prefix("sparse+") {
        let after_scheme = rest
            .strip_prefix("https://")
            .or_else(|| rest.strip_prefix("http://"));
        let sparse = after_scheme
            .is_some_and(|after| after.split('/').next().is_some_and(|host| !host.is_empty()));
        if sparse {
            return IndexUrl::Sparse;
        }
        return IndexUrl::Other;
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        // The Cargo Book's other accepted form: a git-repository index.
        return IndexUrl::Git;
    }
    IndexUrl::Other
}

/// Converts a `sparse+` cargo index URL into the probeable base (the
/// scheme stripped, the trailing slash trimmed):
/// `sparse+https://corp.example/index/` → `https://corp.example/index`.
/// `None` for every non-sparse form (the caller declines honestly).
#[must_use]
pub fn sparse_index_base(cargo_index_url: &str) -> Option<String> {
    match classify_index_url(cargo_index_url) {
        IndexUrl::Sparse => {
            let rest = cargo_index_url
                .strip_prefix("sparse+")
                .unwrap_or_else(|| unreachable!("classified Sparse carries the scheme"));
            Some(rest.trim_end_matches('/').to_owned())
        }
        IndexUrl::Git | IndexUrl::Other => None,
    }
}

/// The `<NAME>` fragment of `CARGO_REGISTRIES_<NAME>_INDEX`: the
/// registry name uppercased with dots and dashes converted to
/// underscores (the Cargo config reference's env-variable rule — the
/// same documented rule `halyard_publish::auth`'s `registry_env_fragment`
/// applies to the token side, so the index and the token of one
/// registry can never disagree about their variable names).
#[must_use]
pub fn registry_env_fragment(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '.' | '-' => '_',
            other => other,
        })
        .collect::<String>()
        .to_uppercase()
}

/// The env-override variable of one registry name.
fn env_variable(name: &str) -> String {
    format!("CARGO_REGISTRIES_{}_INDEX", registry_env_fragment(name))
}

/// Resolves one registry name against parsed-or-raw configuration
/// texts (the pure core the I/O shell and the bench share).
///
/// `configs` carries `(source-label, toml-text)` pairs in **ascending
/// precedence** — a later entry overrides an earlier one (cargo's
/// config hierarchy: files closer to the project win). `env_index` is
/// the `CARGO_REGISTRIES_<NAME>_INDEX` value when set (it overrides
/// every file; a set-but-empty value is refused, the auth seam's
/// empty-token rule at the index scale).
#[must_use]
pub fn resolve_registry_index(
    name: &str,
    configs: &[(&str, &str)],
    env_index: Option<&str>,
) -> RegistryIndexLookup {
    let name = name.trim();
    if name.is_empty() {
        return RegistryIndexLookup::Unsupported(
            "the registry name is empty or blank — set publish.cargo.registry to the \
             configured registry's name (a [registries.<name>] key cannot be empty)"
                .to_owned(),
        );
    }
    if let Some(value) = env_index {
        if value.trim().is_empty() {
            return RegistryIndexLookup::Unsupported(format!(
                "the {} environment variable is set but empty — an empty index URL is a \
                 doomed resolution; set it to the registry's sparse+ URL or unset it",
                env_variable(name)
            ));
        }
        return classify_resolved(name, value, "the environment override");
    }

    // File hierarchy, ascending precedence: a later file's index
    // overrides an earlier one; a table without an `index` key does
    // not erase an earlier index (cargo merges per key). A file that
    // cannot be parsed is refused — it might have carried the entry.
    let mut resolved: Option<String> = None;
    let mut entry_without_index = false;
    for (label, text) in configs {
        let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
            return RegistryIndexLookup::Unsupported(format!(
                "the {label} configuration is not parsable TOML — the \
                 [registries.{name}] entry cannot be read from it; fix the file"
            ));
        };
        let Some(entry) = doc
            .get("registries")
            .and_then(|registries| registries.get(name))
        else {
            continue;
        };
        match entry.get("index").and_then(toml_edit::Item::as_str) {
            Some(url) => resolved = Some(url.to_owned()),
            None => entry_without_index = true,
        }
    }
    match resolved {
        Some(url) => classify_resolved(name, &url, "the configured index"),
        None if entry_without_index => RegistryIndexLookup::Unsupported(format!(
            "the [registries.{name}] entry carries no index — the resume probe needs the \
             registry's sparse+ index URL; add index = \"sparse+…\" to the entry or disable \
             publishing for the registry's packages"
        )),
        None => RegistryIndexLookup::Unknown,
    }
}

/// Maps one classified URL onto the lookup's honest outcomes.
fn classify_resolved(name: &str, url: &str, source: &str) -> RegistryIndexLookup {
    match classify_index_url(url) {
        IndexUrl::Sparse => RegistryIndexLookup::Sparse(
            sparse_index_base(url).unwrap_or_else(|| unreachable!("classified Sparse")),
        ),
        IndexUrl::Git => RegistryIndexLookup::Unsupported(format!(
            "{source} for registry {name} is a git-protocol index ({url}) — the resume probe \
             speaks the sparse HTTP protocol (the Cargo Book's `sparse+` form); configure a \
             sparse+ index URL or disable publishing for the registry's packages"
        )),
        IndexUrl::Other => RegistryIndexLookup::Unsupported(format!(
            "{source} for registry {name} ({url:?}) is not a sparse HTTP index URL — the \
             Cargo Book's `index` key accepts a git-protocol URL or a `sparse+` URL, and only \
             the `sparse+` form is probeable; configure a sparse+ index URL or disable \
             publishing for the registry's packages"
        )),
    }
}

/// The config files the resolution reads, in ascending precedence:
/// per directory (`$CARGO_HOME`, default `$HOME/.cargo`, then the
/// project's `.cargo`) the `config.toml` file first and the legacy
/// `config` file last — the Cargo Book: "Cargo also reads config files
/// without the .toml extension, such as .cargo/config. Support for the
/// .toml extension was added in version 1.39 and is the preferred
/// form. If both files exist, Cargo will use the file without the
/// extension." (verbatim in `docs/research/publish-tooling.md`).
/// Missing files are skipped by [`registry_index`].
#[must_use]
pub fn cargo_config_paths(project_root: &Path) -> Vec<PathBuf> {
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")));
    config_paths_for(home.as_deref(), project_root)
}

/// The pure enumeration [`cargo_config_paths`] runs: the two
/// directories' `config.toml` files first and the legacy `config`
/// siblings last, so a later (higher-precedence) entry overrides per
/// key — the merge in [`resolve_registry_index`] then reproduces the
/// Cargo Book's rule (the file without the extension wins when both
/// exist; a file without the key does not erase the other's entries).
#[must_use]
pub fn config_paths_for(home: Option<&Path>, project_root: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = home {
        paths.push(home.join("config.toml"));
        paths.push(home.join("config"));
    }
    paths.push(project_root.join(".cargo/config.toml"));
    paths.push(project_root.join(".cargo/config"));
    paths
}

/// The I/O shell of [`resolve_registry_index`]: reads every config
/// file (an absent or unreadable file carries no entry), then resolves
/// with the `CARGO_REGISTRIES_<NAME>_INDEX` environment value.
#[must_use]
pub fn registry_index(name: &str, config_paths: &[PathBuf]) -> RegistryIndexLookup {
    let mut configs: Vec<(String, String)> = Vec::new();
    for path in config_paths {
        if let Ok(text) = std::fs::read_to_string(path) {
            configs.push((path.display().to_string(), text));
        }
    }
    let borrowed: Vec<(&str, &str)> = configs
        .iter()
        .map(|(label, text)| (label.as_str(), text.as_str()))
        .collect();
    let env_value = std::env::var_os(env_variable(name))
        .and_then(|value| value.to_str().map(ToOwned::to_owned));
    resolve_registry_index(name, &borrowed, env_value.as_deref())
}
