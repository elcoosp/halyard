//! Environment diagnostics (§16.2 `doctor`): shallow clone, commit-graph
//! presence, release tags, `halyard.toml` validity and (unless
//! `--offline`) the forge token. The report is a stable data structure —
//! snapshot-tested per case, rendered by `render`.

use halyard_core::HistoryProvider;
use halyard_git::GitHistory;

use crate::Cli;
use crate::CliError;

/// One doctor check.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Check {
    /// Stable check name (`shallow`, `commit-graph`, `tags`, `config`,
    /// `token`, `forge-governor`).
    pub name: String,
    /// True when the environment is fine.
    pub ok: bool,
    /// Human detail (includes the fix on failure).
    pub detail: String,
}

/// The full report (rendered by `render::doctor_*`).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct DoctorReport {
    /// Checks in stable order.
    pub checks: Vec<Check>,
    /// Non-fatal advisories (e.g. discovery warnings).
    pub warnings: Vec<String>,
    /// True when every check passed.
    pub healthy: bool,
}

/// Runs every check against the current directory. `registries` opts in
/// to the §13 registry reachability probes (M5.1) — network touched
/// only by the flag (I5; doctor itself never contacts the network).
pub fn check(
    cli: &Cli,
    profiler: &mut crate::profile::Profiler,
    registries: bool,
) -> Result<DoctorReport, CliError> {
    let mut checks: Vec<Check> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Discovery (also produces the advisories).
    let discovery = profiler.span("doctor-discover", || {
        halyard_discover::discover_repo(
            std::path::Path::new("."),
            &halyard_discover::DiscoverInput::default(),
        )
    });
    let discovery = discovery.map_err(|e| CliError::new(e.code(), e.to_string()))?;
    for warning in &discovery.warnings {
        warnings.push(format!("[{}] {}", warning.code, warning.message));
    }

    // Shallow + commit-graph + tags + head: one git probe each.
    let repo = halyard_git::gix::open(std::path::Path::new("."))
        .map_err(|e| CliError::new("E-GIT-OPEN", format!("doctor needs a git repository: {e}")))?;
    let shallow =
        repo.git_dir().join("shallow").is_file() || repo.common_dir().join("shallow").is_file();
    checks.push(Check {
        name: "shallow".to_owned(),
        ok: !shallow,
        detail: if shallow {
            "shallow clone: history is truncated; run `git fetch --unshallow` or `halyard fetch` \
             (see E-GIT-SHALLOW)"
                .to_owned()
        } else {
            "full clone".to_owned()
        },
    });

    let graph = repo.commit_graph_if_enabled().ok().flatten();
    let has_graph = graph.is_some();
    checks.push(Check {
        name: "commit-graph".to_owned(),
        ok: has_graph,
        detail: if has_graph {
            "commit-graph present: walks use generation numbers".to_owned()
        } else {
            "no commit-graph: walks fall back to a memoized DFS; run `git commit-graph write \
             --reachable --changed-paths` to speed planning up"
                .to_owned()
        },
    });

    let tag_count = repo.references().map_or(0, |platform| {
        platform
            .tags()
            .map_or(0, |tags| tags.filter_map(Result::ok).count())
    });
    checks.push(Check {
        name: "tags".to_owned(),
        ok: tag_count > 0,
        detail: if tag_count == 0 {
            "no tags: every package is an initial release; create release tags or accept \
             initial-release planning"
                .to_owned()
        } else {
            format!("{tag_count} tag(s) enumerated")
        },
    });

    // Config validity (defaults count as valid).
    checks.push(config_check(cli));

    // Token presence (skipped under --offline, I5: doctor itself never
    // contacts the network).
    checks.push(token_check(cli.offline));

    // The §11.5 call governor's resolved policy (M5.5): the knobs the
    // live forge paths (`pr`, `release`, `forge-smoke`) will run under —
    // visible before the run, not after the first throttle. An invalid
    // `HALYARD_FORGE_*` value fails the check naming the variable.
    checks.push(governor_check());

    // The publish-auth resolution (M5.3 step 2, ADR-0019): what the
    // `publish` spawn will authenticate with, per default registry —
    // provenance only (variable names, never values). Absence is
    // honest (the tools' own credential stores govern); a
    // present-but-empty variable fails the check with the seam's
    // `E-PUB-AUTH` diagnosis before any run does.
    checks.push(publish_auth_check());
    checks.push(publish_oidc_check());
    // The npm provenance-runtime verdict (ADR-0033): which supported
    // cloud CI runtime was detected — the gate an explicit
    // `publish.npm.provenance = true` live npm publish runs under.
    // Detection is information, never a failure: absence is `ok` with
    // the consequence named.
    checks.push(publish_provenance_check());

    // History engine smoke check: the walk must succeed on this repo
    // (this is the doctor's cheapest "will plan() work" signal).
    let history = profiler.span("doctor-history", || {
        GitHistory::build(
            std::path::Path::new("."),
            &discovery.workspace,
            halyard_config::resolve(&[halyard_config::Layer::Defaults])
                .map_or(10_000, |c| c.initial_max_commits),
        )
    });
    match history {
        Ok(history) => checks.push(Check {
            name: "history".to_owned(),
            ok: true,
            detail: format!(
                "walk ok: {} unreleased commit(s); head {}",
                history.digest().commits.len(),
                history
                    .head_oid()
                    .map(|o| o.to_string())
                    .unwrap_or_default()
            ),
        }),
        Err(e) => checks.push(Check {
            name: "history".to_owned(),
            ok: false,
            detail: format!("{}: {}", e.code(), e),
        }),
    }

    // The opt-in §13 registry probes (M5.1): one immutable-version
    // `published()` probe per default registry — reachability AND
    // protocol comprehension (the answer must parse), never a mutation.
    if registries {
        checks.extend(registry_checks());
    }

    let healthy = checks.iter().all(|c| c.ok);
    Ok(DoctorReport {
        checks,
        warnings,
        healthy,
    })
}

