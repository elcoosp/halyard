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
use clap_complete::{Shell, generate};
use miette::Diagnostic;

use crate::profile::Profiler;
use halyard_config::{Layer, ResolvedConfig, env_layer, load as config_load, resolve};
use halyard_core::planner::{PlanConfig, Snapshot, plan as core_plan};
use halyard_core::{HistoryProvider, ReleaseRefs};
use halyard_discover::{DiscoverInput, discover_repo};
use halyard_git::GitHistory;

mod check;
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

    /// Colour policy: auto | always | never. Parsed and carried for the
    /// §16.3 colour-aware renderers, which land with their milestone —
    /// output is plain text until then (honouring `NO_COLOR` by
    /// emitting no colour at all).
    #[arg(long, value_enum, default_value_t = Color::Auto, global = true)]
    color: Color,

    /// Run as if started in `<dir>` (§16.2 `-C`).
    #[arg(short = 'C', value_name = "DIR", global = true)]
    cwd: Option<PathBuf>,

    /// Path to `halyard.toml` (default: repo root or `.config/halyard.toml`).
    #[arg(long, value_name = "FILE", global = true)]
    config: Option<PathBuf>,

    /// Never touch the network (planning is offline anyway; skips
    /// token checks in `doctor`).
    #[arg(long, global = true)]
    offline: bool,

    /// Skip the persistent per-commit cache (§8.7): plan uncached and
    /// leave `$GIT_DIR/halyard/cache/v1/` untouched.
    #[arg(long, global = true)]
    no_cache: bool,

    /// Emit a Chrome-trace JSON of command phases to `$HALYARD_PROFILE_PATH`
    /// (or stderr when unset).
    #[arg(long, global = true)]
    profile: bool,

    #[command(subcommand)]
    command: Command,
}

