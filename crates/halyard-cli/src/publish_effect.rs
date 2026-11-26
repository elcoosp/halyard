//! The `publish` command's spawn effect (M5.2 step 2, §13.4): the real
//! `cargo publish` / `pnpm publish` spawn behind the engine's
//! [`PublishEffect`] seam. The adapter crates own the argv grammar
//! (`cargo_publish_args` / `pnpm_publish_args`, verified against the
//! tool docs in `docs/research/publish-tooling.md`); this module owns
//! the spawn discipline — argv arrays only (I7), the process rooted at
//! the package root (never the ambient cwd, the tag-push lesson), the
//! resolved credentials injected as an environment pair (never argv,
//! never logged — M5.3 step 1, ADR-0019), tool output scrubbed of
//! every resolved secret before it reaches a report row, and a
//! fail-closed honest decline for ecosystems no argv table covers.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use halyard_forge_github::UreqTransport;
use halyard_publish::{
    AuthOutcome, PublishAuth, PublishEffect, PublishError, PublishTarget, TargetRouter,
    scrub_secrets,
};
use halyard_registry::{CratesIo, Npm, PyPi, RegistrySleeper};

/// The production spawn effect: `dry` selects the tool's documented
/// dry form (`cargo publish --dry-run --offline` / `pnpm publish
/// --dry-run --no-git-checks`) — the §13.8 letter; the engine's
/// [`halyard_publish::PublishMode::Dry`] decides whether the run
/// probes the registry at all, this flag decides what the tool does.
///
/// The spawn discipline (I7, the tag-push lesson): every process is
/// rooted at the **package root** (never the ambient cwd — pnpm has no
/// directory flag, and cargo's `--manifest-path` is resolved against
/// the repo root before the cwd switch), argv arrays only, credentials
/// ride the environment through the shell-owned [`PublishAuth`]
/// provider (the resolved pair injected per target; the token value is
/// never logged, and scrubbed from any tool output that surfaces).
pub struct SpawnPublishEffect {
    repo_root: std::path::PathBuf,
    dry: bool,
    auth: Arc<dyn PublishAuth>,
}

impl SpawnPublishEffect {
    /// The effect for one command run; `repo_root` is the directory the
    /// target paths are relative to (the CLI's `-C` root). `auth` is
    /// the shell-captured provider (the CLI builds
    /// `EnvPublishAuth::capture()`; tests inject a table) — resolution
    /// and scrubbing share one snapshot, so the scrubbing contract
    /// cannot drift from what was injected.
    pub fn new(repo_root: std::path::PathBuf, dry: bool, auth: Arc<dyn PublishAuth>) -> Self {
        Self {
            repo_root,
            dry,
            auth,
        }
    }
}

impl PublishEffect for SpawnPublishEffect {
    fn publish(&self, target: &PublishTarget) -> Result<(), PublishError> {
        // The auth seam resolves before the spawn: a misconfiguration
        // (E-PUB-AUTH) refuses without touching the tool, absence
        // injects nothing (the tool's own credential store governs).
        let credential = match self.auth.credentials(target) {
            Ok(AuthOutcome::Token(credential)) => Some(credential),
            Ok(AuthOutcome::Absent) => None,
            Err(error) => return Err(error),
        };
        let (tool, args): (&str, Vec<String>) = match target.ecosystem.as_str() {
            "cargo" => {
                // `--manifest-path` is resolved against the repo root
                // (the paths in the publication are repo-relative);
                // the process itself runs at the package root.
                let manifest = self.repo_root.join(&target.manifest);
                // The plan's publish contract names the registry (§13);
                // the adapter's grammar maps it — the default
                // registries ride the tool's own default, any other
                // configured name reaches the spawn as `--registry
                // <name>` (M5.3 continued, VERIFIED in the research
                // note). The token side resolves the same name through
                // the auth seam below, so argv and env agree.
                let args = halyard_eco_cargo::cargo_publish_args(
                    &manifest.to_string_lossy(),
                    &target.registry,
                    self.dry,
                );
                ("cargo", args)
            }
            // §13.4 names `pnpm publish` as the npm-side tool; a
            // plain-npm repository without pnpm fails honestly rather
            // than switching to a different tool with different
            // packaging semantics (see the research note). The plan
            // contract's npm options (ADR-0033) ride the adapter's
            // grammar — unset stays bare (the tool's own
            // trusted-publishing posture governs).
            "npm" => {
                let options = halyard_eco_npm::NpmPublishOptions {
                    access: target.access.clone(),
                    provenance: target.provenance,
                };
                (
                    "pnpm",
                    halyard_eco_npm::pnpm_publish_args(self.dry, &options),
                )
            }
            // M8 step 2 (ADR-0056): the pypi adapter's grammar — bare
            // argv (uv's own posture governs trusted publishing and the
            // index); the token rides the auth seam's environment pair
            // (UV_PUBLISH_TOKEN, the documented `--token` variable).
            "pypi" => ("uv", halyard_eco_pypi::uv_publish_args(self.dry)),
            other => {
                return Err(PublishError::Effect {
                    ecosystem: target.ecosystem.clone(),
                    message: format!(
                        "no publish tool grammar for ecosystem {other:?} — the adapter tables \
                         cover cargo, npm and pypi (§13.4); extending one is an ADR"
                    ),
                    retryable: false,
                });
            }
        };
        let mut command = Command::new(tool);
        command
            .args(&args)
            .current_dir(package_root(self.repo_root.as_path(), target));
        if let Some(credential) = &credential {
            // The resolved pair rides the environment — argv (I7) and
            // every log path stay credential-free; the value is
            // scrubbed from anything the tool surfaces (below).
            command.env(&credential.key, &credential.value);
        }
        let output = command.output().map_err(|error| PublishError::Effect {
            ecosystem: target.ecosystem.clone(),
            message: format!("spawn {tool}: {error} (is the tool installed?)"),
            // A missing tool is deterministic for this environment
            // — the resume probe protects the re-run either way.
            retryable: false,
        })?;
        if output.status.success() {
            return Ok(());
        }
        // The tool's own words carry the diagnosis (the last non-empty
        // stderr line — a trailing blank line must not blank the
        // diagnosis), scrubbed of every resolved secret before it
        // reaches the row: a token a misconfigured tool echoes never
        // reaches the renderers (§13.5 "never log tokens" is enforced
        // here, not by convention).
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail = stderr
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("");
        let secrets = self.auth.secrets();
        let refs: Vec<&str> = secrets.iter().map(String::as_str).collect();
        Err(PublishError::Effect {
            ecosystem: target.ecosystem.clone(),
            message: format!(
                "{tool} exited {}: {}",
                output.status.code().unwrap_or(-1),
                scrub_secrets(tail, &refs)
            ),
            retryable: false,
        })
    }
}

