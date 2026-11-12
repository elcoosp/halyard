//! `discover-diff` (M1.5/M1.6 differential gate): run our discovery on
//! every fetched corpus repo and compare against the package managers'
//! own output.
//!
//! * Cargo repos vs `cargo metadata --no-deps --format-version 1`
//!   (members, versions, internal edge kinds).
//! * npm/pnpm repos vs `pnpm m ls --json --depth=-1` (members, versions,
//!   internal edges via `link:` resolution; requires `pnpm` on PATH).
//!
//! Repos whose clones are not fetched (or pnpm missing) are reported as
//! `SKIP` — the gate never fabricates a pass, and CI (nightly differential
//! lane) is where fetched-corpus runs happen.

use std::path::{Path, PathBuf};
use std::process::Command;

use halyard_discover::DiscoverInput;
use halyard_test_support::manifest_fixtures::{
    cargo_metadata_expectation, workspace_expectation,
};
use serde_json::Value;

/// A corpus repo row (subset of `corpus/manifest.toml`).
struct CorpusRepo {
    name: String,
    ecosystems: Vec<String>,
}

/// Loads the corpus manifest rows.
fn corpus_repos(root: &Path) -> Result<Vec<CorpusRepo>, String> {
    let manifest = std::fs::read_to_string(root.join("corpus/manifest.toml"))
        .map_err(|e| format!("read corpus/manifest.toml: {e}"))?;
    let value: Value = toml::from_str(&manifest)
        .map_err(|e| format!("parse corpus/manifest.toml: {e}"))?;
    let repos = value
        .get("repo")
        .and_then(Value::as_array)
        .ok_or_else(|| "corpus/manifest.toml: no [[repo]] rows".to_owned())?;
    let mut out = Vec::new();
    for repo in repos {
        out.push(CorpusRepo {
            name: repo
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            ecosystems: repo
                .get("ecosystems")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
        });
    }
    Ok(out)
}

/// Entry point for `xtask discover-diff --corpus <cargo|npm> [--repo N]`.
pub fn discover_diff(args: &[String]) -> Result<(), String> {
    let mut which = String::new();
    let mut only: Option<String> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--corpus" => {
                which = iter.next().cloned().ok_or("--corpus needs a value")?;
            }
            "--repo" => {
                only = Some(iter.next().cloned().ok_or("--repo needs a value")?);
            }
            other => return Err(format!("unknown flag {other:?} (use --corpus cargo|npm)")),
        }
    }
    if which != "cargo" && which != "npm" {
        return Err("--corpus must be `cargo` or `npm`".to_owned());
    }

    let repo_root = repo_root()?;
    let repos = corpus_repos(&repo_root)?;
    let clones_dir = repo_root.join("corpus/clones");
    let mut ok = 0usize;
    let mut diff = 0usize;
    let mut skipped = 0usize;
    let mut missing_tool: Option<String> = None;

    for repo in repos {
        if let Some(name) = &only {
            if &repo.name != name {
                continue;
            }
        }
        let wanted = repo.ecosystems.iter().any(|eco| eco == &which);
        if !wanted {
            continue;
        }
        let clone = clones_dir.join(&repo.name);
        if !clone.join(".git").exists() {
            println!("SKIP  {} (corpus not fetched)", repo.name);
            skipped += 1;
            continue;
        }
        if which == "npm" && !tool_available("pnpm") {
            missing_tool = Some("pnpm".to_owned());
            println!("SKIP  {} (pnpm not installed)", repo.name);
            skipped += 1;
            continue;
        }
        let outcome = if which == "cargo" {
            diff_cargo(&repo.name, &clone)
        } else {
            diff_npm(&repo.name, &clone)
        };
        match outcome {
            Ok(()) => {
                println!("OK    {}", repo.name);
                ok += 1;
            }
            Err(report) => {
                println!("DIFF  {}\n      {report}", repo.name);
                diff += 1;
            }
        }
    }

    if missing_tool == Some("pnpm".to_owned()) && ok == 0 && diff == 0 {
        println!("SKIP: corpus not fetched or pnpm not installed — deferred to CI");
    }
    println!(
        "discover-diff({which}): ok={ok} diff={diff} skip={skipped}"
    );
    if diff > 0 {
        Err(format!("{diff} corpus repos differ from package-manager ground truth"))
    } else {
        Ok(())
    }
}

/// Compares one cargo repo against `cargo metadata`.
fn diff_cargo(name: &str, clone: &Path) -> Result<(), String> {
    let discovery = halyard_discover::discover_repo(clone, &DiscoverInput::default())
        .map_err(|e| format!("our discovery failed: {e} ({name})"))?;
    let expected = cargo_metadata_expectation(clone)
        .map_err(|e| format!("cargo metadata failed: {e} ({name})"))?;
    let ours = workspace_expectation(&discovery.workspace);
    if ours == expected {
        Ok(())
    } else {
        let mut report = String::new();
        let member_diff: Vec<_> = expected
            .members
            .symmetric_difference(&ours.members)
            .collect();
        if !member_diff.is_empty() {
            report.push_str(&format!("members: {member_diff:?} "));
        }
        let edge_diff: Vec<_> = expected.edges.symmetric_difference(&ours.edges).collect();
        if !edge_diff.is_empty() {
            report.push_str(&format!("edges: {edge_diff:?}"));
        }
        Err(report)
    }
}

