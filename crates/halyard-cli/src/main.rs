//! The `halyard` CLI (M1.10, §16.2): a thin imperative shell over the
//! pure core. Every command runs the same pipeline — discover → git
//! history → plan — and renders through one of the stable formats
//! (`human` | `json` | `gha`). Errors carry the §15.4 stable codes and
//! render through `miette`.
//!
//! Invariants honored here: no network (I5), no subprocess during
//! planning (I6 — the git engine is in-process `gix`), `--format json` on
//! `plan` emits the frozen `halyard-plan/v1` document (I3: byte-stable
//! across runs).

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use miette::Diagnostic;

use crate::profile::Profiler;
use halyard_config::{env_layer, load as config_load, resolve, Layer, ResolvedConfig};
use halyard_core::planner::{plan as core_plan, PlanConfig, Snapshot};
use halyard_core::{HistoryProvider, ReleaseRefs};
use halyard_discover::{discover_repo, DiscoverInput};
use halyard_git::GitHistory;

mod doctor;
mod profile;
mod render;

/// Output format (§16.2 global flags).
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum Format {
    /// Human-readable tables and text (default).
    Human,
    /// Stable JSON (the `halyard-plan/v1` document on `plan`).
    Json,
    /// GitHub Actions job summary + `::warning` annotations.
    Gha,
}

/// Colour policy (honours `NO_COLOR`/`CLICOLOR`, §16.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum Color {
    /// Detect the terminal (default).
    Auto,
    /// Always colour.
    Always,
    /// Never colour.
    Never,
}

/// The halyard release automation CLI.
#[derive(Parser, Debug)]
#[command(
    name = "halyard",
    version,
    about = "Polyglot release automation: plan, explain and ship releases from conventional commits",
    after_help = "Run `halyard plan` in any Cargo/pnpm repository — zero config required (§16.1)."
)]
struct Cli {
    /// Output format: human | json | gha.
    #[arg(long, value_enum, default_value_t = Format::Human, global = true)]
    format: Format,

    /// Colour policy: auto | always | never.
    #[arg(long, value_enum, default_value_t = Color::Auto, global = true)]
    color: Color,

    /// Run as if started in <dir> (§16.2 `-C`).
    #[arg(short = 'C', value_name = "DIR", global = true)]
    cwd: Option<PathBuf>,

    /// Path to `halyard.toml` (default: repo root or `.config/halyard.toml`).
    #[arg(long, value_name = "FILE", global = true)]
    config: Option<PathBuf>,

    /// Never touch the network (planning is offline anyway; skips
    /// token checks in `doctor`).
    #[arg(long, global = true)]
    offline: bool,

    /// Ignore the persistent per-commit cache (M2.5; accepted for
    /// forward compatibility — planning is uncached today).
    #[arg(long, global = true)]
    no_cache: bool,

    /// Emit a Chrome-trace JSON of command phases to $HALYARD_PROFILE_PATH
    /// (or stderr when unset).
    #[arg(long, global = true)]
    profile: bool,

    #[command(subcommand)]
    command: Command,
}

/// The v1 command surface (§16.2; the effectful commands — `apply`, `pr`,
/// `release`, `publish`, `fetch`, `check`, `migrate`, `init`, `action` —
/// land with their milestones M3.6–M4.x and are absent rather than
/// stubbed).
#[derive(Subcommand, Debug)]
enum Command {
    /// Compute and print the release plan (pure, offline).
    Plan {
        /// Print the `halyard-plan/v1` JSON document.
        #[arg(long)]
        json: bool,
        /// Restrict the output to one package.
        #[arg(long, value_name = "P")]
        package: Option<String>,
    },
    /// Why is this package being released? The reason chain (§5.6).
    Explain {
        /// The package name.
        #[arg(value_name = "PKG")]
        package: String,
    },
    /// Table: package, version, last tag, unreleased commits, pending.
    Status,
    /// Print the dependency graph in the given format.
    Graph {
        /// Rendering: text | mermaid | dot | json.
        #[arg(long, value_enum, default_value = "text")]
        format: GraphFormat,
    },
    /// Environment diagnostics (shallow clone? commit-graph? tags?).
    Doctor,
    /// Emit JSON Schemas (plan | config).
    Schema {
        /// Which frozen schema to print.
        #[arg(value_enum, default_value = "plan")]
        what: SchemaKind,
    },
    /// Print shell completion scripts.
    Completions {
        /// Target shell.
        #[arg(value_enum)]
        shell: Shell,
    },
}

/// `graph --format` targets (§16.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum GraphFormat {
    /// Indented dependency listing.
    Text,
    /// Mermaid `graph TD` snippet.
    Mermaid,
    /// Graphviz DOT.
    Dot,
    /// JSON edges.
    Json,
}