/// The package root as a native path: `repo_root` joined with the
/// target's repo-relative root (`""` = the repo root itself).
fn package_root(repo_root: &Path, target: &PublishTarget) -> std::path::PathBuf {
    if target.manifest_dir.is_empty() {
        repo_root.to_path_buf()
    } else {
        repo_root.join(&target.manifest_dir)
    }
}

/// The live registry routing: the two §13 clients over the blocking
/// transport (the workspace's only HTTP client crate; the engine routes
/// before any call, so an unused client costs nothing), **plus** one
/// client per configured custom cargo registry and one override per
/// npm package that names its own registry (ADR-0021):
///
/// * every non-default target registry *name* resolves through the
///   cargo configuration (`[registries.<name>]` / its env override —
///   the same configuration `cargo publish --registry <name>` reads, so
///   the probe and the tool cannot disagree about where a registry
///   lives). A sparse-HTTP index joins the map as a `CratesIo` client
///   over the resolved base.
/// * every npm target's package resolves its publish registry through
///   the npm configuration the spawned `pnpm publish` reads
///   (`publishConfig.registry`, `NPM_CONFIG_REGISTRY`, the `.npmrc`
///   `registry`/`@scope:registry` keys) — a custom base joins the map
///   as a per-target override ([`TargetRouter`]: two npm packages of
///   one workspace may publish to different registries), the default
///   rides the by-name route.
///
/// An unresolvable or non-HTTP name refuses the run closed with the
/// honest `E-PUB-REGISTRY` decline — zero probes, zero spawns
/// (validate-then-act, §15.3).
///
/// # Errors
///
/// [`PublishError::Registry`] when a target's registry resolves to
/// nothing probeable (an unconfigured cargo name, a git-protocol
/// index, a malformed cargo config, a non-HTTP npm base) — the message
/// names the registry/package and the fix.
pub fn publish_router(
    repo_root: &Path,
    targets: &[PublishTarget],
) -> Result<TargetRouter, PublishError> {
    let transport: Arc<dyn halyard_forge::transport::HttpTransport> =
        Arc::new(UreqTransport::new());
    let mut router = TargetRouter::new()
        .with("crates.io", Arc::new(CratesIo::new(Arc::clone(&transport))))
        .with("npm", Arc::new(Npm::new(Arc::clone(&transport))))
        // M8 step 2 (ADR-0056): the PyPI JSON-API probe joins the
        // default routers under the plan contract's `pypi` registry
        // name (the resume/visibility half of the acceptance row).
        .with("pypi", Arc::new(PyPi::new(Arc::clone(&transport))));
    // Distinct target registry names, sorted (I3): the resolution reads
    // local files only, so the iteration order cannot change outcomes —
    // but a deterministic decline order keeps the refusal message
    // byte-stable across runs.
    let mut names: Vec<&str> = targets
        .iter()
        .map(|target| target.registry.as_str())
        .collect();
    names.sort_unstable();
    names.dedup();
    let config_paths = halyard_eco_cargo::registry_config::cargo_config_paths(repo_root);
    for name in names {
        if halyard_eco_cargo::is_default_registry(name) || name == "npm" || name == "pypi" {
            continue;
        }
        match halyard_eco_cargo::registry_config::registry_index(name, &config_paths) {
            halyard_eco_cargo::registry_config::RegistryIndexLookup::Sparse(base) => {
                let client = CratesIo::with_index_base(Arc::clone(&transport), &base);
                router = router.with(name, Arc::new(client));
            }
            halyard_eco_cargo::registry_config::RegistryIndexLookup::Unsupported(message) => {
                return Err(PublishError::Registry {
                    registry: name.to_owned(),
                    message,
                });
            }
            halyard_eco_cargo::registry_config::RegistryIndexLookup::Unknown => {
                let fragment = halyard_eco_cargo::registry_config::registry_env_fragment(name);
                return Err(PublishError::Registry {
                    registry: name.to_owned(),
                    message: format!(
                        "no `[registries.{name}]` index in the cargo configuration \
                         (.cargo/config.toml or {fragment}_INDEX) — configure the registry's \
                         sparse+ index URL, or disable publishing for its packages"
                    ),
                });
            }
        }
    }
    // The npm side routes per package (ADR-0021): the manifest's
    // publishConfig and the .npmrc hierarchy name the registry the
    // spawned `pnpm publish` targets, so the probe resolves the same
    // way (targets sorted by name — the plan's own order).
    let mut npm_targets: Vec<&PublishTarget> = targets
        .iter()
        .filter(|target| target.ecosystem == "npm")
        .collect();
    npm_targets.sort_unstable_by(|a, b| a.package.cmp(&b.package));
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let empty = std::path::PathBuf::new();
    for target in npm_targets {
        let lookup = halyard_eco_npm::publish_registry::publish_registry_base(
            repo_root,
            home.as_deref().unwrap_or(&empty),
            &target.package,
            &target.manifest,
        );
        match lookup {
            halyard_eco_npm::publish_registry::PublishRegistryBase::Default => {}
            halyard_eco_npm::publish_registry::PublishRegistryBase::Custom(base) => {
                let client = Npm::with_registry_base(Arc::clone(&transport), &base);
                router = router.with_override(&target.registry, &target.package, Arc::new(client));
            }
            halyard_eco_npm::publish_registry::PublishRegistryBase::Unsupported(message) => {
                return Err(PublishError::Registry {
                    registry: target.registry.clone(),
                    message: format!(
                        "{message} (package {}, manifest {})",
                        target.package, target.manifest
                    ),
                });
            }
        }
    }
    Ok(router)
}