/// Compares one npm/pnpm repo against `pnpm m ls --json --depth=-1`.
fn diff_npm(name: &str, clone: &Path) -> Result<(), String> {
    let output = Command::new("pnpm")
        .args(["m", "ls", "--json", "--depth=-1"])
        .current_dir(clone)
        .output()
        .map_err(|e| format!("spawn pnpm: {e} ({name})"))?;
    if !output.status.success() {
        return Err(format!(
            "pnpm m ls failed: {} ({name})",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("parse pnpm output: {e} ({name})"))?;
    let discovery = halyard_discover::discover_repo(clone, &DiscoverInput::default())
        .map_err(|e| format!("our discovery failed: {e} ({name})"))?;

    // Defensive parse: pnpm prints an array of workspace projects.
    let projects = value
        .as_array()
        .ok_or_else(|| format!("pnpm output is not an array ({name})"))?;
    let expected_members: std::collections::BTreeSet<(String, String)> = projects
        .iter()
        .filter_map(|p| {
            let pkg = p.as_object()?;
            let name = pkg.get("name")?.as_str()?.to_owned();
            let version = pkg
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or("0.0.0")
                .to_owned();
            Some((name, version))
        })
        .collect();
    let interner = discovery.workspace.interner();
    let ours_members: std::collections::BTreeSet<(String, String)> = discovery
        .workspace
        .packages()
        .iter()
        .map(|p| {
            (
                interner.get(p.name).unwrap_or_default().to_owned(),
                p.version
                    .as_ref()
                    .map_or_else(|| "0.0.0".to_owned(), |v| v.to_string()),
            )
        })
        .collect();
    if ours_members != expected_members {
        let only_expected: Vec<_> =
            expected_members.difference(&ours_members).collect();
        let only_ours: Vec<_> = ours_members.difference(&expected_members).collect();
        return Err(format!(
            "members differ; pnpm-only={only_expected:?} ours-only={only_ours:?}"
        ));
    }
    // Internal edges: pnpm renders workspace links as `link:<rel-path>`.
    let members_by_path: std::collections::BTreeMap<String, String> = discovery
        .workspace
        .packages()
        .iter()
        .map(|p| (p.root.as_str().to_owned(), interner.get(p.name).unwrap_or_default().to_owned()))
        .collect();
    let expected_edges: std::collections::BTreeSet<(String, String)> = projects
        .iter()
        .filter_map(|p| {
            let pkg = p.as_object()?;
            let from = pkg.get("name")?.as_str()?.to_owned();
            let project_path = pkg
                .get("path")
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .unwrap_or_else(|| clone.to_path_buf());
            let deps = pkg.get("dependencies")?.as_object()?;
            let mut edges = Vec::new();
            for (_dep_name, spec) in deps {
                let raw = spec.as_str().unwrap_or_default();
                let Some(rel) = raw.strip_prefix("link:") else {
                    continue;
                };
                let target_dir = project_path.join(rel);
                if let Ok(canonical) = std::fs::canonicalize(&target_dir) {
                    if let Ok(rel_to_root) = canonical.strip_prefix(clone) {
                        let rel_path = rel_to_root.to_string_lossy().replace('\\', "/");
                        if let Some(to) = members_by_path.get(&rel_path) {
                            edges.push((from.clone(), to.clone()));
                        }
                    }
                }
            }
            Some(edges)
        })
        .flatten()
        .collect();
    let ours_edges: std::collections::BTreeSet<(String, String)> = discovery
        .workspace
        .packages()
        .iter()
        .filter_map(|p| {
            let from = interner.get(p.name)?.to_owned();
            Some(
                p.deps
                    .iter()
                    .filter_map(|d| {
                        discovery
                            .workspace
                            .package(d.to)
                            .and_then(|t| interner.get(t.name))
                            .map(|n| (from.clone(), n.to_owned()))
                    })
                    .collect::<Vec<(String, String)>>(),
            )
        })
        .flatten()
        .collect();
    if ours_edges != expected_edges {
        return Err(format!(
            "internal edges differ; pnpm-only={:?} ours-only={:?}",
            expected_edges.difference(&ours_edges).collect::<Vec<_>>(),
            ours_edges.difference(&expected_edges).collect::<Vec<_>>(),
        ));
    }
    Ok(())
}

/// True when the tool answers `--version`.
fn tool_available(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Locates the repository root (walks up for `corpus/manifest.toml`).
fn repo_root() -> Result<PathBuf, String> {
    let mut dir = std::env::current_dir().map_err(|e| format!("current_dir: {e}"))?;
    loop {
        if dir.join("corpus/manifest.toml").exists() {
            return Ok(dir);
        }
        if !dir.pop() {
            return Err("corpus/manifest.toml not found from cwd upward".to_owned());
        }
    }
}
