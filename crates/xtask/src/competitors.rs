//! `bench-competitors` (M0.3): competitor benchmark harness.
//!
//! Runs each competitor release tool against a corpus fixture inside a
//! disposable working copy, measures wall time and peak RSS, diffs the tags
//! created, and records one JSON result per (tool, fixture) under
//! `bench/competitors/results/`. `--report` aggregates results plus the
//! §1.1 claim table into `bench/competitors/REPORT.md`.
//!
//! Tool invocations come from `bench/competitors/tools.toml`; behaviour
//! claims are **never** invented here — anything not measured is reported
//! as `pending`, and CLI-surface observations live in
//! `docs/research/competitor-cli-surfaces.md` (M0.6).

use std::process::Command;
use std::time::Instant;

use serde::Deserialize;

/// A competitor tool invocation recipe.
#[derive(Debug, Deserialize)]
struct Tool {
    /// Tool name as reported in results.
    name: String,
    /// Binary to invoke.
    binary: String,
    /// Arguments for a version probe (used to record the tool version).
    version_args: Vec<String>,
    /// Arguments for the benchmarked run; `{fixture}` is substituted with
    /// the working-copy path.
    bench_args: Vec<String>,
    /// Environment variables required for an offline run.
    #[serde(default)]
    env: Vec<String>,
    /// Config files written into the working copy before the run, as
    /// `(path, content)` — e.g. a `knope.toml` captured from
    /// `knope --generate` (see docs/research/competitor-cli-surfaces.md).
    #[serde(default)]
    prepare_files: Vec<(String, String)>,
    /// True when the tool cannot produce a meaningful result without forge
    /// API access; such tools are skipped unless `--allow-network` is set.
    #[serde(default)]
    needs_network: bool,
}

#[derive(Debug, Deserialize)]
struct Tools {
    /// Tool recipes.
    #[serde(default)]
    tool: Vec<Tool>,
}

/// One claim from §1.1 (hypotheses to verify by measurement).
#[derive(Debug, Deserialize)]
struct Claim {
    /// Tool the claim is about.
    tool: String,
    /// The claim text (from §1.1).
    claim: String,
}

#[derive(Debug, Deserialize)]
struct Claims {
    /// Claim rows.
    #[serde(default)]
    claim: Vec<Claim>,
}

/// Measured run, serialized to `results/<tool>.<fixture>.json`.
#[derive(Debug, serde::Serialize)]
struct RunResult {
    /// Tool name.
    tool: String,
    /// Tool version string (first line of `version_args` output).
    version: String,
    /// Fixture directory the run was executed on.
    fixture: String,
    /// Wall-clock duration of the run in milliseconds.
    elapsed_ms: u128,
    /// Peak resident set size in KiB when measurable via `/usr/bin/time -v`.
    peak_rss_kb: Option<u64>,
    /// Tool exit status.
    exit_code: i32,
    /// Tags present after the run but not before.
    created_tags: Vec<String>,
    /// Last 2 KiB of stdout.
    stdout_tail: String,
    /// Last 2 KiB of stderr.
    stderr_tail: String,
}

/// Entry point for `xtask bench-competitors`.
pub fn bench_competitors(args: &[String]) -> Result<(), String> {
    let root = crate::schema::workspace_root()?;
    let write_report = args.iter().any(|a| a == "--report");
    let allow_network = args.iter().any(|a| a == "--allow-network");
    let tool_filter = match args.iter().position(|a| a == "--tool") {
        Some(i) => Some(
            args.get(i + 1)
                .cloned()
                .ok_or_else(|| "--tool needs a value".to_owned())?,
        ),
        None => None,
    };
    let fixture = match args.iter().position(|a| a == "--fixture") {
        Some(i) => Some(
            args.get(i + 1)
                .cloned()
                .ok_or_else(|| "--fixture needs a value".to_owned())?,
        ),
        None => None,
    };

    let tools: Tools = load(&root.join("bench/competitors/tools.toml"))?;
    let claims: Claims = load(&root.join("bench/competitors/claims.toml"))?;
    let results_dir = root.join("bench/competitors/results");
    std::fs::create_dir_all(&results_dir)
        .map_err(|e| format!("mkdir {}: {e}", results_dir.display()))?;

    if write_report {
        return write_report_md(&root, &tools.tool, &claims.claim, &results_dir);
    }

    let fixtures: Vec<String> = match fixture {
        Some(f) => vec![f],
        None => default_fixtures(&root),
    };
    let mut ran = 0usize;
    for t in &tools.tool {
        if let Some(want) = &tool_filter
            && t.name != *want
        {
            continue;
        }
        if t.needs_network && !allow_network {
            println!(
                "skip  {}: recipe requires forge API access (pass --allow-network)",
                t.name
            );
            continue;
        }
        for f in &fixtures {
            println!("run {}: tool={} fixture={f}", t.name, t.binary);
            match run_tool(&root, t, f, &results_dir) {
                Ok(res) => {
                    println!(
                        "  exit={} elapsed={}ms tags_created={}",
                        res.exit_code,
                        res.elapsed_ms,
                        res.created_tags.len()
                    );
                    ran += 1;
                }
                Err(e) => println!("  skipped: {e}"),
            }
        }
    }
    println!(
        "{ran} measured runs; regenerate the report with \
         `cargo run -p xtask -- bench-competitors --report`"
    );
    Ok(())
}

