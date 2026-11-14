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
use halyard_git::tag_target::{GixTagTargetResolver, ManifestVersionProbe};

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
/// `publish`, `fetch`, `migrate`, `init`, `action` — land with their
/// milestones M4.x/M5 and are absent rather than stubbed; `apply` joined
/// in M3.6, `pr` in M4.2b, `check` in M4.3 and `release --dry` (the
/// §11.1 git-side tag-phase forecast) in the tag-phase slice).
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
    /// Run the §11.1 tag phase for real: reconcile → pending releases →
    /// annotated-tag directives, targets resolved by the §11.1
    /// manifest-pinpoint walk, tags created through the real writer and
    /// pushed to `origin` (§11.2; `--no-push` skips the push phase).
    /// Forge releases stay deferred (M4.1b — reported, never faked);
    /// `--dry` forecasts everything offline (no writes, no network).
    Release {
        /// Forecast the tag phase offline — nothing written, no network
        /// touched (I5); the push section forecasts what a live run
        /// would send.
        #[arg(long)]
        dry: bool,
        /// Create the tags but skip the §11.2 push phase (local-only
        /// release; the report states the skip).
        #[arg(long)]
        no_push: bool,
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
        Command::Release { dry, no_push } => cmd_release(cli, profiler, *dry, *no_push),
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

/// The §11.1 step-2 reconciliation shared by the `pr` and `release`
/// forecasts: the manifest versions and the enumerated tags are the only
/// inputs — the same pure decision ([`halyard_core::release_state`]) the
/// live run makes, resolved through the planner's own template rule and
/// skip set, so the forecast can never disagree with the plan (I3).
/// Returns the full state rows (Tagged/Pending/Behind — the report) and
/// the deduplicated pending queue (the tag phase's input).
fn reconcile_snapshot(
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
    let (states, pending) = reconcile_snapshot(&context, &plan_config, profiler);
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
    // The tag-phase queue: one `ReleaseSpec` per pending release, with
    // the §9.4 release notes rendered through the notes pipeline
    // (`github-release-notes` over the planner's entry preview). The
    // forge context (the compare link) derives from the configured
    // `origin` URL and the package's previous release tag — a non-GitHub
    // remote, an initial release or a release whose plan entry is absent
    // (the post-merge PROPAGATE state, where the merged plan carried the
    // notes) renders honestly without a link / with empty notes: no
    // fabrication.
    let remote_url = remote_origin_url()?;
    let pending_specs: Vec<halyard_forge::ReleaseSpec> = pending_specs(
        &pending,
        &plan,
        &states,
        &context.discovery.workspace,
        remote_url.as_deref(),
    )?;
    // The notes ride the JSON report; the specs move into the machine
    // input below, so the notes are kept out (owned) by tag first.
    let notes_by_tag: std::collections::BTreeMap<String, String> = pending_specs
        .iter()
        .map(|spec| (spec.tag.clone(), spec.notes.clone()))
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
                    pending_releases: pending_specs.clone(),
                    release_pr,
                },
            )
        })
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    let pending_rows: Vec<serde_json::Value> = pending
        .iter()
        .map(|release| {
            let notes = notes_by_tag
                .get(&release.tag)
                .map_or_else(String::new, Clone::clone);
            serde_json::json!({
                "package": release.package,
                "version": release.version.to_string(),
                "tag": release.tag,
                "notes": notes,
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

/// The tag-phase queue with the §9.4 release notes: one `ReleaseSpec`
/// per pending release. The release that is *already built* renders its
/// notes from the package changelog's own section for that version
/// ([`halyard_changelog::render_release_notes_for_version`] — the merged
/// release PR wrote the section); a plan release whose tag matches the
/// pending tag (the same release proposed by the current plan) renders
/// from its entry preview. Anything else renders empty notes: the shell
/// never fabricates release content.
fn pending_specs(
    pending: &[halyard_core::PendingRelease],
    plan: &halyard_core::Plan,
    states: &[halyard_core::PackageTagState],
    workspace: &halyard_core::package::Workspace,
    remote_url: Option<&str>,
) -> Result<Vec<halyard_forge::ReleaseSpec>, CliError> {
    pending
        .iter()
        .map(|release| {
            // The forge context: origin → previous tag → this tag. A
            // non-GitHub remote or an initial release renders without
            // the compare line (never a guessed foreign-forge shape).
            let latest_tag = states
                .iter()
                .find(|s| s.package == release.package)
                .and_then(|s| s.latest_tag.as_deref());
            let compare = remote_url
                .and_then(|url| halyard_changelog::compare_url(url, latest_tag, &release.tag));
            let context = halyard_changelog::NotesContext { compare };
            // The package changelog file: the config override wins over
            // the keep-a-changelog default (`<package root>/CHANGELOG.md`).
            let package = workspace
                .packages()
                .iter()
                .find(|pkg| workspace.interner().get(pkg.name) == Some(release.package.as_str()));
            let changelog_path = package
                .and_then(|pkg| {
                    pkg.changelog.clone().map_or_else(
                        || {
                            let root = pkg.root.as_str();
                            let file = if root.is_empty() {
                                "CHANGELOG.md".to_owned()
                            } else {
                                format!("{root}/CHANGELOG.md")
                            };
                            halyard_core::RelPath::new(&file).map(|path| path.to_path_buf())
                        },
                        |path| Some(path.to_path_buf()),
                    )
                })
                .ok_or_else(|| {
                    CliError::new(
                        "E-CLI-NO-PACKAGE",
                        format!(
                            "pending release for {:?} names no workspace package (internal \
                             inconsistency)",
                            release.package
                        ),
                    )
                })?;
            let notes =
                match std::fs::read_to_string(&changelog_path) {
                    Ok(text) => halyard_changelog::render_release_notes_for_version(
                        &text,
                        &release.version.to_string(),
                        &context,
                    ),
                    // No changelog file: a plan release with the same tag
                    // (the same release proposed by the current plan)
                    // renders from its entry preview.
                    Err(_) => {
                        match plan.releases.iter().find(|r| r.tag == release.tag).map(
                            |plan_release| {
                                halyard_changelog::render_release_notes(plan_release, &context)
                            },
                        ) {
                            Some(Ok(notes)) => notes,
                            Some(Err(error)) => {
                                return Err(CliError::new(error.code(), error.to_string()));
                            }
                            None => String::new(),
                        }
                    }
                };
            Ok(halyard_forge::ReleaseSpec {
                tag: release.tag.clone(),
                notes,
            })
        })
        .collect()
}

/// The configured `origin` URL (in-process `gix` config read — no
/// subprocess, no network, I5/I6): the compare-link derivation's input.
/// `None` when no remote is configured.
fn remote_origin_url() -> Result<Option<String>, CliError> {
    let repo = halyard_git::gix::open(std::path::Path::new(".")).map_err(|e| {
        CliError::new(
            "E-GIT-OPEN",
            format!("cannot open the repository for the remote lookup: {e}"),
        )
    })?;
    Ok(repo
        .config_snapshot()
        .string("remote.origin.url")
        .map(|value| value.to_string()))
}

/// The shell's manifest-version probe: dispatches the tag-target
/// pinpoint walk's manifest reads to the ecosystem adapters that own the
/// grammars (§7.3 — the git engine never parses manifests). Ecosystems
/// without an adapter (custom) carry no parsable version, so their
/// targets keep the documented HEAD fallback.
struct ShellProbe;

impl ManifestVersionProbe for ShellProbe {
    fn manifest_version(&self, ecosystem: &str, manifest: &[u8]) -> Option<String> {
        match ecosystem {
            "cargo" => halyard_eco_cargo::manifest_version(manifest),
            "npm" => halyard_eco_npm::manifest_version(manifest),
            _ => None,
        }
    }
}

/// The `release` command (§11.1 step 2, git side): the reconciliation
/// feeds the §11.1 precision rule — each pending tag targets the commit
/// that *introduced* its release version
/// ([`halyard_git::tag_target::GixTagTargetResolver`], the bounded
/// first-parent walk on manifest blob-OID change). `--dry` runs the
/// whole phase through the preview writer and pusher — probes are real,
/// mutations are forecasts, nothing is written and no network is
/// touched (I5). The live run creates the annotated tags through
/// [`halyard_git::tag::GixTagWriter`] (the shell owns the tagger
/// identity: git config + clock) and pushes the created tags to
/// `origin` through [`halyard_git::push::GixTagPusher`] (§11.2;
/// `--no-push` skips the phase; a missing remote is an honest skip).
/// Forge releases stay deferred (M4.1b) — reported, never faked.
/// Failures are resumable (I2) and fail the run (exit 1).
fn cmd_release(
    cli: &Cli,
    profiler: &mut Profiler,
    dry: bool,
    no_push: bool,
) -> Result<ExitCode, CliError> {
    let context = context(cli, profiler)?;
    let plan_config: PlanConfig = context.config.plan_config();
    let (states, pending) = reconcile_snapshot(&context, &plan_config, profiler);
    let Some(head) = context.history.head_oid() else {
        return Err(CliError::new(
            "E-GIT-HEAD",
            "the repository has no commits; make an initial commit before running a release",
        ));
    };
    let (report, provenance) = release_tag_phase(&context, &pending, &head, dry, profiler)?;
    let created: Vec<String> = report
        .rows
        .iter()
        .filter_map(|row| match row {
            halyard_git::tag::TagPhaseRow::Applied {
                tag,
                outcome: halyard_git::tag::TagOutcome::Created { .. },
            } => Some(tag.clone()),
            _ => None,
        })
        .collect();
    let push = release_push_phase(dry, no_push, &created, profiler)?;
    // I2: the phase is resumable — failures render as rows and fail the
    // run (§15.4 exit 1) so CI stops on a partial tag phase.
    let failed = report.failed > 0 || push.as_ref().is_some_and(|p| p.failed > 0);
    let head_hex = head.to_string();
    match cli.format {
        Format::Json => {
            let document = release_json_document(
                &pending,
                &report,
                &provenance,
                push.as_ref(),
                dry,
                &head_hex,
            );
            print_pretty_json(&document)?;
        }
        Format::Gha => {
            let rendered = if dry {
                render::release_dry_gha(&head_hex, &states, &report, &provenance, push.as_ref())
            } else {
                render::release_live_gha(&head_hex, &states, &report, &provenance, push.as_ref())
            };
            print!("{rendered}");
        }
        Format::Human => {
            let rendered = if dry {
                render::release_dry_human(&head_hex, &states, &report, &provenance, push.as_ref())
            } else {
                render::release_live_human(&head_hex, &states, &report, &provenance, push.as_ref())
            };
            print!("{rendered}");
        }
    }
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// The §11.1 step-2 tag phase over one executor, two effects: the
/// forecast probes through the preview writer; the live run writes
/// through the real one with the shell's tagger identity (an incomplete
/// identity fails closed per directive — never a synthetic default,
/// `E-GIT-TAG`). Returns the phase report plus each directive's resolved
/// target provenance (the pinpoint answer, keyed by tag name).
fn release_tag_phase(
    context: &Context,
    pending: &[halyard_core::PendingRelease],
    head: &halyard_core::Oid,
    dry: bool,
    profiler: &mut Profiler,
) -> Result<
    (
        halyard_git::tag::TagPhaseReport,
        render::TargetProvenanceMap,
    ),
    CliError,
> {
    // The pinpoint walk reads the workspace's manifests (inherited
    // versions are routed at their authoritative workspace manifest) and
    // the digest's release bases; every unverifiable target keeps the
    // documented HEAD fallback — including the blob-less clone case,
    // where the walk degrades instead of guessing.
    let resolver = GixTagTargetResolver::open(std::path::Path::new("."), Box::new(ShellProbe))
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    let writer: Box<dyn halyard_git::tag::TagWriter> = if dry {
        Box::new(
            halyard_git::tag::PreviewTagWriter::open(std::path::Path::new("."))
                .map_err(|e| CliError::new(e.code(), e.to_string()))?,
        )
    } else {
        Box::new(
            halyard_git::tag::GixTagWriter::open(std::path::Path::new("."), tagger_identity()?)
                .map_err(|e| CliError::new(e.code(), e.to_string()))?,
        )
    };
    Ok(profiler.span("release-phase", || {
        let queue = halyard_git::tag_target::directives_for_pending_resolved(
            pending,
            &context.discovery.workspace,
            context.history.digest(),
            head,
            &resolver,
        );
        let mut targets = std::collections::BTreeMap::new();
        let mut directives = Vec::with_capacity(queue.len());
        for resolved in queue {
            targets.insert(
                resolved.directive.name.clone(),
                render::TargetProvenance {
                    commit: resolved.directive.target.to_string(),
                    source: resolved.source,
                },
            );
            directives.push(resolved.directive);
        }
        let report = halyard_git::tag::run_tag_phase(writer.as_ref(), &directives);
        (report, targets)
    }))
}

/// The §11.2 push phase: the tags this run created (a forecast's
/// creations forecast their own push; the live run's creations push for
/// real). `--no-push` skips; no configured remote is an honest skip
/// rendered by the report.
fn release_push_phase(
    dry: bool,
    no_push: bool,
    created: &[String],
    profiler: &mut Profiler,
) -> Result<Option<halyard_git::push::TagPushReport>, CliError> {
    if no_push {
        return Ok(None);
    }
    let pusher: Box<dyn halyard_git::push::TagPusher> = if dry {
        Box::new(
            halyard_git::push::PreviewTagPusher::open(std::path::Path::new("."))
                .map_err(|e| CliError::new(e.code(), e.to_string()))?,
        )
    } else {
        Box::new(
            halyard_git::push::GixTagPusher::open(std::path::Path::new("."))
                .map_err(|e| CliError::new(e.code(), e.to_string()))?,
        )
    };
    Ok(Some(
        profiler
            .span("release-push", || pusher.push_tags("origin", created))
            .map_err(|e| CliError::new(e.code(), e.to_string()))?,
    ))
}

/// The `release` JSON document (stable field order via `serde_json::json!`;
/// built here so the renderers stay renderer-only, §7.2 style). The live
/// document carries the honestly deferred forge half.
fn release_json_document(
    pending: &[halyard_core::PendingRelease],
    report: &halyard_git::tag::TagPhaseReport,
    provenance: &render::TargetProvenanceMap,
    push: Option<&halyard_git::push::TagPushReport>,
    dry: bool,
    head_hex: &str,
) -> serde_json::Value {
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
    let created_key: &'static str = if dry { "would_create" } else { "created" };
    let directive_rows: Vec<serde_json::Value> = report
        .rows
        .iter()
        .map(|row| render::release_row_json(row, provenance, created_key))
        .collect();
    if dry {
        serde_json::json!({
            "mode": "dry",
            "head": head_hex,
            "pending_releases": pending_rows,
            "directives": directive_rows,
            "created": report.created,
            "already_tagged": report.already_tagged,
            "failed": report.failed,
            "push": render::release_push_json(push, dry),
        })
    } else {
        serde_json::json!({
            "mode": "live",
            "head": head_hex,
            "pending_releases": pending_rows,
            "directives": directive_rows,
            "created": report.created,
            "already_tagged": report.already_tagged,
            "failed": report.failed,
            "push": render::release_push_json(push, dry),
            "forge_releases": "deferred (M4.1b)",
        })
    }
}

/// The shell's tagger identity (§11.1 step 2): `user.name` / `user.email`
/// from the repository's resolved git config (in-process `gix` read —
/// no subprocess) and the wall clock (the shell owns the clock; I3
/// determinism is a planning invariant, not a live-effect one). An
/// incomplete identity stays as-is: the writer validates per directive
/// and fails closed with `E-GIT-TAG`, never synthesizing a default.
fn tagger_identity() -> Result<halyard_git::tag::TaggerIdentity, CliError> {
    let repo = halyard_git::gix::open(std::path::Path::new(".")).map_err(|e| {
        CliError::new(
            "E-GIT-OPEN",
            format!("cannot open the repository for the tagger identity: {e}"),
        )
    })?;
    let config = repo.config_snapshot();
    let name = config
        .string("user.name")
        .map_or_else(String::new, |value| value.to_string());
    let email = config
        .string("user.email")
        .map_or_else(String::new, |value| value.to_string());
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| {
            CliError::new(
                "E-GIT-TAG",
                format!(
                    "the system clock is before the unix epoch; the tag timestamp \
                 cannot be trusted: {e}"
                ),
            )
        })?
        .as_secs();
    Ok(halyard_git::tag::TaggerIdentity::new(
        name,
        email,
        i64::try_from(seconds).unwrap_or(i64::MAX),
    ))
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