/// The v1 command surface (§16.2; the remaining effectful commands —
/// `release`, `publish`, `fetch`, `migrate`, `init`, `action` — land
/// with their milestones M4.x/M5 and are absent rather than stubbed;
/// `apply` joined in M3.6, `pr` in M4.2b and `check` in M4.3).
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
    /// Apply the computed plan's manifest edits to the working tree
    /// (M3.6 local flow): discover → history → plan → edit set →
    /// adapter patchers → atomic writes.
    Apply {
        /// Print the edit set without writing anything.
        #[arg(long)]
        dry: bool,
    },
    /// Forecast the Release PR without touching a forge (M4.2b step 1,
    /// §11.1 prepare phase): plan → §11.3 body → machine pass over the
    /// dry-run adapter. Live mode lands with the M4.1b GitHub adapter.
    Pr {
        /// Forecast the prepare phase offline (the only implemented
        /// mode; live mode needs the M4.1b forge adapter).
        #[arg(long)]
        dry: bool,
    },
    /// Why is this package being released? The reason chain (§5.6).
    Explain {
        /// The package name.
        #[arg(value_name = "PKG")]
        package: String,
    },
    /// Lint a squash-merge PR title and gate on pending releases
    /// (§11.6): the title is the bump source in squash-merge repos, so
    /// `check --pr-title` predicts — per pending package — the bump the
    /// merge would convey; `--exit-code` turns a pending release into
    /// exit code 3 (§15.4).
    Check {
        /// The squash-merge PR title to lint (§11.6).
        #[arg(long, value_name = "TITLE")]
        pr_title: Option<String>,
        /// Exit 3 when a release is pending (§15.4 exit codes).
        #[arg(long)]
        exit_code: bool,
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
    let mut profiler = Profiler::start(cli.profile);
    let result = run(&cli, &mut profiler);
    profiler.finish();
    match result {
        Ok(code) => code,
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

fn run(cli: &Cli, profiler: &mut Profiler) -> Result<ExitCode, CliError> {
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
            cmd_plan(cli, profiler, *json, package.as_deref())?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Apply { dry } => {
            cmd_apply(cli, profiler, *dry)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Pr { dry } => {
            cmd_pr(cli, profiler, *dry)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Explain { package } => {
            cmd_explain(cli, profiler, package)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Check {
            pr_title,
            exit_code,
        } => cmd_check(cli, profiler, pr_title.as_deref(), *exit_code),
        Command::Status => {
            cmd_status(cli, profiler)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Graph { format } => {
            cmd_graph(cli, profiler, *format)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Doctor => {
            cmd_doctor(cli, profiler)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Schema { what } => {
            let document = match what {
                SchemaKind::Plan => include_str!("../../../schemas/halyard-plan-v1.json"),
                SchemaKind::Config => include_str!("../../../schemas/halyard-config-v1.json"),
            };
            println!("{document}");
            Ok(ExitCode::SUCCESS)
        }
        Command::Completions { shell } => {
            let mut command = Cli::command();
            generate(*shell, &mut command, "halyard", &mut std::io::stdout());
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Serializes `value` into a `serde_json::Value` (`E-CLI-RENDER` on
/// failure).
fn json_value<T: serde::Serialize>(value: &T) -> Result<serde_json::Value, CliError> {
    serde_json::to_value(value).map_err(|e| CliError::new("E-CLI-RENDER", e.to_string()))
}

/// Pretty-prints one JSON value (the shared JSON output path).
fn print_pretty_json(value: &serde_json::Value) -> Result<(), CliError> {
    let rendered = serde_json::to_string_pretty(value)
        .map_err(|e| CliError::new("E-CLI-RENDER", e.to_string()))?;
    println!("{rendered}");
    Ok(())
}

/// The `plan` command.
fn cmd_plan(
    cli: &Cli,
    profiler: &mut Profiler,
    json: bool,
    package: Option<&str>,
) -> Result<(), CliError> {
    let context = context(cli, profiler)?;
    let plan = plan_from_context(&context, profiler)?;
    let plan = filtered(plan, package, &context.discovery.workspace);
    if json || cli.format == Format::Json {
        print_pretty_json(&json_value(&plan)?)?;
    } else if cli.format == Format::Gha {
        print!("{}", render::plan_gha(&plan));
    } else {
        print!("{}", render::plan_human(&plan));
    }
    Ok(())
}

/// The §11.1 step-2 reconciliation for the `pr` forecast: the manifest
/// versions and the enumerated tags are the only inputs — the same pure
/// decision ([`halyard_core::release_state`]) the live run makes,
/// resolved through the planner's own template rule and skip set, so the
/// forecast can never disagree with the plan (I3). Returns the full
/// state rows (Tagged/Pending/Behind — the report) and the deduplicated
/// pending queue (the machine's tag-phase input).
fn reconcile_for_pr(
    context: &Context,
    plan_config: &PlanConfig,
    profiler: &mut Profiler,
) -> (
    Vec<halyard_core::PackageTagState>,
    Vec<halyard_core::PendingRelease>,
) {
    let tag_names: Vec<halyard_core::BString> = context
        .history
        .tags()
        .iter()
        .map(|tag| tag.name.clone())
        .collect();
    let states = profiler.span("reconcile", || {
        halyard_core::release_state::reconcile_tags(
            &context.discovery.workspace,
            &tag_names,
            plan_config,
        )
    });
    let pending = profiler.span("reconcile-pending", || {
        halyard_core::release_state::pending_releases(
            &context.discovery.workspace,
            &tag_names,
            plan_config,
        )
    });
    (states, pending)
}

/// The `pr` command (M4.2b step 1): the §11.1 forecast over the dry-run
/// adapter. `--dry` drives the full machine pass — the §11.1 step-2
/// manifest-vs-tags reconciliation feeds the tag phase, the plan renders
/// the §11.3 body for the prepare phase — and reports the effects, the
/// §11.5 call budget and the body. Nothing is written and no network is
/// touched (I5); live mode lands with the M4.1b forge adapter
/// (`E-CLI-PR`).
fn cmd_pr(cli: &Cli, profiler: &mut Profiler, dry: bool) -> Result<(), CliError> {
    if !dry {
        return Err(CliError::new(
            "E-CLI-PR",
            "live Release-PR mode needs a forge adapter (M4.1b, not implemented yet); \
             use `halyard pr --dry` for the offline forecast",
        ));
    }
    let context = context(cli, profiler)?;
    let plan = plan_from_context(&context, profiler)?;
    let plan_config: PlanConfig = context.config.plan_config();
    let (states, pending) = reconcile_for_pr(&context, &plan_config, profiler);
    // §11.2: the release branch is `halyard/release/<base>`. The plan's
    // `base_branch` stays `HEAD` until the live shell resolves the
    // remote HEAD (planner.rs) — the forecast names exactly what a live
    // run on this config would target.
    let branch = halyard_forge::BranchName::new(&format!("halyard/release/{}", plan.base_branch))
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    // The §11.3 body renders only when the plan has releases; an empty
    // plan sends the machine on the close probe (§11.1 step 3).
    let body = if plan.releases.is_empty() {
        None
    } else {
        Some(
            halyard_changelog::render_release_body(&plan)
                .map_err(|e| CliError::new(e.code(), e.to_string()))?,
        )
    };
    // The tag-phase queue: one `ReleaseSpec` per pending release. The
    // notes stay empty — the live tag phase renders them from the
    // package changelog (§9.4) once the notes pipeline lands with the
    // live slice; the forecast does not fabricate entries that do not
    // exist yet.
    let pending_specs: Vec<halyard_forge::ReleaseSpec> = pending
        .iter()
        .map(|release| halyard_forge::ReleaseSpec {
            tag: release.tag.clone(),
            notes: String::new(),
        })
        .collect();
    // The prepare-phase input: the Release PR the plan would upsert —
    // the machine gates it behind the tag phase (PROPAGATE, §11.1
    // step 2) when anything is pending.
    let title = release_pr_title(&plan);
    let release_pr = body.clone().map(|body| halyard_forge::PrSpec {
        branch: branch.clone(),
        title: title.clone(),
        body,
    });
    let report = profiler
        .span("pr-dry", || {
            halyard_forge::run_release_machine(
                &halyard_forge::PreviewForge::new(),
                &halyard_forge::MachineInput {
                    branch: branch.clone(),
                    pending_releases: pending_specs,
                    release_pr,
                },
            )
        })
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    let pending_rows: Vec<serde_json::Value> = pending
        .iter()
        .map(|release| {
            serde_json::json!({
                "package": release.package,
                "version": release.version.to_string(),
                "tag": release.tag,
            })
        })
        .collect();
    match cli.format {
        Format::Json => {
            let effects: Vec<serde_json::Value> =
                report.effects.iter().map(render::pr_effect_json).collect();
            print_pretty_json(&serde_json::json!({
                "mode": "dry",
                "title": title,
                "branch": branch.as_str(),
                "pending_plan": !plan.releases.is_empty(),
                "pending_releases": pending_rows,
                "propagated": report.propagated,
                "effects": effects,
                "calls": report.calls,
                "body": body,
            }))?;
        }
        Format::Gha => print!(
            "{}",
            render::pr_dry_gha(&title, branch.as_str(), &states, &report, body.as_deref())
        ),
        Format::Human => print!(
            "{}",
            render::pr_dry_human(&title, branch.as_str(), &states, &report, body.as_deref())
        ),
    }
    Ok(())
}

/// The Release-PR title (§11.3): `chore: release <tags>` in the plan's
/// package order — deterministic (I3) and conventional, so the merged
/// squash subject later passes the `check` lint (§11.6).
fn release_pr_title(plan: &halyard_core::Plan) -> String {
    let tags: Vec<&str> = plan.releases.iter().map(|r| r.tag.as_str()).collect();
    format!("chore: release {}", tags.join(", "))
}

/// The `explain` command.
fn cmd_explain(cli: &Cli, profiler: &mut Profiler, package: &str) -> Result<(), CliError> {
    let plan = compute_plan(cli, profiler)?;
    let release = plan
        .releases
        .iter()
        .find(|r| r.package == package)
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
            print_pretty_json(&value)?;
        }
        Format::Gha => print!("{}", render::explain_gha(release)),
        Format::Human => print!("{}", render::explain_human(release)),
    }
    Ok(())
}

/// The `status` command.
fn cmd_status(cli: &Cli, profiler: &mut Profiler) -> Result<(), CliError> {
    let context = context(cli, profiler)?;
    match cli.format {
        Format::Json => print_pretty_json(&json_value(&render::status_rows(&context))?)?,
        Format::Gha => print!("{}", render::status_gha(&context)),
        Format::Human => print!("{}", render::status_human(&context)),
    }
    Ok(())
}

/// The `graph` command.
fn cmd_graph(cli: &Cli, profiler: &mut Profiler, format: GraphFormat) -> Result<(), CliError> {
    let context = context(cli, profiler)?;
    match cli.format {
        Format::Json => print_pretty_json(&json_value(&render::graph_edges(&context))?)?,
        _ => match format {
            GraphFormat::Text => print!("{}", render::graph_text(&context)),
            GraphFormat::Mermaid => print!("{}", render::graph_mermaid(&context)),
            GraphFormat::Dot => print!("{}", render::graph_dot(&context)),
            GraphFormat::Json => print_pretty_json(&json_value(&render::graph_edges(&context))?)?,
        },
    }
    Ok(())
}

/// The `doctor` command.
fn cmd_doctor(cli: &Cli, profiler: &mut Profiler) -> Result<(), CliError> {
    let report = doctor::check(cli, profiler)?;
    match cli.format {
        Format::Json => print_pretty_json(&json_value(&report)?)?,
        Format::Gha => print!("{}", render::doctor_gha(&report)),
        Format::Human => print!("{}", render::doctor_human(&report)),
    }
    Ok(())
}

/// The `check` command (§11.6): lint a squash-merge PR title against the
/// resolved config (the same mapping the planner applies) and predict —
/// per pending package — the bump the merge would convey; `--exit-code`
/// gates CI on a pending release (§15.4 exit code 3). A rejected title
/// fails with the `E-CLI-TITLE` diagnosis (§11.6: errors teach).
fn cmd_check(
    cli: &Cli,
    profiler: &mut Profiler,
    pr_title: Option<&str>,
    exit_code: bool,
) -> Result<ExitCode, CliError> {
    let context = context(cli, profiler)?;
    let plan = plan_from_context(&context, profiler)?;
    let pending = !plan.releases.is_empty();
    let plan_config: PlanConfig = context.config.plan_config();
    if let Some(title) = pr_title {
        match check::lint(title, &plan_config) {
            halyard_conventional::TitleVerdict::Accepted(lint) => {
                // §11.6's precise diagnosis: an unmapped kind with a close
                // known kind is a likely typo — fail loudly instead of
                // silently releasing nothing. A genuinely unmapped kind
                // (`chore:`, `docs:`) is a legitimate no-bump verdict.
                if let Some(closest) = &lint.closest_known {
                    return Err(CliError::new(
                        "E-CLI-TITLE",
                        format!(
                            "PR title type {:?} is not a known release type — did you mean \
                             {closest:?}? Known types: {}. Fix: reword the merge title; \
                             unmapped types release nothing",
                            lint.kind,
                            known_types_csv(&plan_config),
                        ),
                    ));
                }
                let predictions = check::predict(&plan, &lint, &plan_config);
                match cli.format {
                    Format::Json => {
                        let predicted: Vec<serde_json::Value> = predictions
                            .iter()
                            .map(|row| {
                                serde_json::json!({
                                    "package": row.package,
                                    "from": row.from,
                                    "bump": row.bump,
                                    "remapped": row.remapped,
                                })
                            })
                            .collect();
                        print_pretty_json(&serde_json::json!({
                            "title": title,
                            "verdict": "accepted",
                            "kind": lint.kind,
                            "scope": lint.scope,
                            "breaking": lint.breaking,
                            "level": lint.level.as_str(),
                            "predicted": predicted,
                            "pending": pending,
                        }))?;
                    }
                    Format::Gha => print!("{}", render::check_gha(title, &lint, &predictions)),
                    Format::Human => {
                        print!("{}", render::check_human(title, &lint, &predictions));
                    }
                }
            }
            halyard_conventional::TitleVerdict::NotConventional { reason } => {
                return Err(CliError::new(
                    "E-CLI-TITLE",
                    format!(
                        "PR title {title:?} is not a conventional commit: {reason}. \
                         Fix: squash-merge repos derive the bump from the title \
                         (§11.6); use `type(scope): description` (e.g. \
                         `feat(parser): accept spans`)"
                    ),
                ));
            }
            halyard_conventional::TitleVerdict::FixupOrSquash => {
                return Err(CliError::new(
                    "E-CLI-TITLE",
                    format!(
                        "PR title {title:?} is a fixup!/squash! subject — autosquash \
                         commits are never released (§8.5). Fix: reword the merge \
                         title to a conventional commit"
                    ),
                ));
            }
        }
    } else {
        match cli.format {
            Format::Json => {
                let releases: Vec<serde_json::Value> = plan
                    .releases
                    .iter()
                    .map(|release| {
                        serde_json::json!({
                            "package": release.package,
                            "from": release.from,
                            "to": release.to,
                            "bump": release.bump,
                        })
                    })
                    .collect();
                print_pretty_json(
                    &serde_json::json!({ "pending": pending, "releases": releases }),
                )?;
            }
            Format::Gha => print!("{}", render::check_pending_gha(&plan)),
            Format::Human => print!("{}", render::check_pending_human(&plan)),
        }
    }
    // §15.4 exit codes: 3 = a release is pending (the `check --exit-code`
    // CI gate). The title-lint failure path above exits 1 via CliError.
    Ok(if exit_code && pending {
        ExitCode::from(3)
    } else {
        ExitCode::SUCCESS
    })
}

/// The config's known release types, sorted, comma-separated (the
/// `E-CLI-TITLE` diagnosis lists what the mapping accepts).
fn known_types_csv(config: &PlanConfig) -> String {
    config
        .commit_types
        .keys()
        .cloned()
        .collect::<Vec<_>>()
        .join(", ")
}

/// The shared pipeline context: discovery + resolved config + git history.
struct Context {
    config: ResolvedConfig,
    discovery: halyard_discover::Discovery,
    history: GitHistory,
}

/// Discovers the workspace, resolves configuration and paints history.
/// The git engine runs over the §8.7 persistent cache unless `--no-cache`
/// is given ([`plan_cache`]); the cache-key components (trie layout hash,
/// config hash) ride along so a layout or policy change invalidates the
/// cached attributions cleanly (§8.7).
fn context(cli: &Cli, profiler: &mut Profiler) -> Result<Context, CliError> {
    let discovery = profiler
        .span("discover", || {
            discover_repo(std::path::Path::new("."), &DiscoverInput::default())
        })
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    let config = profiler.span("config", || resolve_config(cli))?;
    let history = profiler
        .span("history", || {
            // §8.7 cache-key component: the trie layout hash over the
            // package roots and their include/exclude filters (sorted by
            // root, matching the hash's normalization).
            let filters: Vec<(String, Vec<String>, Vec<String>)> = discovery
                .workspace
                .packages()
                .iter()
                .map(|pkg| {
                    (
                        pkg.root.as_str().to_owned(),
                        pkg.include
                            .include()
                            .iter()
                            .map(ToString::to_string)
                            .collect(),
                        pkg.include
                            .exclude()
                            .iter()
                            .map(ToString::to_string)
                            .collect(),
                    )
                })
                .collect();
            let mut filters = filters;
            filters.sort_unstable_by(|a, b| a.0.cmp(&b.0));
            let cache = plan_cache(cli);
            GitHistory::build_with_options(
                std::path::Path::new("."),
                &discovery.workspace,
                halyard_git::GitHistoryOptions {
                    initial_max_commits: config.initial_max_commits,
                    cache,
                    layout_hash: halyard_core::trie::layout_hash(&discovery.workspace, &filters),
                    config_hash: halyard_config::config_hash(&config),
                    ..halyard_git::GitHistoryOptions::default()
                },
            )
        })
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    Ok(Context {
        config,
        discovery,
        history,
    })
}

/// Opens the §8.7 persistent plan cache under `$GIT_DIR/halyard/cache/v1/`
/// (`None` under `--no-cache`). A cache that cannot open (unwritable
/// `$GIT_DIR`, corrupt header) degrades to an uncached run with a warning
/// — §8.7: the cache is an optimization, never a correctness surface, so
/// planning must proceed.
fn plan_cache(cli: &Cli) -> Option<halyard_cache::Cache> {
    if cli.no_cache {
        return None;
    }
    match halyard_cache::Cache::open(std::path::Path::new(".git")) {
        Ok(cache) => Some(cache),
        Err(error) => {
            eprintln!(
                "warning: [{}] planning proceeds uncached (the cache is safe to delete \
                 and will be rebuilt)",
                error.code()
            );
            None
        }
    }
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
    layers.push(Layer::Env(
        env_layer().map_err(|e| CliError::new(e.code(), e.to_string()))?,
    ));
    resolve(&layers).map_err(|e| CliError::new(e.code(), e.to_string()))
}

/// Runs the full pure pipeline and returns the plan.
fn compute_plan(cli: &Cli, profiler: &mut Profiler) -> Result<halyard_core::Plan, CliError> {
    let context = context(cli, profiler)?;
    plan_from_context(&context, profiler)
}

/// The pure planning step over an established [`Context`] (shared by
/// `plan`, `explain` and `apply` — the same pipeline, §16.2).
fn plan_from_context(
    context: &Context,
    profiler: &mut Profiler,
) -> Result<halyard_core::Plan, CliError> {
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

/// The `apply` command (M3.6): plan → edit set → adapter patchers →
/// atomic writes. `--dry` prints the deterministic edit-set preview and
/// touches nothing.
fn cmd_apply(cli: &Cli, profiler: &mut Profiler, dry: bool) -> Result<(), CliError> {
    let context = context(cli, profiler)?;
    let plan = plan_from_context(&context, profiler)?;
    if dry {
        let preview = halyard_edit::dry_run_preview(&plan)
            .map_err(|e| CliError::new(e.code(), e.to_string()))?;
        match cli.format {
            Format::Json => {
                let files: Vec<serde_json::Value> = preview
                    .iter()
                    .map(|(file, patches)| {
                        serde_json::json!({
                            "file": file,
                            "patches": patches.iter().map(|p| serde_json::json!({
                                "start": p.start, "end": p.end, "text": p.text,
                            })).collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                let total: usize = preview.values().map(Vec::len).sum();
                print_pretty_json(&serde_json::json!({ "files": files, "patches": total }))?;
            }
            Format::Gha | Format::Human => print!("{}", render::apply_dry(&preview)),
        }
        return Ok(());
    }
    let adapters = halyard_discover::registered_adapters();
    let report = profiler
        .span("apply", || {
            halyard_edit::apply_plan(
                std::path::Path::new("."),
                &plan,
                &context.discovery.workspace,
                &adapters,
            )
        })
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    // M3.2/M3.4 native fallback (§9.2 rule 4 / §9.3): an in-memory
    // patcher declined → the CLI shell runs the sanctioned effect step
    // per declined lockfile and verifies that only workspace members
    // moved. Failing that verification is a hard error (re-plan instead
    // of trusting a shifted tree).
    let declined: Vec<&halyard_edit::LockfileSummary> = report
        .lockfiles
        .iter()
        .filter(|lock| lock.native_reason.is_some())
        .collect();
    let native_refreshes = if declined.is_empty() {
        Vec::new()
    } else {
        native_lockfile_refreshes(&context.discovery.workspace, &adapters, &declined)?
    };
    match cli.format {
        Format::Json => {
            print_pretty_json(&apply_report_json(&report, &native_refreshes))?;
        }
        Format::Gha | Format::Human => {
            print!("{}", render::apply_report(&report, &native_refreshes));
        }
    }
    Ok(())
}

/// The §9.2 rule-4 / §9.3 native fallbacks (M3.2+M3.4): an in-memory
/// patcher declined, so the CLI shell runs each declined lockfile's
/// adapter effect step (`cargo update --workspace --offline`,
/// `pnpm install --lockfile-only --offline`) and verifies afterwards that
/// **only workspace members moved** — a resolution change fails closed
/// (`E-LOCK-NATIVE`, re-plan instead of trusting a shifted tree).
/// Returns one `(lockfile, moved members)` row per declined lockfile.
fn native_lockfile_refreshes(
    workspace: &halyard_core::package::Workspace,
    adapters: &[&dyn halyard_core::EcosystemAdapter],
    declined: &[&halyard_edit::LockfileSummary],
) -> Result<Vec<(String, Vec<String>)>, CliError> {
    let mut rows = Vec::new();
    for lock in declined {
        let adapter = adapters
            .iter()
            .copied()
            .find(|adapter| adapter.id() == lock.ecosystem)
            .ok_or_else(|| {
                CliError::new(
                    halyard_core::lockfile::NativeRefreshError::CODE,
                    format!(
                        "plan releases {} packages but no {} adapter is registered",
                        lock.ecosystem, lock.ecosystem
                    ),
                )
            })?;
        let read_lock = || {
            std::fs::read(lock.file.to_path_buf()).map_err(|source| {
                CliError::new(
                    halyard_edit::EditError::IO_CODE,
                    format!("cannot read {} for the native refresh: {source}", lock.file),
                )
            })
        };
        let before = read_lock()?;
        adapter
            .refresh_lockfile_native(std::path::Path::new("."))
            .map_err(|e| CliError::new(e.code(), e.to_string()))?;
        let after = read_lock()?;
        // An unresolvable package symbol is an internal inconsistency —
        // fail closed with the stable code instead of silently verifying
        // against an empty member list (which would pass any refresh).
        let members: Vec<String> = workspace
            .packages()
            .iter()
            .filter(|package| package.ecosystem.resolve(workspace.interner()) == lock.ecosystem)
            .map(|package| {
                workspace
                    .interner()
                    .get(package.name)
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        CliError::new(
                            halyard_core::lockfile::NativeRefreshError::CODE,
                            format!(
                                "package symbol {} is not interned (internal error; the \
                             native-refresh verification cannot proceed safely)",
                                package.name.to_u32(),
                            ),
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let moved = match lock.ecosystem.as_str() {
            "cargo" => {
                halyard_eco_cargo::lockfile::verify_native_refresh(&before, &after, &members)
                    .map_err(|e| CliError::new(e.code(), e.to_string()))?
            }
            "npm" => halyard_eco_npm::lockfile::verify_native_refresh(&before, &after, &members)
                .map_err(|e| CliError::new(e.code(), e.to_string()))?,
            other => {
                return Err(CliError::new(
                    halyard_core::lockfile::NativeRefreshError::CODE,
                    format!("no native-refresh verifier for ecosystem {other:?}"),
                ));
            }
        };
        rows.push((lock.file.to_string(), moved));
    }
    Ok(rows)
}

/// The `apply` JSON summary (deterministic field order via `serde_json::json!`;
/// built here so `halyard-edit` stays dependency-free, §7.2).
fn apply_report_json(
    report: &halyard_edit::ApplyReport,
    native_refreshes: &[(String, Vec<String>)],
) -> serde_json::Value {
    let files: Vec<serde_json::Value> = report
        .files
        .iter()
        .map(|file| {
            serde_json::json!({
                "file": file.file.to_string(),
                "applied": file.applied,
                "skipped": file.skipped,
            })
        })
        .collect();
    let lockfiles: Vec<serde_json::Value> = report
        .lockfiles
        .iter()
        .map(|lock| {
            serde_json::json!({
                "file": lock.file.to_string(),
                "ecosystem": lock.ecosystem,
                "applied": lock.applied,
                "skipped": lock.skipped,
                "native_refresh_required": lock.native_reason.is_some(),
                "native_reason": lock.native_reason,
            })
        })
        .collect();
    let native: Vec<serde_json::Value> = native_refreshes
        .iter()
        .map(|(file, moved)| {
            serde_json::json!({
                "file": file,
                "members_moved": moved,
            })
        })
        .collect();
    serde_json::json!({
        "files": files,
        "lockfiles": lockfiles,
        "native_refresh": native,
        "applied": report.total_applied(),
        "skipped": report.total_skipped(),
    })
}

/// `--package P` output filter: the plan document restricted to one
/// package — its release, its order position, its skips and the edits
/// under its manifest root (the package's manifest plus everything
/// inside its package root directory).
fn filtered(
    plan: halyard_core::Plan,
    package: Option<&str>,
    workspace: &halyard_core::package::Workspace,
) -> halyard_core::Plan {
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
    // The package's edit files: its own manifest plus every file inside
    // its package root (workspace-root manifests of a root package are
    // its manifest itself). Resolved from the workspace, not from a
    // hardcoded layout.
    let owned = |file: &str| {
        workspace.packages().iter().any(|pkg| {
            workspace.interner().get(pkg.name) == Some(package)
                && (pkg.manifest.as_str() == file
                    || (!pkg.root.as_str().is_empty()
                        && file
                            .strip_prefix(pkg.root.as_str())
                            .is_some_and(|rest| rest.starts_with('/'))))
        })
    };
    let edits: Vec<halyard_core::PlanEdit> = plan
        .edits
        .into_iter()
        .filter(|edit| owned(&edit.file))
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
