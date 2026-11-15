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
    AuthOutcome, MapRouter, PublishAuth, PublishEffect, PublishError, PublishTarget, scrub_secrets,
};
use halyard_registry::{CratesIo, Npm, RegistrySleeper};

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
                let args =
                    halyard_eco_cargo::cargo_publish_args(&manifest.to_string_lossy(), self.dry);
                ("cargo", args)
            }
            // §13.4 names `pnpm publish` as the npm-side tool; a
            // plain-npm repository without pnpm fails honestly rather
            // than switching to a different tool with different
            // packaging semantics (see the research note).
            "npm" => ("pnpm", halyard_eco_npm::pnpm_publish_args(self.dry)),
            other => {
                return Err(PublishError::Effect {
                    ecosystem: target.ecosystem.clone(),
                    message: format!(
                        "no publish tool grammar for ecosystem {other:?} — the adapter tables \
                         cover cargo and npm (§13.4); extending one is an ADR"
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
/// before any call, so an unused client costs nothing).
#[must_use]
pub fn publish_router() -> MapRouter {
    let transport: Arc<dyn halyard_forge::transport::HttpTransport> =
        Arc::new(UreqTransport::new());
    MapRouter::new()
        .with("crates.io", Arc::new(CratesIo::new(Arc::clone(&transport))))
        .with("npm", Arc::new(Npm::new(transport)))
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