/// The §13 registry reachability probes (`doctor --registries`, M5.1):
/// the [`halyard_registry::Registry`] seam over the runtime transport,
/// probing `serde@1.0.0` / `react@0.0.1` — versions published long
/// before this code existed and immutable by the registries' own
/// contract (§13: "the version can never be overwritten"), so the
/// probes are stable and read-only. A failing probe fails the check
/// with its `E-REG-*` code; the check verifies reachability and
/// protocol comprehension, not the package (the visible flag is
/// rendered either way).
fn registry_checks() -> Vec<Check> {
    let transport: std::sync::Arc<dyn halyard_forge::transport::HttpTransport> =
        std::sync::Arc::new(halyard_forge_github::UreqTransport::new());
    vec![
        registry_check(
            "registry-crates-io",
            "serde",
            "1.0.0",
            &halyard_registry::CratesIo::new(std::sync::Arc::clone(&transport)),
        ),
        registry_check(
            "registry-npm",
            "react",
            "0.0.1",
            &halyard_registry::Npm::new(transport),
        ),
    ]
}

/// One registry probe rendered as a doctor [`Check`].
fn registry_check<R: halyard_registry::Registry + ?Sized>(
    name: &str,
    package: &str,
    version: &str,
    registry: &R,
) -> Check {
    match registry.published(package, version) {
        Ok(visible) => Check {
            name: name.to_owned(),
            ok: true,
            detail: format!(
                "{package}@{version} probe answered (visible: {visible}) — reachable and \
                 protocol-comprehensible"
            ),
        },
        Err(error) => Check {
            name: name.to_owned(),
            ok: false,
            detail: format!("{}: {error}", error.code()),
        },
    }
}

/// The `halyard.toml` validity check (defaults count as valid).
fn config_check(cli: &Cli) -> Check {
    let config_path = cli.config.clone().or_else(|| {
        let root = std::path::PathBuf::from("halyard.toml");
        let dotted = std::path::PathBuf::from(".config/halyard.toml");
        if root.is_file() {
            Some(root)
        } else if dotted.is_file() {
            Some(dotted)
        } else {
            None
        }
    });
    match config_path {
        Some(path) => match halyard_config::load(&path) {
            Ok(_) => Check {
                name: "config".to_owned(),
                ok: true,
                detail: format!("{} parsed", path.display()),
            },
            Err(e) => Check {
                name: "config".to_owned(),
                ok: false,
                detail: format!("{}: {} ({})", path.display(), e, e.code()),
            },
        },
        None => Check {
            name: "config".to_owned(),
            ok: true,
            detail: "no halyard.toml; built-in defaults apply".to_owned(),
        },
    }
}