/// Default fixture set: corpus clones when present (sorted for determinism).
fn default_fixtures(root: &std::path::Path) -> Vec<String> {
    let mut found = Vec::new();
    let clones = root.join("corpus/clones");
    if let Ok(entries) = std::fs::read_dir(&clones) {
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|e| e.path().join(".git").exists())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        names.sort();
        found.extend(
            names
                .into_iter()
                .map(|n| clones.join(n).to_string_lossy().into_owned()),
        );
    }
    found
}

fn run_tool(
    root: &std::path::Path,
    tool: &Tool,
    fixture: &str,
    results_dir: &std::path::Path,
) -> Result<RunResult, String> {
    // Disposable working copy of the fixture.
    let work = results_dir.join(format!("work-{}", sanitize(&tool.name)));
    let _ = std::fs::remove_dir_all(&work);
    git(
        root,
        &[
            "clone",
            "--quiet",
            &fixture_uri(fixture),
            &work.to_string_lossy(),
        ],
    )?;
    for (rel, content) in &tool.prepare_files {
        let path = work.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        std::fs::write(&path, content).map_err(|e| format!("write {}: {e}", path.display()))?;
    }

    let before = list_tags(&work)?;
    let needs_repo_url = tool.bench_args.iter().any(|a| a.contains("{repo-url}"));
    let fixture_origin = if needs_repo_url {
        Some(
            git(
                std::path::Path::new(fixture),
                &["remote", "get-url", "origin"],
            )
            .map_err(|_| "recipe needs {repo-url} but the fixture has no origin remote".to_owned())?
            .trim()
            .to_owned(),
        )
    } else {
        None
    };
    let (elapsed_ms, peak_rss_kb, status, stdout, stderr) =
        timed_run(tool, &work, fixture_origin.as_deref())?;
    let after = list_tags(&work)?;
    let created: Vec<String> = after
        .iter()
        .filter(|t| !before.contains(t))
        .cloned()
        .collect();

    let Some(version) = probe_version(tool) else {
        return Err(format!("binary {} not installed", tool.binary));
    };
    let result = RunResult {
        tool: tool.name.clone(),
        version,
        fixture: fixture.to_owned(),
        elapsed_ms,
        peak_rss_kb,
        exit_code: status,
        created_tags: created,
        stdout_tail: tail(&stdout),
        stderr_tail: tail(&stderr),
    };
    let path = results_dir.join(format!(
        "{}.{}.json",
        sanitize(&tool.name),
        sanitize(&fixture_name(fixture))
    ));
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(result)
}