/// `schema` targets (§16.2; the frozen documents live under `schemas/`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum SchemaKind {
    /// The `halyard-plan/v1` document.
    Plan,
    /// The `halyard.toml` v1 document.
    Config,
}

/// CLI failures with stable §15.4 codes (rendered by `miette`).
#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("{message}")]
struct CliError {
    /// Stable code (`E-DISC-*`, `E-GIT-*`, `E-PLAN-*`, `E-CLI-*`).
    #[diagnostic(code(halyard::cli))]
    code: String,
    /// Human message including the fix hint.
    message: String,
}

impl CliError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let _color = apply_color_policy(&cli.color);
    let mut profiler = Profiler::start(cli.profile);
    let result = run(&cli, &mut profiler);
    profiler.finish();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if cli.format == Format::Json {
                let payload = serde_json::json!({
                    "error": {
                        "code": error.code,
                        "message": error.message,
                    }
                });
                println!("{payload}");
            } else {
                eprintln!("{:?}", miette::Report::new(error));
            }
            ExitCode::FAILURE
        }
    }
}

/// Resolves the effective colour policy from `--color` plus the
/// `NO_COLOR`/`CLICOLOR` conventions (§16.3). Read-only: `std::env::set_var`
/// is `unsafe` in edition 2024 and this crate forbids `unsafe`, so the
/// decision is passed to renderers instead of mutating the environment.
fn apply_color_policy(color: &Color) -> bool {
    let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    let forced = std::env::var_os("CLICOLOR_FORCE").is_some_and(|v| v == "1");
    match color {
        Color::Never => false,
        Color::Always => true,
        Color::Auto => forced || !no_color,
    }
}

fn run(cli: &Cli, profiler: &mut Profiler) -> Result<(), CliError> {
    if let Some(dir) = &cli.cwd {
        std::env::set_current_dir(dir).map_err(|e| {
            CliError::new(
                "E-CLI-CWD",
                format!("cannot change directory to {}: {e}", dir.display()),
            )
        })?;
    }
    match &cli.command {
        Command::Plan { json, package } => {
            let plan = compute_plan(cli, profiler)?;
            let plan = filtered(plan, package.as_deref());
            if *json || cli.format == Format::Json {
                let value = serde_json::to_value(&plan)
                    .map_err(|e| CliError::new("E-CLI-RENDER", e.to_string()))?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value)
                        .map_err(|e| CliError::new("E-CLI-RENDER", e.to_string()))?
                );
            } else if cli.format == Format::Gha {
                print!("{}", render::plan_gha(&plan));
            } else {
                print!("{}", render::plan_human(&plan));
            }
            Ok(())
        }
        Command::Explain { package } => {
            let plan = compute_plan(cli, profiler)?;
            let release = plan
                .releases
                .iter()
                .find(|r| r.package == *package)
                .ok_or_else(|| {
                    CliError::new(
                        "E-CLI-NO-PACKAGE",
                        format!(
                            "no pending release for {package:?} (it is either not in the \
                             workspace or skipped); run `halyard status` to see every package"
                        ),
                    )
                })?;
            match cli.format {
                Format::Json => {
                    let value = serde_json::json!({
                        "package": release.package,
                        "from": release.from,
                        "to": release.to,
                        "bump": release.bump,
                        "tag": release.tag,
                        "reasons": release.reasons,
                    });
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&value)
                            .map_err(|e| CliError::new("E-CLI-RENDER", e.to_string()))?
                    );
                }
                Format::Gha => print!("{}", render::explain_gha(release)),
                Format::Human => print!("{}", render::explain_human(release)),
            }
            Ok(())
        }
        Command::Status => {
            let context = context(cli, profiler)?;
            match cli.format {
                Format::Json => {
                    let value = serde_json::to_value(&render::status_rows(&context))
                        .map_err(|e| CliError::new("E-CLI-RENDER", e.to_string()))?;
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&value)
                            .map_err(|e| CliError::new("E-CLI-RENDER", e.to_string()))?
                    );
                }
                Format::Gha => print!("{}", render::status_gha(&context)),
                Format::Human => print!("{}", render::status_human(&context)),
            }
            Ok(())
        }
        Command::Graph { format } => {
            let context = context(cli, profiler)?;
            match cli.format {
                Format::Json => {
                    let edges = render::graph_edges(&context);
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&edges)
                            .map_err(|e| CliError::new("E-CLI-RENDER", e.to_string()))?
                    );
                }
                _ => match format {
                    GraphFormat::Text => print!("{}", render::graph_text(&context)),
                    GraphFormat::Mermaid => print!("{}", render::graph_mermaid(&context)),
                    GraphFormat::Dot => print!("{}", render::graph_dot(&context)),
                    GraphFormat::Json => {
                        let edges = render::graph_edges(&context);
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&edges)
                                .map_err(|e| CliError::new("E-CLI-RENDER", e.to_string()))?
                        );
                    }
                },
            }
            Ok(())
        }
        Command::Doctor => {
            let report = doctor::check(cli, profiler)?;
            match cli.format {
                Format::Json => {
                    let value = serde_json::to_value(&report)
                        .map_err(|e| CliError::new("E-CLI-RENDER", e.to_string()))?;
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&value)
                            .map_err(|e| CliError::new("E-CLI-RENDER", e.to_string()))?
                    );
                }
                Format::Gha => print!("{}", render::doctor_gha(&report)),
                Format::Human => print!("{}", render::doctor_human(&report)),
            }
            Ok(())
        }
        Command::Schema { what } => {
            let document = match what {
                SchemaKind::Plan => include_str!("../../../schemas/halyard-plan-v1.json"),
                SchemaKind::Config => include_str!("../../../schemas/halyard-config-v1.json"),
            };
            println!("{document}");
            Ok(())
        }
        Command::Completions { shell } => {
            let mut command = Cli::command();
            generate(*shell, &mut command, "halyard", &mut std::io::stdout());
            Ok(())
        }
    }
}

