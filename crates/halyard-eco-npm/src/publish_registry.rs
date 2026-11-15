//! The npm publish-registry resolution (M5.2 continued, ADR-0021):
//! where the §13 probe must look for one npm package, read from the
//! configuration the spawned `pnpm publish` itself reads — so the probe
//! and the tool cannot disagree about where a package publishes.
//!
//! The grammar is VERIFIED against the npm documentation (verbatim
//! quotes recorded in `docs/research/publish-tooling.md`):
//!
//! * `publishConfig` — "a set of config values that will be used at
//!   publish-time"; "use the publishConfig dictionary described below
//!   to override the registry config param at publish-time" (package.json
//!   docs). The per-package publish-time override, and therefore the
//!   highest source this module reads.
//! * `NPM_CONFIG_REGISTRY` — "Any environment variables that start
//!   with `npm_config_` will be interpreted as a configuration
//!   parameter" (config docs); environment variables rank above the
//!   npmrc files in npm's documented priority ladder.
//! * `registry` — "The base URL of the npm registry", default
//!   `https://registry.npmjs.org/`; the per-project `.npmrc` wins over
//!   the per-user `$HOME/.npmrc` (npmrc files passed in ascending
//!   precedence, the cargo-resolution convention).
//! * `@scope:registry` — npm config's scoped-registry grammar: a
//!   `@scope/name` package publishes through its scope's key, beating
//!   the plain `registry` key **regardless of the source tier** (the
//!   cross-tier probe: a scoped .npmrc key beats a plain env registry
//!   value for that scope's packages).
//! * `npm_config_@<scope>:registry` — the scoped key's environment
//!   form, VERIFIED by probe (npm 11.19.1; the seven probes are
//!   recorded in `docs/research/publish-tooling.md`): the variable
//!   name is the literal config-key suffix, case-insensitive; the
//!   underscore intermediate form is NOT the scoped key.
//!
//! Fail-closed rules mirror the cargo side
//! (`halyard_eco_cargo::registry_config`): a set-but-empty value is
//! refused with the variable/key named (the `E-PUB-AUTH` empty-token
//! rule at the URL scale), a non-HTTP URL is an honest decline naming
//! the fix, and a manifest that cannot be parsed carries no
//! `publishConfig` — the npmrc chain still resolves (the spawn reads
//! the same files).
//!
//! The consumer is the CLI shell's routing map: a
//! [`PublishRegistryBase::Custom`] base builds a
//! `halyard_registry::Npm::with_registry_base` client as a per-target
//! override (`TargetRouter`, ADR-0021 — two npm packages of one
//! workspace may publish to different registries); [`PublishRegistryBase::Default`]
//! rides the by-name route; every other outcome refuses the run before
//! any call (`E-PUB-REGISTRY`, zero side effects).

use std::path::Path;

use crate::json_span::JsonDoc;

/// The resolved publish-registry base of one npm package. Plain
/// wire-shape data (§5.8): the CLI shell maps each variant onto the
/// routing map or its honest decline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublishRegistryBase {
    /// No source names a registry: the documented default
    /// (`https://registry.npmjs.org/`) governs — the by-name route
    /// already probes it.
    Default,
    /// A custom packument base URL (http/https, trailing slash
    /// trimmed) — probeable with `halyard_registry::Npm::
    /// with_registry_base` as a per-target override.
    Custom(
        /// The probeable packument base URL.
        String,
    ),
    /// A source named a registry but the value is unusable. The
    /// message names the source, the value and the fix — surfaced
    /// verbatim in the `E-PUB-REGISTRY` row.
    Unsupported(
        /// What is wrong and what to do instead.
        String,
    ),
}

impl std::fmt::Display for PublishRegistryBase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Default => write!(f, "the default npm registry"),
            Self::Custom(base) => write!(f, "npm registry base {base}"),
            Self::Unsupported(message) => write!(f, "{message}"),
        }
    }
}

/// The documented default (`registry` config key, npm docs).
const DEFAULT_REGISTRY: &str = "https://registry.npmjs.org";

/// The scoped-registry environment variable of one scope — the literal
/// config-key suffix, VERIFIED by probe (npm 11.19.1; probes recorded
/// in `docs/research/publish-tooling.md`): `npm config get
/// @myorg:registry` resolves when the environment carries
/// `npm_config_@myorg:registry` (and the uppercase spelling — config
/// values are case-insensitive), while the underscore intermediate form
/// (`npm_config_myorg_registry`) is NOT the scoped key — npm reads it
/// as an unknown `myorg-registry` key and warns it will stop working.
#[must_use]
pub fn scoped_registry_env_variable(scope: &str) -> String {
    format!("npm_config_@{scope}:registry")
}

/// Whether one environment-variable name is the scoped-registry
/// variable of `scope` (ASCII case-insensitive — the documented
/// case-insensitivity rule, pinned by probe 4).
#[must_use]
pub fn scoped_env_name_matches(actual: &std::ffi::OsStr, scope: &str) -> bool {
    let expected = scoped_registry_env_variable(scope);
    actual
        .to_str()
        .is_some_and(|name| name.eq_ignore_ascii_case(&expected))
}