/// The §11.5 call governor's resolved-policy check (M5.5): reports the
/// policy the live forge paths will run under (shell default with the
/// `HALYARD_FORGE_*` overrides applied). An invalid value fails the
/// check naming the variable and the accepted shape — a diagnosis, the
/// same failure the live paths themselves raise (`E-FORGE-REQUEST`).
fn governor_check() -> Check {
    match crate::governor_policy_from_env() {
        Ok(policy) => {
            let budget = policy
                .max_calls
                .map_or_else(|| "unbounded".to_owned(), |budget| budget.to_string());
            Check {
                name: "forge-governor".to_owned(),
                ok: true,
                detail: format!(
                    "§11.5 call governor: pacing {} ms · budget {budget} · retries {} \
                     (override with HALYARD_FORGE_MIN_CALL_SPACING_MS / \
                     HALYARD_FORGE_MAX_CALLS / HALYARD_FORGE_MAX_RETRIES)",
                    policy.min_spacing_ms, policy.max_retries,
                ),
            }
        }
        Err(error) => Check {
            name: "forge-governor".to_owned(),
            ok: false,
            detail: error.to_string(),
        },
    }
}

/// The publish-auth resolution check (M5.3 step 2, ADR-0019): the same
/// capture the `publish` run performs, over the two default §13
/// registries — which documented variable authenticates each one
/// (`env:NAME`) or `none` (nothing injected; `cargo login` / the
/// user-level auth file govern, §13.4's "the tools own credentials").
/// A present-but-empty token variable is the seam's `E-PUB-AUTH`
/// misconfiguration and fails the check; absence is `ok` (an honest
/// value, never guessed).
fn publish_auth_check() -> Check {
    let auth = match halyard_publish::EnvPublishAuth::capture() {
        Ok(auth) => auth,
        Err(error) => {
            return Check {
                name: "publish-auth".to_owned(),
                ok: false,
                detail: error.to_string(),
            };
        }
    };
    let targets = [
        halyard_publish::PublishTarget {
            package: String::new(),
            version: String::new(),
            ecosystem: "cargo".to_owned(),
            registry: "crates.io".to_owned(),
            manifest_dir: String::new(),
            manifest: String::new(),
            access: None,
            provenance: None,
        },
        halyard_publish::PublishTarget {
            package: String::new(),
            version: String::new(),
            ecosystem: "npm".to_owned(),
            registry: "npm".to_owned(),
            manifest_dir: String::new(),
            manifest: String::new(),
            access: None,
            provenance: None,
        },
    ];
    match halyard_publish::auth_summary(&auth, &targets) {
        Ok(summary) => {
            let pairs: Vec<String> = summary
                .iter()
                .map(|(registry, provenance)| format!("{registry} = {provenance}"))
                .collect();
            // The grammar is data (ADR-0019): the documented candidate
            // variables per registry, in resolution order — what would
            // resolve, not only what resolved.
            let candidates: Vec<String> = targets
                .iter()
                .map(|target| {
                    format!(
                        "{} candidates {:?}",
                        target.registry,
                        halyard_publish::variables_for(target)
                    )
                })
                .collect();
            Check {
                name: "publish-auth".to_owned(),
                ok: true,
                detail: format!(
                    "publish spawn credentials (values never logged; a set-but-empty variable \
                     fails E-PUB-AUTH): {}; {}",
                    pairs.join(", "),
                    candidates.join("; ")
                ),
            }
        }
        Err(error) => Check {
            name: "publish-auth".to_owned(),
            ok: false,
            detail: error.to_string(),
        },
    }
}

/// The publish-oidc context check (M5.3 step 3, ADR-0031): whether the
/// GitHub Actions OIDC runtime context is present (`id-token: write` —
/// the runner exposes both documented variables). Presence is
/// informational (`ok` — the exchange fires only in a live publish
/// when the env grammar covers nothing); absence is the normal shape
/// outside GitHub Actions; a half-configured or empty context is the
/// seam's `E-PUB-AUTH` misconfiguration and fails the check. Read-only
/// over the process environment — no network, no token is ever logged.
fn publish_oidc_check() -> Check {
    match halyard_publish::gha_oidc_context(&|name| std::env::var(name).ok()) {
        Ok(Some(context)) => Check {
            name: "publish-oidc".to_owned(),
            ok: true,
            detail: format!(
                "GitHub Actions OIDC context present ({context}) — a live publish exchanges \
                 the crates.io token when no documented variable covers the registry"
            ),
        },
        Ok(None) => Check {
            name: "publish-oidc".to_owned(),
            ok: true,
            detail: "no GitHub Actions OIDC context (the normal shape outside Actions) — the \
                     documented env grammar and the tools' own stores govern"
                .to_owned(),
        },
        Err(error) => Check {
            name: "publish-oidc".to_owned(),
            ok: false,
            detail: error.to_string(),
        },
    }
}