/// The production sleeper pair for the §13 step-3 visibility wait (the
/// ADR-0015 injected-clock convention: tests script theirs; the shell
/// owns the wall clock).
pub struct ThreadSleeper;

impl RegistrySleeper for ThreadSleeper {
    fn sleep(&self, delay: Duration) {
        std::thread::sleep(delay);
    }
}

#[cfg(test)]
mod pypi_router_tests {
    use super::*;
    use halyard_publish::{PublishTarget, RegistryRouter};

    /// The pypi target's registry (`pypi`) routes to the JSON-API probe
    /// client (M8 step 2, ADR-0056): the default router registration
    /// keeps the resume/visibility half of the M8 acceptance row
    /// offline-testable (the registration, not the wire — the wire is
    /// the `halyard_registry` contract suite's job).
    #[test]
    fn the_pypi_registry_name_routes_to_the_json_api_probe() {
        let dir = std::env::temp_dir().join(format!("pypi-router-{}", std::process::id()));
        let target = PublishTarget {
            package: "p1".to_owned(),
            version: "0.1.1".to_owned(),
            ecosystem: "pypi".to_owned(),
            registry: "pypi".to_owned(),
            manifest_dir: String::new(),
            manifest: "pyproject.toml".to_owned(),
            access: None,
            provenance: None,
        };
        let router = publish_router(&dir, &[target]).expect("router builds");
        assert!(router.registry("pypi").is_ok(), "pypi routes");
        // An unknown cargo name still refuses (the skip-list covers the
        // default names only — validate-then-act, §15.3).
        let unknown = PublishTarget {
            registry: "not-a-registry".to_owned(),
            ..PublishTarget {
                package: "p1".to_owned(),
                version: "0.1.1".to_owned(),
                ecosystem: "cargo".to_owned(),
                registry: "not-a-registry".to_owned(),
                manifest_dir: String::new(),
                manifest: "Cargo.toml".to_owned(),
                access: None,
                provenance: None,
            }
        };
        let error = publish_router(&dir, &[unknown]).expect_err("unknown refuses");
        assert_eq!(error.code(), "E-PUB-REGISTRY");
    }
}