/// Scans the environment for the scoped-registry variable of `scope`
/// (the I/O shell's env read; the variable name is not a valid POSIX
/// shell identifier, so CI systems set it through their env maps and
/// the value arrives here). The first case-insensitive match wins.
#[must_use]
pub fn scan_scoped_registry_env(scope: &str) -> Option<String> {
    std::env::vars_os()
        .find(|(name, _)| scoped_env_name_matches(name, scope))
        .and_then(|(_, value)| value.to_str().map(ToOwned::to_owned))
}

/// Resolves the publish-registry base of one npm package (the pure
/// core the I/O shell and the tests share).
///
/// * `name` — the package name (`p1` or `@scope/pkg`; the scope
///   selects the `@scope:registry` npmrc key and its env variable).
/// * `package_json` — the manifest bytes when readable (`None` = the
///   manifest is absent or unreadable; it carries no `publishConfig`
///   then, and the npmrc chain still resolves).
/// * `npmrc_files` — `(source-label, text)` pairs in **ascending
///   precedence** (the per-user file first, the per-project file
///   last; a later file overrides an earlier one per key).
/// * `env_registry` — the `NPM_CONFIG_REGISTRY` value when set.
/// * `env_scoped_registry` — the `npm_config_@<scope>:registry` value
///   when set (the scoped key's env form, VERIFIED by probe; only a
///   scoped package has one).
///
/// The resolution mirrors npm's own grammar (VERIFIED, quotes and
/// probes in `docs/research/publish-tooling.md`): the per-package
/// publish-time `publishConfig.registry` outranks everything; within
/// the remaining config keys the scoped key governs a scoped package
/// **regardless of the source tier** (a `@scope:registry` value from
/// an .npmrc file beats a plain `NPM_CONFIG_REGISTRY` env value —
/// probe-pinned), and within one key the env tier beats the npmrc
/// files. A set-but-empty value is refused with the source named.
#[must_use]
pub fn resolve_publish_registry(
    name: &str,
    package_json: Option<&[u8]>,
    npmrc_files: &[(&str, &str)],
    env_registry: Option<&str>,
    env_scoped_registry: Option<&str>,
) -> PublishRegistryBase {
    // The publish-time, per-package override outranks the general
    // config sources (npm docs: publishConfig overrides "the registry
    // config param at publish-time").
    if let Some(bytes) = package_json
        && let Some(lookup) = publish_config_registry(bytes)
    {
        return lookup;
    }
    let scope = name
        .strip_prefix('@')
        .and_then(|rest| rest.split('/').next());

    // The scoped key's env form (probe-verified variable name): the
    // env tier of the scoped key beats the files tier of the same key.
    if let Some(scope) = scope
        && let Some(value) = env_scoped_registry
    {
        if value.trim().is_empty() {
            return PublishRegistryBase::Unsupported(format!(
                "the {} environment variable is set but empty — an empty registry URL is a \
                 doomed resolution; set it to the registry's http(s) base URL or unset it",
                scoped_registry_env_variable(scope)
            ));
        }
        return validate_registry_url(
            value,
            &format!("the {} override", scoped_registry_env_variable(scope)),
        );
    }

    // npmrc files, ascending precedence: the per-project file wins
    // over the per-user one; a later file overrides per key.
    let mut plain: Option<String> = None;
    let mut scoped: Option<String> = None;
    let mut scoped_seen = false;
    for (_label, text) in npmrc_files {
        let (file_plain, file_scoped) = npmrc_registries(text, scope);
        if file_plain.is_some() {
            plain = file_plain;
        }
        if file_scoped.is_some() {
            scoped = file_scoped;
            scoped_seen = true;
        }
    }
    // A set-but-empty value is a misconfiguration regardless of which
    // key carried it — refused, never read as unset.
    let scoped_value = if scoped_seen { scoped.as_deref() } else { None };
    for (source, value) in [
        ("the @scope:registry key in an .npmrc file", scoped_value),
        ("the registry key in an .npmrc file", plain.as_deref()),
    ] {
        if let Some(value) = value
            && value.trim().is_empty()
        {
            return PublishRegistryBase::Unsupported(format!(
                "{source} is set but empty — an empty registry URL is a doomed resolution; \
                 set it to the registry's http(s) base URL or remove the key"
            ));
        }
    }
    // The scoped key governs a scoped package regardless of the source
    // tier: it beats the plain `registry` key even when the plain value
    // comes from the env tier (the cross-tier probe, pinned by tests).
    if scoped_seen && let Some(value) = scoped.as_deref() {
        return validate_registry_url(value, "the @scope:registry key");
    }
    // The plain key: the env tier beats the files tier.
    if let Some(value) = env_registry {
        if value.trim().is_empty() {
            return PublishRegistryBase::Unsupported(
                "the NPM_CONFIG_REGISTRY environment variable is set but empty — an empty \
                 registry URL is a doomed resolution; set it to the registry's http(s) base \
                 URL or unset it"
                    .to_owned(),
            );
        }
        return validate_registry_url(value, "the NPM_CONFIG_REGISTRY override");
    }
    if let Some(value) = plain.as_deref() {
        return validate_registry_url(value, "the .npmrc registry key");
    }
    PublishRegistryBase::Default
}