/// The shared pipeline context: discovery + resolved config + git history.
struct Context {
    config: ResolvedConfig,
    discovery: halyard_discover::Discovery,
    history: GitHistory,
}

/// Discovers the workspace, resolves configuration and paints history.
fn context(cli: &Cli, profiler: &mut Profiler) -> Result<Context, CliError> {
    let discovery = profiler
        .span("discover", || {
            discover_repo(std::path::Path::new("."), &DiscoverInput::default())
        })
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    let config = profiler.span("config", || resolve_config(cli))?;
    let history = profiler
        .span("history", || {
            GitHistory::build(
                std::path::Path::new("."),
                &discovery.workspace,
                config.initial_max_commits,
            )
        })
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    Ok(Context {
        config,
        discovery,
        history,
    })
}

/// Layers: defaults < file (repo `halyard.toml` or `--config`) < env.
/// (CLI-flag and package-metadata layers map through the same `Layer`
/// enum when their milestones land.)
fn resolve_config(cli: &Cli) -> Result<ResolvedConfig, CliError> {
    let mut layers = vec![Layer::Defaults];
    let path = cli.config.clone().or_else(|| {
        let root = PathBuf::from("halyard.toml");
        let dotted = PathBuf::from(".config/halyard.toml");
        if root.is_file() {
            Some(root)
        } else if dotted.is_file() {
            Some(dotted)
        } else {
            None
        }
    });
    if let Some(path) = path {
        let config = config_load(&path).map_err(|e| CliError::new(e.code(), e.to_string()))?;
        layers.push(Layer::File(config));
    }
    layers.push(Layer::Env(env_layer().map_err(
        |e| CliError::new(e.code(), e.to_string()),
    )?));
    resolve(&layers).map_err(|e| CliError::new(e.code(), e.to_string()))
}

/// Runs the full pure pipeline and returns the plan.
fn compute_plan(cli: &Cli, profiler: &mut Profiler) -> Result<halyard_core::Plan, CliError> {
    let context = context(cli, profiler)?;
    profiler.span("plan", || {
        let bases = context.history.digest().bases.clone();
        let refs = ReleaseRefs::from_bases(bases);
        let snapshot = Snapshot {
            workspace: &context.discovery.workspace,
            history: &context.history,
            refs: &refs,
        };
        let plan_config: PlanConfig = context.config.plan_config();
        core_plan(&snapshot, &plan_config).map_err(|e| CliError::new(e.code(), e.to_string()))
    })
}

/// `--package P` output filter: the plan document restricted to one
/// package — its release, its order position, its skips and the edits
/// under its manifest root.
fn filtered(plan: halyard_core::Plan, package: Option<&str>) -> halyard_core::Plan {
    let Some(package) = package else {
        return plan;
    };
    let releases: Vec<halyard_core::PlanRelease> = plan
        .releases
        .into_iter()
        .filter(|r| r.package == package)
        .collect();
    let order: Vec<String> = plan
        .order
        .into_iter()
        .filter(|name| name == package)
        .collect();
    let skipped: Vec<halyard_core::PlanSkip> = plan
        .skipped
        .into_iter()
        .filter(|s| s.package == package)
        .collect();
    let edits: Vec<halyard_core::PlanEdit> = plan
        .edits
        .into_iter()
        .filter(|edit| {
            edit.file == format!("crates/{package}/Cargo.toml")
                || edit.file == format!("crates/{package}/package.json")
                || edit.file == "Cargo.toml"
                || edit.file == "package.json"
        })
        .collect();
    halyard_core::Plan {
        schema: plan.schema,
        base_branch: plan.base_branch,
        head: plan.head,
        plan_hash: plan.plan_hash,
        releases,
        edits,
        order,
        skipped,
    }
}