/// The npm provenance-runtime check (ADR-0033): which supported cloud
/// CI runtime was detected — the GitHub Actions OIDC context (npm's
/// documented `id-token: write` must) or the GitLab CI/CD marker
/// (`GITLAB_CI`, VERIFIED) — versus the laptop shape. Detection is
/// information, never a failure: absence is `ok` with the gate's
/// consequence named, so a `publish.npm.provenance = true` workspace
/// sees the refusal before the run does.
fn publish_provenance_check() -> Check {
    let reader = |name: &str| std::env::var(name).ok();
    let oidc_present =
        halyard_publish::gha_oidc_context(&reader).is_ok_and(|context| context.is_some());
    let runtime = halyard_publish::provenance_runtime_detected(oidc_present, &reader);
    match runtime {
        halyard_publish::ProvenanceRuntime::CloudDetected => {
            let source = if oidc_present {
                "GitHub Actions OIDC context"
            } else {
                "GitLab CI/CD markers"
            };
            Check {
                name: "publish-provenance".to_owned(),
                ok: true,
                detail: format!(
                    "supported cloud runtime detected ({source}) — an explicit \
                     `publish.npm.provenance = true` live npm publish may run"
                ),
            }
        }
        halyard_publish::ProvenanceRuntime::NotDetected => Check {
            name: "publish-provenance".to_owned(),
            ok: true,
            detail: "no supported cloud runtime detected (the laptop shape) — an explicit \
                     `publish.npm.provenance = true` live npm publish refuses (E-PUB-AUTH); run \
                     on a hosted runner with `id-token: write`, set `provenance = false`, or \
                     leave the key unset to let trusted publishing decide"
                .to_owned(),
        },
    }
}

/// The forge-credentials presence check (skipped under `--offline`):
/// the GitHub App env (ADR-0014) or a token — either authenticates the
/// `pr`/`release` forge halves.
fn token_check(offline: bool) -> Check {
    if offline {
        return Check {
            name: "token".to_owned(),
            ok: true,
            detail: "--offline: forge credentials check skipped".to_owned(),
        };
    }
    let app_set = [
        "HALYARD_APP_ID",
        "HALYARD_APP_INSTALLATION_ID",
        "HALYARD_APP_PRIVATE_KEY",
        "HALYARD_APP_PRIVATE_KEY_PATH",
    ]
    .iter()
    .any(|name| std::env::var(name).is_ok_and(|value| !value.is_empty()));
    if app_set {
        let key_source = std::env::var("HALYARD_APP_PRIVATE_KEY")
            .ok()
            .filter(|value| !value.is_empty())
            .map(|_| "HALYARD_APP_PRIVATE_KEY".to_owned())
            .or_else(|| {
                std::env::var("HALYARD_APP_PRIVATE_KEY_PATH")
                    .ok()
                    .filter(|value| !value.is_empty())
                    .map(|path| format!("key at {path}"))
            })
            .unwrap_or_else(|| "no private key set".to_owned());
        return Check {
            name: "token".to_owned(),
            ok: true,
            detail: format!(
                "GitHub App env present (App-JWT → installation-token auth, ADR-0014; \
                 {key_source} — forge actions trigger the repository's workflows)"
            ),
        };
    }
    let token = std::env::var("GITHUB_TOKEN").ok().filter(|t| !t.is_empty());
    Check {
        name: "token".to_owned(),
        ok: token.is_some(),
        detail: match token {
            Some(_) => "GITHUB_TOKEN present (release/publish steps can authenticate; \
                        note: forge actions taken with it do not trigger the \
                        repository's workflows — the GitHub App env does)"
                .to_owned(),
            None => "no forge credentials: planning works offline, but `pr`/`release`/\
                     `publish` need the GitHub App env (HALYARD_APP_ID + \
                     HALYARD_APP_PRIVATE_KEY_PATH + HALYARD_APP_INSTALLATION_ID) or \
                     GITHUB_TOKEN (or run with --offline to silence this check)"
                .to_owned(),
        },
    }
}