/// The I/O shell of [`resolve_publish_registry`]: reads the manifest
/// bytes, the per-project and per-user `.npmrc`, the
/// `NPM_CONFIG_REGISTRY` environment value and — for a scoped package —
/// the scoped key's `npm_config_@<scope>:registry` variable
/// (case-insensitive scan, VERIFIED by probe).
#[must_use]
pub fn publish_registry_base(
    project_root: &Path,
    home_dir: &Path,
    name: &str,
    manifest: &str,
) -> PublishRegistryBase {
    let package_json = std::fs::read(project_root.join(manifest)).ok();
    let mut npmrc_files: Vec<(String, String)> = Vec::new();
    for (label, path) in [
        ("user .npmrc", home_dir.join(".npmrc")),
        ("project .npmrc", project_root.join(".npmrc")),
    ] {
        if let Ok(text) = std::fs::read_to_string(&path) {
            npmrc_files.push((label.to_owned(), text));
        }
    }
    let borrowed: Vec<(&str, &str)> = npmrc_files
        .iter()
        .map(|(label, text)| (label.as_str(), text.as_str()))
        .collect();
    let env_registry = std::env::var_os("NPM_CONFIG_REGISTRY")
        .and_then(|value| value.to_str().map(ToOwned::to_owned));
    let env_scoped_registry = name
        .strip_prefix('@')
        .and_then(|rest| rest.split('/').next())
        .and_then(scan_scoped_registry_env);
    resolve_publish_registry(
        name,
        package_json.as_deref(),
        &borrowed,
        env_registry.as_deref(),
        env_scoped_registry.as_deref(),
    )
}

/// Reads `publishConfig.registry` out of the manifest bytes (`None`
/// when the bytes are not a parsable JSON object or the key is absent).
fn publish_config_registry(bytes: &[u8]) -> Option<PublishRegistryBase> {
    let doc = JsonDoc::parse(bytes).ok()?;
    let value = doc.get_path(&["publishConfig", "registry"])?;
    match &value.value {
        crate::json_span::JsonValue::String { decoded, .. } => {
            if decoded.trim().is_empty() {
                return Some(PublishRegistryBase::Unsupported(
                    "the publishConfig.registry key in package.json is set but empty — an \
                     empty registry URL is a doomed resolution; set it to the registry's \
                     http(s) base URL or remove the key"
                        .to_owned(),
                ));
            }
            Some(validate_registry_url(
                decoded,
                "the publishConfig.registry key",
            ))
        }
        _ => Some(PublishRegistryBase::Unsupported(
            "the publishConfig.registry key in package.json is not a string — set it to the \
             registry's http(s) base URL"
                .to_owned(),
        )),
    }
}

/// Extracts the plain `registry` and the scope's `@scope:registry`
/// values from one npmrc text (`None` = the key is absent; an empty
/// string = present-but-empty, refused by the caller).
fn npmrc_registries(text: &str, scope: Option<&str>) -> (Option<String>, Option<String>) {
    let scope_key = scope.map(|scope| format!("@{scope}:registry"));
    let mut plain = None;
    let mut scoped = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key == "registry" && plain.is_none() {
            plain = Some(strip_quotes(value.trim()).to_owned());
        }
        if Some(key) == scope_key.as_deref() && scoped.is_none() {
            scoped = Some(strip_quotes(value.trim()).to_owned());
        }
    }
    (plain, scoped)
}

/// Strips one matching pair of surrounding `"` delimiters (npmrc
/// quoted-value syntax, not part of the URL).
fn strip_quotes(value: &str) -> &str {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        return value.get(1..value.len() - 1).unwrap_or(value);
    }
    value
}

/// Validates one registry base URL: http/https with a non-empty host,
/// the trailing slash trimmed (idempotent with the `Npm` client's
/// `with_registry_base`). Anything else is an honest decline naming
/// the source and the fix.
fn validate_registry_url(url: &str, source: &str) -> PublishRegistryBase {
    let trimmed = url.trim();
    let after_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"));
    let valid = after_scheme
        .is_some_and(|after| after.split('/').next().is_some_and(|host| !host.is_empty()));
    if !valid {
        return PublishRegistryBase::Unsupported(format!(
            "{source} is {url:?}, which is not an http(s) registry base URL — the npm \
             packument probe speaks HTTP(S); set it to the registry's base URL (default \
             {DEFAULT_REGISTRY}) or remove the override"
        ));
    }
    PublishRegistryBase::Custom(trimmed.trim_end_matches('/').to_owned())
}
