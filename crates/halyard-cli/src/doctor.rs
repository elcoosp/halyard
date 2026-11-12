//! Environment diagnostics (§16.2 `doctor`): shallow clone, commit-graph
//! presence, release tags, `halyard.toml` validity and (unless
//! `--offline`) the forge token. The report is a stable data structure —
//! snapshot-tested per case, rendered by `render`.

use halyard_core::HistoryProvider;
use halyard_git::GitHistory;

use crate::CliError;
use crate::Cli;

/// One doctor check.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Check {
    /// Stable check name (`shallow`, `commit-graph`, `tags`, `config`,
    /// `token`).
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

/// Runs every check against the current directory.
pub fn check(cli: &Cli, profiler: &mut crate::profile::Profiler) -> Result<DoctorReport, CliError> {
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
    let repo = halyard_git::gix::open(std::path::Path::new(".")).map_err(|e| {
        CliError::new(
            "E-GIT-OPEN",
            format!("doctor needs a git repository: {e}"),
        )
    })?;
    let shallow = repo.git_dir().join("shallow").is_file()
        || repo.common_dir().join("shallow").is_file();
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

    let tag_count = repo
        .references()
        .map(|platform| {
            platform
                .tags()
                .map(|tags| tags.filter_map(Result::ok).count())
                .unwrap_or(0)
        })
        .unwrap_or(0);
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
        Some(path) => {
            match halyard_config::load(&path) {
                Ok(_) => checks.push(Check {
                    name: "config".to_owned(),
                    ok: true,
                    detail: format!("{} parsed", path.display()),
                }),
                Err(e) => checks.push(Check {
                    name: "config".to_owned(),
                    ok: false,
                    detail: format!("{}: {} ({})", path.display(), e, e.code()),
                }),
            }
        }
        None => checks.push(Check {
            name: "config".to_owned(),
            ok: true,
            detail: "no halyard.toml; built-in defaults apply".to_owned(),
        }),
    }

    // Token presence (skipped under --offline, I5: doctor itself never
    // contacts the network).
    if cli.offline {
        checks.push(Check {
            name: "token".to_owned(),
            ok: true,
            detail: "--offline: forge token check skipped".to_owned(),
        });
    } else {
        let token = std::env::var("GITHUB_TOKEN")
            .ok()
            .filter(|t| !t.is_empty());
        checks.push(Check {
            name: "token".to_owned(),
            ok: token.is_some(),
            detail: match token {
                Some(_) => "GITHUB_TOKEN present (release/publish steps can authenticate)".to_owned(),
                None => "GITHUB_TOKEN not set: planning works offline, but `pr`/`release`/\
                         `publish` need a token (or run with --offline to silence this check)"
                    .to_owned(),
            },
        });
    }

    // History engine smoke check: the walk must succeed on this repo
    // (this is the doctor's cheapest "will plan() work" signal).
    let history = profiler.span("doctor-history", || {
        GitHistory::build(
            std::path::Path::new("."),
            &discovery.workspace,
            halyard_config::resolve(&[
                halyard_config::Layer::Defaults,
            ])
            .map(|c| c.initial_max_commits)
            .unwrap_or(10_000),
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

    let healthy = checks.iter().all(|c| c.ok);
    Ok(DoctorReport {
        checks,
        warnings,
        healthy,
    })
}