fn timed_run(
    tool: &Tool,
    work: &std::path::Path,
    fixture_origin: Option<&str>,
) -> Result<(u128, Option<u64>, i32, String, String), String> {
    let mut args: Vec<String> = tool
        .bench_args
        .iter()
        .map(|a| a.replace("{fixture}", &work.to_string_lossy()))
        .collect();
    // `{repo-url}` is the fixture's git origin (a real GitHub URL for corpus
    // clones) so networked recipes like release-please's `--repo-url` are
    // supplied from the fixture, never hardcoded here.
    if let Some(origin) = fixture_origin {
        for a in &mut args {
            *a = a.replace("{repo-url}", origin);
        }
    }
    let mut cmd = Command::new(&tool.binary);
    cmd.current_dir(work);
    cmd.args(&args);
    for kv in &tool.env {
        if let Some((k, v)) = kv.split_once('=') {
            cmd.env(k, v);
        }
    }
    let start = Instant::now();
    let out = cmd
        .output()
        .map_err(|e| format!("spawn {}: {e}", tool.binary))?;
    let elapsed = start.elapsed().as_millis();
    let status = out.status.code().unwrap_or(-1);
    Ok((
        elapsed,
        None, // /usr/bin/time wrapper integration lands with the first real measured run
        status,
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

fn probe_version(tool: &Tool) -> Option<String> {
    let out = Command::new(&tool.binary)
        .args(&tool.version_args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Some(text.lines().next().unwrap_or_default().trim().to_owned())
}

fn list_tags(work: &std::path::Path) -> Result<Vec<String>, String> {
    let out = git(work, &["tag", "--list"])?;
    Ok(out
        .lines()
        .map(str::to_owned)
        .filter(|l| !l.is_empty())
        .collect())
}

fn git(cwd: &std::path::Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(args);
    let out = cmd
        .output()
        .map_err(|e| format!("spawn git {args:?}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "git {args:?} failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

fn fixture_uri(fixture: &str) -> String {
    let p = std::path::Path::new(fixture);
    let absolute = if p.is_absolute() {
        p.to_path_buf()
    } else {
        crate::schema::workspace_root()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(p)
    };
    let s = absolute.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        format!("file:///{s}")
    } else {
        format!("file://{s}")
    }
}

fn fixture_name(fixture: &str) -> String {
    match std::path::Path::new(fixture).file_name() {
        Some(n) => n.to_string_lossy().into_owned(),
        None => fixture.to_owned(),
    }
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Last ≤ 2048 chars of `s`, cut on a char boundary (never panics).
fn tail(s: &str) -> String {
    if s.len() <= 2048 {
        return s.to_owned();
    }
    let mut start = s.len() - 2048;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    s.get(start..).unwrap_or_default().to_owned()
}

fn load<T: for<'de> Deserialize<'de>>(path: &std::path::Path) -> Result<T, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))
}

/// Generate `bench/competitors/REPORT.md` from results + claims.
fn write_report_md(
    root: &std::path::Path,
    tools: &[Tool],
    claims: &[Claim],
    results_dir: &std::path::Path,
) -> Result<(), String> {
    use std::fmt::Write as _;

    let mut rows = String::new();
    for t in tools {
        let result_files: Vec<String> = std::fs::read_dir(results_dir)
            .map_err(|e| format!("read {}: {e}", results_dir.display()))?
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let is_json = std::path::Path::new(&name)
                    .extension()
                    .is_some_and(|e| e == "json");
                (name.starts_with(&format!("{}.", sanitize(&t.name))) && is_json).then_some(name)
            })
            .collect();
        let version = probe_version(t).unwrap_or_else(|| "not installed".to_owned());
        let measured = if result_files.is_empty() {
            "pending"
        } else {
            "measured"
        };
        let files_cell = if result_files.is_empty() {
            "no result files yet".to_owned()
        } else {
            result_files.join(", ")
        };
        let _ = writeln!(
            rows,
            "| `{}` | {} | {} | {} |",
            t.name, version, measured, files_cell
        );
    }

    let mut claim_rows = String::new();
    for c in claims {
        let tool = tools.iter().find(|t| t.name == c.tool);
        let result_files: Vec<String> = match tool {
            Some(t) => std::fs::read_dir(results_dir)
                .map(|entries| {
                    entries
                        .flatten()
                        .filter_map(|e| e.file_name().into_string().ok())
                        .filter(|name| {
                            name.starts_with(&format!("{}.", sanitize(&t.name)))
                                && std::path::Path::new(name)
                                    .extension()
                                    .is_some_and(|e| e == "json")
                        })
                        .collect()
                })
                .unwrap_or_default(),
            None => Vec::new(),
        };
        let status = if result_files.is_empty() {
            match tool {
                None => "pending (no recipe in tools.toml)".to_owned(),
                Some(t) if t.needs_network => {
                    "pending (recipe needs forge API access; run with --allow-network)".to_owned()
                }
                Some(t) => {
                    if probe_version(t).is_some() {
                        "pending (installed, no result files yet)".to_owned()
                    } else {
                        "pending (tool not runnable in this environment)".to_owned()
                    }
                }
            }
        } else {
            format!("measured — see {}", result_files.join(", "))
        };
        let _ = writeln!(claim_rows, "| {} | `{}` | {status} |", c.claim, c.tool);
    }

    let report = format!(
        "# Competitor benchmark report (M0.3)\n\n\
         Generated by `cargo run -p xtask -- bench-competitors --report`. Raw JSON lives in \
         `bench/competitors/results/` (gitignored); this file is committed.\n\n\
         ## §1.1 claims\n\n\
         | Claim (from §1.1) | Tool | Status |\n|---|---|---|\n{claim_rows}\n\
         ## Tool runs\n\n\
         | Tool | Version | Status | Result files |\n|---|---|---|---|\n{rows}\n\
         ## Method\n\n\
         1. Each tool runs on a disposable clone of a corpus fixture (see `tools.toml` for the \
         exact argv; nothing is invented here — unrunnable tools stay `pending`).\n\
         2. Wall time is measured around the tool process; peak RSS and API-call counts attach \
         with the first networked measurement run (needs `mitmproxy` recording, M0.3 follow-up).\n\
         3. Marketing claims are only made from these numbers, never from the §1.1 table.\n"
    );
    let path = root.join("bench/competitors/REPORT.md");
    std::fs::write(&path, report).map_err(|e| format!("write {}: {e}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}
