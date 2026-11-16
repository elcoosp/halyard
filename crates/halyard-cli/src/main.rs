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

use std::fmt::Write as _;
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
use halyard_forge::Forge as _;
use halyard_forge_github::{ForgeEndpoint, GitHubRestForge, UreqTransport};
use halyard_git::GitHistory;
use halyard_git::fetch::TagFetcher as _;
use halyard_git::tag_target::{GixTagTargetResolver, ManifestVersionProbe};

mod check;
mod doctor;
mod profile;
mod publish_effect;
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

/// The v1 command surface (§16.2; the remaining commands —
/// `init`, `action` — land with their
/// milestones M4.x/M5 and are absent rather than stubbed; `apply` joined
/// in M3.6, `pr` in M4.2b, `check` in M4.3, `release --dry` (the
/// §11.1 git-side tag-phase forecast) in the tag-phase slice, `fetch`
/// (§8.8) in the smart-minimal-fetch slice, `publish` (§13, the
/// M5.2 engine + the verified-argv spawn) in the publish slice and
/// `migrate` (report, and with `--write` the §16.2 draft write) in the
/// M4.7 slices).
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
    /// The §11.1 prepare phase (M4.2b + M4.1b): plan → §11.3 body →
    /// machine pass over the adapter — `--dry` forecasts offline over
    /// the preview adapter (no network, I5); live mode runs the same
    /// machine over the GitHub REST adapter (ADR-0013): the Release PR
    /// is opened/rewritten/closed for real, gated on
    /// `GITHUB_TOKEN`/`HALYARD_FORGE_TOKEN` and a GitHub-shaped origin.
    Pr {
        /// Forecast the prepare phase offline (no network touched).
        #[arg(long)]
        dry: bool,
    },
    /// Run the §11.1 tag phase for real: reconcile → pending releases →
    /// annotated-tag directives, targets resolved by the §11.1
    /// manifest-pinpoint walk, tags created through the real writer and
    /// pushed to `origin` (§11.2; `--no-push` skips the push phase).
    /// The forge half (GET-by-tag-first release creation) runs over the
    /// M4.1b GitHub REST adapter when credentials allow it and is
    /// reported as an honest skip otherwise; `--dry` forecasts
    /// everything offline (no writes, no network).
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
    /// Publish the pending releases (M5.2, §13): the publish engine
    /// validates the plan (I8), layers the targets dependencies-first
    /// over the workspace graph, resumes through the registry
    /// existence probes (already-published versions are no-ops, I1),
    /// waits for index visibility before dependents publish and shells
    /// out to `cargo publish` / `pnpm publish` per target (§13.4).
    /// `--dry` forecasts offline: the tools run their documented dry
    /// form, the registry is never probed (I5). Failures are rows —
    /// the re-run continues from the first failure (§13.7); a partial
    /// publish exits 4 (§15.4).
    Publish {
        /// Forecast offline (the tool's dry form, no registry probes).
        #[arg(long)]
        dry: bool,
    },
    /// Import a foreign release tool's configuration into a migration
    /// report (M4.7, §16.2): the release-please and changesets
    /// configs are surveyed against the `halyard.toml` v1 surface —
    /// every mappable foreign key becomes a row with the pinned value
    /// (the foreign defaults preserved where they differ from
    /// halyard's), every semantic difference becomes an unmapped row
    /// with its reason, and a validated draft `halyard.toml` rides the
    /// report. Without `--write` the command is report-only: nothing
    /// is written. `--write` (M4.7 step 2) lands the validated draft
    /// at `halyard.toml` atomically, create-only — an existing config
    /// is never clobbered and two configured tools are a refusal, not
    /// a guess (`E-CFG-MIGRATE`); nothing configured is an honest
    /// no-op. Needs no git repo and no existing `halyard.toml` — the
    /// migration targets a repo that is not yet halyard-configured.
    Migrate {
        /// Write the validated draft to `halyard.toml` (§16.2 step 2):
        /// atomic (temp sibling → fsync → rename), refused when the
        /// target already exists, refused when more than one foreign
        /// tool is configured.
        #[arg(long)]
        write: bool,
    },
    /// Why is this package being released? The reason chain (§5.6).
    Explain {
        /// The package name.
        #[arg(value_name = "PKG")]
        package: String,
    },
    /// Smart minimal fetch (§8.8): probe the remote's tags, select each
    /// package's latest release tag and fetch those tags plus the base
    /// branch in one minimal `git fetch` (blob-less, tag-bounded,
    /// shallow-deepening bounded by the previous releases), with the
    /// documented full-fetch fallback (ADR-0016). Idempotent (I1):
    /// tags already local at the probed target are not requested again.
    Fetch,
    /// Lint a squash-merge PR title and gate on pending releases
    /// (§11.6): the title is the bump source in squash-merge repos, so
    /// `check --pr-title` predicts — per pending package — the bump the
    /// merge would convey; `--exit-code` turns a pending release into
    /// exit code 3 (§15.4). `--comment` posts the verdict to the PR
    /// under review (found by its head branch) through the forge seam's
    /// marker-based comment upsert — the §11.6 "if merged, this will
    /// release" comment, idempotent on re-runs; live-only (credentials
    /// required, the decline names the fix).
    Check {
        /// The squash-merge PR title to lint (§11.6).
        #[arg(long, value_name = "TITLE")]
        pr_title: Option<String>,
        /// Post the verdict as the PR's halyard check comment (§11.6,
        /// M4.5 step 1): requires `--pr-title` (the verdict is the
        /// comment) and live forge credentials.
        #[arg(long)]
        comment: bool,
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
    /// `--registries` adds the opt-in §13 registry reachability probe
    /// (network touched only by the flag, I5).
    Doctor {
        /// Probe the default registries (crates.io + npm) for
        /// reachability and protocol comprehension (M5.1).
        #[arg(long)]
        registries: bool,
    },
    /// Read-only forge reachability probe (the nightly `forge-smoke`
    /// job's subcommand, ADR-0013): builds the GitHub REST adapter from
    /// `HALYARD_FORGE_TOKEN`/`GITHUB_TOKEN`, probes release-tag
    /// existence (a GET, never a mutation) and prints the rate snapshot
    /// and the §11.5 call log. Nothing is written anywhere.
    ForgeSmoke {
        /// The `owner/repo` slug to probe (default: `GITHUB_REPOSITORY`,
        /// else the GitHub slug of the configured `origin` remote).
        #[arg(long, value_name = "OWNER/REPO")]
        repo: Option<String>,
        /// The API base to probe (default: `GITHUB_API_URL`, else
        /// `https://api.github.com`; the loopback mock lane overrides
        /// this).
        #[arg(long, value_name = "URL")]
        api_base: Option<String>,
    },
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
        Command::ForgeSmoke { repo, api_base } => {
            cmd_forge_smoke(cli, repo.as_deref(), api_base.as_deref())
        }
        Command::Release { dry, no_push } => cmd_release(cli, profiler, *dry, *no_push),
        Command::Publish { dry } => cmd_publish(cli, profiler, *dry),
        Command::Migrate { write } => {
            cmd_migrate(cli, profiler, *write)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Fetch => cmd_fetch(cli, profiler),
        Command::Explain { package } => {
            cmd_explain(cli, profiler, package)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Check {
            pr_title,
            comment,
            exit_code,
        } => cmd_check(cli, profiler, pr_title.as_deref(), *comment, *exit_code),
        Command::Status => {
            cmd_status(cli, profiler)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Graph { format } => {
            cmd_graph(cli, profiler, *format)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Doctor { registries } => {
            cmd_doctor(cli, profiler, *registries)?;
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
        // The plan JSON is the frozen halyard-plan/v1 schema (§5.8) — the
        // walk provenance surfaces ride the report documents and the
        // human/gha renders, never the plan schema.
        print_pretty_json(&json_value(&plan)?)?;
    } else if cli.format == Format::Gha {
        print!("{}", render::plan_gha(&plan, &context.stats));
    } else {
        print!("{}", render::plan_human(&plan, &context.stats));
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

/// The `pr` command (M4.2b + M4.1b): the §11.1 prepare phase over the
/// adapter the mode selects. `--dry` drives the full machine pass over
/// the preview adapter — the §11.1 step-2 manifest-vs-tags
/// reconciliation feeds the tag phase, the plan renders the §11.3 body
/// — and reports the effects, the §11.5 call budget and the body;
/// nothing is written and no network is touched (I5). Live mode runs
/// the same machine over the GitHub REST adapter
/// ([`GitHubRestForge`]): the Release PR is opened, rewritten or —
/// under an empty plan — closed for real, fail-closed on missing
/// credentials (`E-FORGE-AUTH`) and on a non-GitHub origin
/// (`E-CLI-PR`).
fn cmd_pr(cli: &Cli, profiler: &mut Profiler, dry: bool) -> Result<(), CliError> {
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
    // The adapter the mode selects: the preview adapter forecasts an
    // empty forge offline; the live mode builds the GitHub REST adapter
    // from the environment and the origin remote (fail-closed on
    // missing credentials and foreign forges).
    let (forge, slug): (Box<dyn halyard_forge::Forge>, Option<String>) = if dry {
        (Box::new(halyard_forge::PreviewForge::new()), None)
    } else {
        let env = forge_credentials()?;
        let slug = env.endpoint.slug();
        let forge = Box::new(live_forge(env)) as Box<dyn halyard_forge::Forge>;
        (forge, Some(slug))
    };
    // The live run rides the §11.5 call governor (M5.5, ADR-0015):
    // pacing, Retry-After, deterministic backoff and the per-run budget
    // from the `HALYARD_FORGE_*` env over the shell default. The dry run
    // stays ungoverned — a forecast spends no calls (I5).
    let policy = if dry {
        None
    } else {
        Some(governor_policy_from_env()?)
    };
    let governor = policy.map(|policy| {
        let (clock, sleeper) = halyard_forge::governor::system_time();
        halyard_forge::Governor::new(forge.as_ref(), policy, clock, sleeper)
    });
    let mode_label = if dry { "pr-dry" } else { "pr-live" };
    let report = profiler
        .span(mode_label, || {
            let forged: &dyn halyard_forge::Forge = governor
                .as_ref()
                .map_or_else(|| forge.as_ref(), |governor| governor);
            halyard_forge::run_release_machine(
                forged,
                &halyard_forge::MachineInput {
                    branch: branch.clone(),
                    pending_releases: pending_specs.clone(),
                    release_pr,
                },
            )
        })
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    // The governor's run summary (§11.5 end-of-run usage summary): the
    // live documents carry it, the dry run has no governor to report.
    let governor_summary = governor.as_ref().map(render::GovernorSummary::of);
    let pending_rows = pending_rows_json(&pending, &notes_by_tag);
    let history_json = context.stats.json();
    let mut extras = serde_json::json!({
        "dry": dry,
        "repo": slug,
        "pending_plan": !plan.releases.is_empty(),
        "pending_releases": pending_rows,
        "history": history_json,
        "walk_line": context.stats.human_line(),
    });
    // The §11.5 governor keys ride the live extras only — a dry run has
    // no governor, and the dry JSON schema stays byte-stable.
    if let Some(summary) = governor_summary.as_ref()
        && let Some(map) = extras.as_object_mut()
    {
        map.insert(
            "governor_line".to_owned(),
            serde_json::Value::String(summary.line()),
        );
        map.insert("governor".to_owned(), summary.json());
    }
    render_pr_report(
        cli,
        &title,
        &branch,
        &states,
        &report,
        body.as_deref(),
        &extras,
    );
    Ok(())
}

/// The pending-release JSON rows (the pr report's `pending_releases`):
/// package, version, tag and the §9.4 notes keyed by tag.
fn pending_rows_json(
    pending: &[halyard_core::PendingRelease],
    notes_by_tag: &std::collections::BTreeMap<String, String>,
) -> Vec<serde_json::Value> {
    pending
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
        .collect()
}

/// The `pr` report rendering (all three formats) — the shell's output
/// surface, split out so the machine pass stays the only logic in
/// `cmd_pr`.
/// The `dry` flag rides the extras document (the report view is the
/// single bundle the shell's renderer consumes).
fn view_dry(extras: &serde_json::Value) -> bool {
    extras
        .get("dry")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or_default()
}

/// The precomputed walk-provenance line the caller bakes into
/// `extras` (the human/gha tail every report carries).
fn walk_line(extras: &serde_json::Value) -> String {
    extras
        .get("walk_line")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// The precomputed governor line the caller bakes into `extras` (the
/// live reports' human/gha tail; absent on dry runs — no governor, no
/// line, M5.5).
fn governor_line(extras: &serde_json::Value) -> Option<String> {
    extras
        .get("governor_line")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

fn render_pr_report(
    cli: &Cli,
    title: &str,
    branch: &halyard_forge::BranchName,
    states: &[halyard_core::PackageTagState],
    report: &halyard_forge::MachineReport,
    body: Option<&str>,
    extras: &serde_json::Value,
) {
    match cli.format {
        Format::Json => {
            let effects: Vec<serde_json::Value> =
                report.effects.iter().map(render::pr_effect_json).collect();
            let mut document = serde_json::json!({
                "mode": if view_dry(extras) { "dry" } else { "live" },
                "title": title,
                "branch": branch.as_str(),
                "propagated": report.propagated,
                "effects": effects,
                "calls": report.calls,
                "body": body,
                "repo": extras.get("repo"),
                "pending_plan": extras.get("pending_plan"),
                "pending_releases": extras.get("pending_releases"),
                "history": extras.get("history"),
            });
            // The §11.5 governor summary rides the live document only —
            // a dry run has no governor to report (the dry JSON schema
            // stays byte-stable).
            if let Some(governor) = extras.get("governor")
                && let Some(map) = document.as_object_mut()
            {
                map.insert("governor".to_owned(), governor.clone());
            }
            let _ = print_pretty_json(&document);
        }
        Format::Gha => {
            let mut rendered = if view_dry(extras) {
                render::pr_dry_gha(title, branch.as_str(), states, report, body)
            } else {
                render::pr_live_gha(title, branch.as_str(), states, report, body)
            };
            rendered.push_str(&walk_line(extras));
            rendered.push('\n');
            if let Some(line) = governor_line(extras) {
                rendered.push_str(&line);
                rendered.push('\n');
            }
            print!("{rendered}");
        }
        Format::Human => {
            let mut rendered = if view_dry(extras) {
                render::pr_dry_human(title, branch.as_str(), states, report, body)
            } else {
                render::pr_live_human(title, branch.as_str(), states, report, body)
            };
            rendered.push_str(&walk_line(extras));
            rendered.push('\n');
            if let Some(line) = governor_line(extras) {
                rendered.push_str(&line);
                rendered.push('\n');
            }
            print!("{rendered}");
        }
    }
}

/// The live forge wire-up's environment-derived credentials (ADR-0013,
/// ADR-0014): the GitHub App env (`HALYARD_APP_ID` +
/// `HALYARD_APP_PRIVATE_KEY`/`HALYARD_APP_PRIVATE_KEY_PATH` +
/// `HALYARD_APP_INSTALLATION_ID`) wins when present — forge actions
/// taken with an installation token trigger the repository's own
/// workflows, which a static `GITHUB_TOKEN` does not (§11.5) — else the
/// token (`HALYARD_FORGE_TOKEN` first — the nightly smoke job's
/// contract — then `GITHUB_TOKEN`). The `owner/repo` coordinates come
/// from the configured `origin` (the changelog crate's remote-shape
/// grammar, the same parser the compare link rides) and the API base
/// (`GITHUB_API_URL` override, else github.com — GitHub Enterprise and
/// the loopback mock lane both ride this).
struct ForgeEnv {
    endpoint: ForgeEndpoint,
    credentials: halyard_forge_github::ForgeCredentials,
}

/// The GitHub App env, validated as a complete set: any `HALYARD_APP_*`
/// variable present commits the run to App mode — a partial set fails
/// closed naming what is missing (never a guessed fallback to the
/// static token). The private key rides `HALYARD_APP_PRIVATE_KEY_PATH`
/// (a PEM file — the shape GitHub's App settings download) or inline in
/// `HALYARD_APP_PRIVATE_KEY`.
fn app_credentials_from_env() -> Result<Option<halyard_forge_github::AppCredentials>, CliError> {
    let app_id = std::env::var("HALYARD_APP_ID")
        .ok()
        .filter(|value| !value.is_empty());
    let key_inline = std::env::var("HALYARD_APP_PRIVATE_KEY")
        .ok()
        .filter(|value| !value.is_empty());
    let key_path = std::env::var("HALYARD_APP_PRIVATE_KEY_PATH")
        .ok()
        .filter(|value| !value.is_empty());
    let installation = std::env::var("HALYARD_APP_INSTALLATION_ID")
        .ok()
        .filter(|value| !value.is_empty());
    if app_id.is_none() && key_inline.is_none() && key_path.is_none() && installation.is_none() {
        return Ok(None);
    }
    let mut missing: Vec<&str> = Vec::new();
    if app_id.is_none() {
        missing.push("HALYARD_APP_ID");
    }
    if key_inline.is_none() && key_path.is_none() {
        missing.push("HALYARD_APP_PRIVATE_KEY (or HALYARD_APP_PRIVATE_KEY_PATH)");
    }
    if installation.is_none() {
        missing.push("HALYARD_APP_INSTALLATION_ID");
    }
    if !missing.is_empty() {
        return Err(CliError::new(
            "E-FORGE-AUTH",
            format!(
                "the GitHub App env is incomplete (App auth is committed once any \
                 HALYARD_APP_* variable is set): missing {}",
                missing.join(", ")
            ),
        ));
    }
    let app_id = app_id.unwrap_or_default();
    let installation_id: u64 = installation.unwrap_or_default().parse().map_err(|_| {
        CliError::new(
            "E-FORGE-AUTH",
            "HALYARD_APP_INSTALLATION_ID must be the numeric installation id \
                 (the URL of the App's installation page under the organization)",
        )
    })?;
    let pem = if let Some(inline) = key_inline {
        inline
    } else {
        std::fs::read_to_string(key_path.unwrap_or_default()).map_err(|error| {
            CliError::new(
                "E-FORGE-AUTH",
                format!("the App private key PEM could not be read: {error}"),
            )
        })?
    };
    let credentials =
        halyard_forge_github::AppCredentials::from_pem(&app_id, &pem, installation_id)
            .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    Ok(Some(credentials))
}

fn forge_credentials() -> Result<ForgeEnv, CliError> {
    let credentials = if let Some(app) = app_credentials_from_env()? {
        halyard_forge_github::ForgeCredentials::App(Box::new(app))
    } else {
        let token = std::env::var("HALYARD_FORGE_TOKEN")
            .ok()
            .filter(|value| !value.is_empty())
            .or_else(|| {
                std::env::var("GITHUB_TOKEN")
                    .ok()
                    .filter(|value| !value.is_empty())
            })
            .ok_or_else(|| {
                CliError::new(
                    "E-FORGE-AUTH",
                    "live forge mode needs credentials: configure the GitHub App env \
                         (HALYARD_APP_ID + HALYARD_APP_PRIVATE_KEY_PATH + \
                         HALYARD_APP_INSTALLATION_ID) or export GITHUB_TOKEN (or \
                         HALYARD_FORGE_TOKEN) with repo access, and re-run `halyard doctor` \
                         to verify",
                )
            })?;
        halyard_forge_github::ForgeCredentials::from_token(&token)
    };
    let remote_url = remote_origin_url()?;
    let slug = remote_url.as_deref().and_then(halyard_changelog::repo_slug);
    let (owner, repo) = slug.ok_or_else(|| {
        CliError::new(
            "E-CLI-PR",
            "live forge mode needs a GitHub origin: set remote.origin.url to a GitHub \
             remote (the adapter is the M4.1b GitHub REST one; foreign forges are Year-2 \
             milestones)",
        )
    })?;
    let api_base = std::env::var("GITHUB_API_URL")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "https://api.github.com".to_owned());
    let endpoint = ForgeEndpoint::with_base(&owner, &repo, &api_base)
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    Ok(ForgeEnv {
        endpoint,
        credentials,
    })
}

/// The live mode's adapter construction: the REST adapter over the
/// resolved credentials — token credentials keep the M4.1b behavior,
/// App credentials mint their installation token through the same
/// transport (ADR-0014).
fn live_forge(env: ForgeEnv) -> GitHubRestForge<UreqTransport> {
    GitHubRestForge::with_credentials(env.endpoint, env.credentials, UreqTransport::new())
}

/// The §11.5 call governor's shell policy (M5.5, ADR-0015): the
/// production shape ([`halyard_forge::GovernorPolicy::shell_default`])
/// with three `HALYARD_FORGE_*` overrides — the same env layer the App
/// credentials ride (§16.4). Absent variables keep the shell default;
/// a present-but-invalid value fails closed naming the variable and the
/// accepted shape (`E-FORGE-REQUEST`), never a guessed fallback.
pub(crate) fn governor_policy_from_env() -> Result<halyard_forge::GovernorPolicy, CliError> {
    let mut policy = halyard_forge::GovernorPolicy::shell_default();
    if let Some(raw) = env_nonempty("HALYARD_FORGE_MIN_CALL_SPACING_MS") {
        policy.min_spacing_ms = parse_env_u64("HALYARD_FORGE_MIN_CALL_SPACING_MS", &raw)?;
    }
    if let Some(raw) = env_nonempty("HALYARD_FORGE_MAX_CALLS") {
        policy.max_calls = Some(parse_env_u32("HALYARD_FORGE_MAX_CALLS", &raw)?);
    }
    if let Some(raw) = env_nonempty("HALYARD_FORGE_MAX_RETRIES") {
        policy.max_retries = parse_env_u32("HALYARD_FORGE_MAX_RETRIES", &raw)?;
    }
    Ok(policy)
}

/// Reads an environment variable, treating empty exactly like unset
/// (the App-env grammar's convention).
fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

/// Parses a millisecond/non-negative-integer env value fail-closed.
fn parse_env_u64(name: &str, raw: &str) -> Result<u64, CliError> {
    raw.parse::<u64>().map_err(|_| {
        CliError::new(
            "E-FORGE-REQUEST",
            format!("{name} must be a non-negative integer (milliseconds), got {raw:?}"),
        )
    })
}

/// Parses a count env value fail-closed.
fn parse_env_u32(name: &str, raw: &str) -> Result<u32, CliError> {
    raw.parse::<u32>().map_err(|_| {
        CliError::new(
            "E-FORGE-REQUEST",
            format!("{name} must be a non-negative integer, got {raw:?}"),
        )
    })
}

/// The smoke probe's target: an explicit `--repo OWNER/REPO`, else
/// `GITHUB_REPOSITORY` (the Actions convention), else the configured
/// origin's GitHub slug; the API base rides `--api-base` /
/// `GITHUB_API_URL`, defaulting to github.com (GitHub Enterprise and
/// the loopback mock lane override it).
fn resolve_smoke_endpoint(
    repo: Option<&str>,
    api_base: Option<&str>,
) -> Result<(ForgeEndpoint, String, String), CliError> {
    let slug = repo
        .map(str::to_owned)
        .or_else(|| std::env::var("GITHUB_REPOSITORY").ok())
        .or_else(|| {
            remote_origin_url()
                .ok()
                .flatten()
                .and_then(|url| halyard_changelog::repo_slug(&url))
                .map(|(owner, name)| format!("{owner}/{name}"))
        })
        .ok_or_else(|| {
            CliError::new(
                "E-CLI-PR",
                "forge-smoke needs a repository: pass --repo OWNER/REPO, set \
                 GITHUB_REPOSITORY, or configure a GitHub remote.origin.url",
            )
        })?;
    let (owner, name) = slug
        .split_once('/')
        .filter(|(owner, name)| !owner.is_empty() && !name.is_empty())
        .ok_or_else(|| {
            CliError::new(
                "E-FORGE-REQUEST",
                format!("the repository slug {slug:?} must be OWNER/REPO"),
            )
        })?;
    let base = api_base
        .map(str::to_owned)
        .or_else(|| {
            std::env::var("GITHUB_API_URL")
                .ok()
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| "https://api.github.com".to_owned());
    let endpoint = ForgeEndpoint::with_base(owner, name, &base)
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    Ok((endpoint, slug, base))
}

/// The `forge-smoke` command (ADR-0013): the read-only probe the
/// nightly job invokes (`HALYARD_FORGE_TOKEN`). Builds the GitHub REST
/// adapter from the environment and fires one existence probe — a GET
/// on a synthetic tag, the documented 404→`false` shape — then prints
/// the rate snapshot and the §11.5 call log. Nothing is written: the
/// smoke proves reachability and auth, never mutates (I1 trivially).
/// Failures exit with the adapter's §15.4 code (`E-FORGE-*`).
fn cmd_forge_smoke(
    cli: &Cli,
    repo: Option<&str>,
    api_base: Option<&str>,
) -> Result<ExitCode, CliError> {
    let (endpoint, slug, base) = resolve_smoke_endpoint(repo, api_base)?;
    let credentials = if let Some(app) = app_credentials_from_env()? {
        halyard_forge_github::ForgeCredentials::App(Box::new(app))
    } else {
        let token = std::env::var("HALYARD_FORGE_TOKEN")
            .ok()
            .filter(|value| !value.is_empty())
            .or_else(|| {
                std::env::var("GITHUB_TOKEN")
                    .ok()
                    .filter(|value| !value.is_empty())
            })
            .ok_or_else(|| {
                CliError::new(
                    "E-FORGE-AUTH",
                    "forge-smoke needs read-capable credentials: configure the GitHub App \
                     env (HALYARD_APP_ID + HALYARD_APP_PRIVATE_KEY_PATH + \
                     HALYARD_APP_INSTALLATION_ID) or set HALYARD_FORGE_TOKEN (the nightly \
                     job's secret) or GITHUB_TOKEN",
                )
            })?;
        halyard_forge_github::ForgeCredentials::from_token(&token)
    };
    let auth_mode = match &credentials {
        halyard_forge_github::ForgeCredentials::Token(_) => "token",
        halyard_forge_github::ForgeCredentials::App(_) => "app",
    };
    let forge = GitHubRestForge::with_credentials(endpoint, credentials, UreqTransport::new());
    // The probe rides the §11.5 call governor too (M5.5): the smoke's
    // one GET is budgeted and retryable like any live call, and the
    // report carries the resolved policy + spend (the nightly job's
    // JSON keeps its `auth_mode` shape, with `governor` added).
    let (release_exists, rate, calls, governor_summary) = governed_smoke_probe(&forge)?;
    let probe_tag = SMOKE_PROBE_TAG;
    let call_rows = call_log_rows(&calls);
    match cli.format {
        Format::Json => print_pretty_json(&serde_json::json!({
            "mode": "smoke",
            "repo": slug,
            "api_base": base,
            "auth_mode": auth_mode,
            "probe_tag": probe_tag,
            "release_exists": release_exists,
            "rate_limit": { "limit": rate.limit, "remaining": rate.remaining },
            "calls": call_rows,
            "governor": governor_summary.json(),
        }))?,
        Format::Gha => {
            let mut out = String::new();
            let _ = writeln!(out, "forge smoke: {slug} @ {base}");
            let _ = writeln!(
                out,
                "- probe {probe_tag}: {}",
                if release_exists {
                    "present"
                } else {
                    "absent (expected)"
                }
            );
            let _ = writeln!(
                out,
                "- rate limit observed: {}/{}",
                rate.remaining, rate.limit
            );
            let _ = writeln!(out, "- calls: {}", calls.len());
            for record in &calls {
                let _ = writeln!(out, "- {:>5} {}", record.method, record.target);
            }
            let _ = writeln!(out, "- {}", governor_summary.line());
            print!("{out}");
        }
        Format::Human => {
            let mut out = String::new();
            let _ = writeln!(out, "forge smoke: {slug} @ {base}");
            let _ = writeln!(
                out,
                "release probe {probe_tag}: {}",
                if release_exists {
                    "present"
                } else {
                    "absent (expected — the probe tag is never created)"
                }
            );
            let _ = writeln!(
                out,
                "rate limit (observed headers): {}/{} remaining/limit",
                rate.remaining, rate.limit
            );
            let _ = writeln!(out, "calls ({}):", calls.len());
            for record in &calls {
                let _ = writeln!(out, "  {:>5} {}", record.method, record.target);
            }
            let _ = writeln!(out, "{}", governor_summary.line());
            print!("{out}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// The §11.5 call-log rows (the smoke JSON's `calls` array): method and
/// target per recorded forge operation, oldest first.
fn call_log_rows(calls: &[halyard_forge::CallRecord]) -> Vec<serde_json::Value> {
    calls
        .iter()
        .map(|record| {
            serde_json::json!({
                "method": record.method,
                "target": record.target,
            })
        })
        .collect()
}

/// The synthetic tag the smoke probe tests existence for (never created —
/// the documented 404→`false` shape).
const SMOKE_PROBE_TAG: &str = "halyard-forge-smoke-probe";

/// The smoke's governed probe (M5.5): the read-only existence GET on the
/// synthetic tag — a GET, answered by the documented 404→`false`
/// mapping; expected `false` on a healthy forge (the tag is never
/// created) — over the §11.5 governor, with the rate snapshot, the
/// adapter call ledger and the governor summary.
fn governed_smoke_probe(
    forge: &GitHubRestForge<UreqTransport>,
) -> Result<
    (
        bool,
        halyard_forge::RateInfo,
        Vec<halyard_forge::CallRecord>,
        render::GovernorSummary,
    ),
    CliError,
> {
    let policy = governor_policy_from_env()?;
    let (clock, sleeper) = halyard_forge::governor::system_time();
    let governor = halyard_forge::Governor::new(forge, policy, clock, sleeper);
    let release_exists = governor
        .release_exists(SMOKE_PROBE_TAG)
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    let rate = forge.rate_limit();
    let calls = forge.calls();
    let summary = render::GovernorSummary::of(&governor);
    Ok((release_exists, rate, calls, summary))
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
/// The §8.8 smart minimal fetch (M4.6): probe → select → classify →
/// one minimal `git fetch` with the documented full-fetch fallback
/// (ADR-0016). Discovery and config only — deliberately **not** the
/// history walk: a shallow clone (the very state `fetch` exists to
/// repair) fails the walk with `E-GIT-SHALLOW`, so the command cannot
/// require it. The remote is `origin` (the same convention the §11.2
/// push phase uses); the base branch is the resolved config override,
/// defaulting to `HEAD` like the planner's own resolution.
fn cmd_fetch(cli: &Cli, profiler: &mut Profiler) -> Result<ExitCode, CliError> {
    let discovery = profiler
        .span("discover", || {
            discover_repo(std::path::Path::new("."), &DiscoverInput::default())
        })
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    let config = profiler.span("config", || resolve_config(cli))?;
    let plan_config: PlanConfig = config.plan_config();
    let base = plan_config
        .base_branch
        .clone()
        .unwrap_or_else(|| "HEAD".to_owned());
    let fetcher = halyard_git::fetch::GixTagFetcher::open(std::path::Path::new("."))
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    let report = profiler
        .span("fetch", || {
            fetcher.fetch_release_tags(&discovery.workspace, &plan_config, "origin", Some(&base))
        })
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    match cli.format {
        Format::Json => print_pretty_json(&fetch_json_document(&report))?,
        Format::Gha => print!("{}", render::fetch_gha(&report)),
        Format::Human => print!("{}", render::fetch_human(&report)),
    }
    // §15.4: a failed row fails the run (I2 — the re-run completes; the
    // tags that landed classify as AlreadyLocal next time).
    Ok(if report.failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// The `fetch` JSON document (stable field order via `serde_json::json!`;
/// built here so the renderers stay renderer-only, §7.2 style).
fn fetch_json_document(report: &halyard_git::fetch::FetchReport) -> serde_json::Value {
    let rows: Vec<serde_json::Value> = report
        .rows
        .iter()
        .map(|row| match row {
            halyard_git::fetch::FetchRow::Fetched { tag } => {
                serde_json::json!({ "tag": tag, "outcome": "fetched" })
            }
            halyard_git::fetch::FetchRow::AlreadyLocal { tag } => {
                serde_json::json!({ "tag": tag, "outcome": "already_local" })
            }
            halyard_git::fetch::FetchRow::Failed { tag, error } => {
                serde_json::json!({ "tag": tag, "outcome": "failed", "error": error.code() })
            }
        })
        .collect();
    let selection: serde_json::Map<String, serde_json::Value> = report
        .selection
        .iter()
        .map(|(package, tag)| (package.clone(), serde_json::Value::String(tag.clone())))
        .collect();
    serde_json::json!({
        "remote": report.remote,
        "base_branch": report.base_branch,
        "selection": serde_json::Value::Object(selection),
        "rows": rows,
        "argv": report.argv,
        "probe_tags": report.probe_tags,
        "fetched": report.fetched,
        "already_local": report.already_local,
        "failed": report.failed,
        "fallback_used": report.fallback_used,
    })
}

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
    // The forge half (§11.1 step 2, M4.1b): GET-by-tag-first release
    // creation over the GitHub REST adapter. Live mode only — the dry
    // run forecasts it as absent (I5); live runs without credentials or
    // against a foreign origin report an honest skip, never a fake.
    let forge_releases = if dry {
        render::ForgeReleases::Skipped {
            reason: "dry run — the forge half is forecast, not executed (I5)".to_owned(),
        }
    } else {
        forge_release_phase(&context, profiler, &pending)?
    };
    // I2: the phase is resumable — failures render as rows and fail the
    // run (§15.4 exit 1) so CI stops on a partial tag phase.
    let failed = report.failed > 0 || push.as_ref().is_some_and(|p| p.failed > 0);
    let head_hex = head.to_string();
    match cli.format {
        Format::Json => {
            let document = release_json_document(
                &ReleaseDocInput {
                    pending: &pending,
                    report: &report,
                    provenance: &provenance,
                    push: push.as_ref(),
                    forge_releases: &forge_releases,
                    history: &context.stats.json(),
                },
                dry,
                &head_hex,
            );
            print_pretty_json(&document)?;
        }
        Format::Gha => {
            let mut rendered = if dry {
                render::release_dry_gha(&head_hex, &states, &report, &provenance, push.as_ref())
            } else {
                render::release_live_gha(
                    &head_hex,
                    &states,
                    &report,
                    &provenance,
                    push.as_ref(),
                    &forge_releases,
                )
            };
            rendered.push_str(&context.stats.human_line());
            rendered.push('\n');
            print!("{rendered}");
        }
        Format::Human => {
            let mut rendered = if dry {
                render::release_dry_human(&head_hex, &states, &report, &provenance, push.as_ref())
            } else {
                render::release_live_human(
                    &head_hex,
                    &states,
                    &report,
                    &provenance,
                    push.as_ref(),
                    &forge_releases,
                )
            };
            rendered.push_str(&context.stats.human_line());
            rendered.push('\n');
            print!("{rendered}");
        }
    }
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// The live release's forge half (§11.1 step 2, M4.1b): the pending
/// releases ride the §11.1 machine's tag phase over the GitHub REST
/// adapter — GET-by-tag-first creation with the notes-pipeline payloads.
/// Missing credentials or a foreign origin is an honest skip rendered
/// into the report (the git-side half still ran); adapter failures fail
/// the run fail-closed (I2: the re-run completes — GET finds what
/// landed).
fn forge_release_phase(
    context: &Context,
    profiler: &mut Profiler,
    pending: &[halyard_core::PendingRelease],
) -> Result<render::ForgeReleases, CliError> {
    let credentials = if let Some(app) = app_credentials_from_env()? {
        halyard_forge_github::ForgeCredentials::App(Box::new(app))
    } else {
        let token = std::env::var("HALYARD_FORGE_TOKEN")
            .ok()
            .filter(|value| !value.is_empty())
            .or_else(|| {
                std::env::var("GITHUB_TOKEN")
                    .ok()
                    .filter(|value| !value.is_empty())
            });
        let Some(token) = token else {
            return Ok(render::ForgeReleases::Skipped {
                reason: "no forge credentials (configure the GitHub App env or export \
                         GITHUB_TOKEN to create forge releases)"
                    .to_owned(),
            });
        };
        halyard_forge_github::ForgeCredentials::from_token(&token)
    };
    let remote_url = remote_origin_url()?;
    let slug = remote_url.as_deref().and_then(halyard_changelog::repo_slug);
    let Some((owner, repo)) = slug else {
        return Ok(render::ForgeReleases::Skipped {
            reason: "origin is not a GitHub remote — forge releases are skipped, never faked"
                .to_owned(),
        });
    };
    let api_base = std::env::var("GITHUB_API_URL")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "https://api.github.com".to_owned());
    let endpoint = ForgeEndpoint::with_base(&owner, &repo, &api_base)
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    let forge = GitHubRestForge::with_credentials(endpoint, credentials, UreqTransport::new());
    // The notes payloads ride the same notes pipeline `pr --dry` uses;
    // the plan feeds the entry-preview fallback (no fabrication).
    let plan_config: PlanConfig = context.config.plan_config();
    let plan = plan_from_context(context, profiler)?;
    let (states, _) = reconcile_snapshot(context, &plan_config, profiler);
    let remote = remote_origin_url()?;
    let specs = pending_specs(
        pending,
        &plan,
        &states,
        &context.discovery.workspace,
        remote.as_deref(),
    )?;
    let branch = halyard_forge::BranchName::new(&format!("halyard/release/{}", plan.base_branch))
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    // The forge half rides the §11.5 call governor too (M5.5): the tag
    // phase's per-package `create_release` calls are content creation —
    // paced, retryable, budgeted (§11.5: serialize content-creating
    // calls).
    let policy = governor_policy_from_env()?;
    let (clock, sleeper) = halyard_forge::governor::system_time();
    let governor = halyard_forge::Governor::new(&forge, policy, clock, sleeper);
    let report = profiler
        .span("release-forge-phase", || {
            halyard_forge::run_release_machine(
                &governor,
                &halyard_forge::MachineInput {
                    branch,
                    pending_releases: specs,
                    // The tag phase owns this pass (§11.1 step 2); the
                    // Release PR is the `pr` command's prepare phase.
                    release_pr: None,
                },
            )
        })
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    Ok(render::ForgeReleases::Ran {
        repo: format!("{owner}/{repo}"),
        effects: report.effects,
        calls: report.calls,
        governor: render::GovernorSummary::of(&governor),
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

/// The release document's inputs (§7.2 style: the renderers stay
/// renderer-only, so the shell passes one bundle instead of a
/// positional pile — `forge_releases` is the real outcome in both
/// modes, the dry run's forecast skip included).
struct ReleaseDocInput<'a> {
    pending: &'a [halyard_core::PendingRelease],
    report: &'a halyard_git::tag::TagPhaseReport,
    provenance: &'a render::TargetProvenanceMap,
    push: Option<&'a halyard_git::push::TagPushReport>,
    forge_releases: &'a render::ForgeReleases,
    history: &'a serde_json::Value,
}

/// The `release` JSON document (stable field order via `serde_json::json!`;
/// built here so the renderers stay renderer-only, §7.2 style). The live
/// document carries the forge half's real outcome — ran over the M4.1b
/// adapter or an honest skip.
fn release_json_document(
    input: &ReleaseDocInput<'_>,
    dry: bool,
    head_hex: &str,
) -> serde_json::Value {
    let pending_rows: Vec<serde_json::Value> = input
        .pending
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
    let directive_rows: Vec<serde_json::Value> = input
        .report
        .rows
        .iter()
        .map(|row| render::release_row_json(row, input.provenance, created_key))
        .collect();
    if dry {
        serde_json::json!({
            "mode": "dry",
            "head": head_hex,
            "history": input.history,
            "pending_releases": pending_rows,
            "directives": directive_rows,
            "created": input.report.created,
            "already_tagged": input.report.already_tagged,
            "failed": input.report.failed,
            "push": render::release_push_json(input.push, dry),
            "forge_releases": render::forge_releases_json(input.forge_releases),
        })
    } else {
        serde_json::json!({
            "mode": "live",
            "head": head_hex,
            "history": input.history,
            "pending_releases": pending_rows,
            "directives": directive_rows,
            "created": input.report.created,
            "already_tagged": input.report.already_tagged,
            "failed": input.report.failed,
            "push": render::release_push_json(input.push, dry),
            "forge_releases": render::forge_releases_json(input.forge_releases),
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
/// The `publish` command (M5.2 step 2, §13): the shell around the
/// `halyard-publish` engine — plan → publication → run, with the
/// per-ecosystem spawn effect (the adapter crates' argv tables) and the
/// live registry routing (the two §13 clients over the blocking
/// transport, plus the configured custom cargo registries resolved
/// from the cargo configuration, ADR-0021). `--dry` is the §13.8
/// forecast: the tools run their
/// documented dry form, the registry is never probed (I5), and the
/// report carries `Forecast` rows. Live failures are rows (§13.7); the
/// command exits 4 on a partial publish (§15.4), 1 when nothing
/// landed, and refuses a live run under `--offline` (`E-CLI-PUBLISH`).
fn cmd_publish(cli: &Cli, profiler: &mut Profiler, dry: bool) -> Result<ExitCode, CliError> {
    if !dry && cli.offline {
        return Err(CliError::new(
            "E-CLI-PUBLISH",
            "--offline contradicts a live publish (the resume probes and the tool upload are \
             the live run's network) — use `publish --dry` for the offline forecast, or drop \
             --offline for the real run",
        ));
    }
    // The shell captures the publish-auth snapshot once (M5.3 step 1,
    // ADR-0019): resolution and scrubbing share one deterministic table,
    // and a present-but-empty token variable refuses the run here —
    // before any plan, probe or spawn (E-PUB-AUTH).
    let auth = halyard_publish::EnvPublishAuth::capture()
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    let context = context(cli, profiler)?;
    let plan = plan_from_context(&context, profiler)?;
    let publication = profiler
        .span("publish-plan", || {
            halyard_publish::plan_publication(&plan, &context.discovery.workspace)
        })
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    let mode = if dry {
        halyard_publish::PublishMode::Dry
    } else {
        halyard_publish::PublishMode::Live
    };
    let policy = halyard_publish::PublishPolicy::new(
        halyard_publish::PublishPolicy::default_visibility(),
        1,
    )
    .in_mode(mode);
    // The routing resolution (ADR-0021) reads the cargo configuration
    // before any call: configured custom registries join the map, an
    // unresolvable name refuses the run closed — zero probes, zero
    // spawns (validate-then-act, §15.3).
    let router = publish_effect::publish_router(std::path::Path::new("."), &publication.targets)
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    let auth = std::sync::Arc::new(auth);
    let effect =
        publish_effect::SpawnPublishEffect::new(std::path::PathBuf::from("."), dry, auth.clone());
    // The per-registry auth provenance (M5.3 step 2): which documented
    // variable authenticates each registry — resolved before the run,
    // rendered in every format (names only, never values).
    let auth_map = halyard_publish::auth_summary(auth.as_ref(), &publication.targets)
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    let report = profiler
        .span("publish-run", || {
            halyard_publish::run_publication(
                &publication,
                &router,
                &effect,
                &policy,
                &publish_effect::ThreadSleeper,
            )
        })
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    let mode_label = if dry { "dry" } else { "live" };
    match cli.format {
        Format::Json => print_pretty_json(&json_value(&render::publish_report_json(
            &report,
            &publication.not_targets,
            mode_label,
            &auth_map,
        ))?)?,
        Format::Gha | Format::Human => {
            print!("{}", render::publish_report(&report, mode_label, &auth_map));
        }
    }
    if report.failed == 0 {
        return Ok(ExitCode::SUCCESS);
    }
    if !dry && report.published + report.already_published > 0 {
        // §15.4 exit codes: 4 = partial publish (retryable) — the
        // resume re-run continues from the first failure.
        Ok(ExitCode::from(4))
    } else {
        Ok(ExitCode::FAILURE)
    }
}

/// The `migrate` command (M4.7, §16.2): surveys the foreign
/// release-tool configuration in the working directory into a migration
/// report. The shell owns the I/O (each source declares the files it
/// reads; a missing file is absence, an unreadable one refuses), the
/// engine maps and validates (ADR-0022), the renderers surface the
/// mapped rows, the semantic differences and the validated draft.
/// Without `--write` the command is report-only: nothing is written.
/// `--write` (M4.7 step 2, ADR-0023) plans the write over the report
/// and lands the validated draft at `halyard.toml` through the
/// [`halyard_config::AtomicDraftSink`] — atomic, create-only, and
/// fail-closed on every guess: an existing config is never clobbered,
/// two configured tools refuse, nothing configured is an honest no-op
/// (`E-CFG-MIGRATE` for the refusals).
fn cmd_migrate(cli: &Cli, profiler: &mut Profiler, write: bool) -> Result<(), CliError> {
    let sources = halyard_config::default_sources();
    let mut files = halyard_config::ForeignFiles::new();
    for source in &sources {
        for path in source.config_paths() {
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    files.insert(*path, content);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(CliError::new(
                        halyard_config::ConfigError::MIGRATE_CODE,
                        format!(
                            "cannot read {path}: {error} — fix the file's permissions; the \
                             migration refuses a file it cannot survey"
                        ),
                    ));
                }
            }
        }
    }
    let report = profiler
        .span("migrate-survey", || {
            halyard_config::migrate(&files, &sources)
        })
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    // The write step (M4.7 step 2): the pure plan first, then the
    // effect over the real sink — validate-then-act (§15.3), so a
    // refusal happens before anything touches the disk.
    let write_report = if write {
        let plan = profiler
            .span("migrate-write-plan", || {
                halyard_config::plan_draft_write(&report)
            })
            .map_err(|e| CliError::new(e.code(), e.to_string()))?;
        Some(
            profiler
                .span("migrate-write-run", || {
                    halyard_config::run_draft_write(plan, &halyard_config::AtomicDraftSink)
                })
                .map_err(|e| CliError::new(e.code(), e.to_string()))?,
        )
    } else {
        None
    };
    match cli.format {
        Format::Json => {
            let value =
                render::migrate_report_json(&report, write_report.as_ref()).map_err(|e| {
                    CliError::new(
                        halyard_config::ConfigError::MIGRATE_CODE,
                        format!("the migration report does not serialize: {e}"),
                    )
                })?;
            print_pretty_json(&value)?;
        }
        Format::Gha => print!(
            "{}",
            render::migrate_report_gha(&report, write_report.as_ref())
        ),
        Format::Human => print!("{}", render::migrate_report(&report, write_report.as_ref())),
    }
    Ok(())
}

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
fn cmd_doctor(cli: &Cli, profiler: &mut Profiler, registries: bool) -> Result<(), CliError> {
    let report = doctor::check(cli, profiler, registries)?;
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
///
/// The comment mode (`--comment`, M4.5 step 1) posts the verdict to the
/// PR under review through the forge seam's marker-based upsert — the
/// §11.6 "if merged, this will release" comment. The target is the open
/// PR whose head branch matches the run's head branch
/// (`GITHUB_HEAD_REF`, else the checked-out branch); a rejected title
/// posts its diagnosis too, then still fails the command. Live-only:
/// without credentials the decline is honest (`E-CLI-COMMENT`), never a
/// pretended post.
fn cmd_check(
    cli: &Cli,
    profiler: &mut Profiler,
    pr_title: Option<&str>,
    comment: bool,
    exit_code: bool,
) -> Result<ExitCode, CliError> {
    // The comment mode needs a verdict to post — refused fail-closed
    // before any forge contact (the fix names the missing flag).
    if comment && pr_title.is_none() {
        return Err(CliError::new(
            "E-CLI-COMMENT",
            "`check --comment` posts the verdict of a linted title, but no title was given — \
             pass `--pr-title <TITLE>` (the comment mode has nothing to say without one)",
        ));
    }
    let context = context(cli, profiler)?;
    let plan = plan_from_context(&context, profiler)?;
    let pending = !plan.releases.is_empty();
    let plan_config: PlanConfig = context.config.plan_config();
    let Some(title) = pr_title else {
        render_check_pending(cli, &plan, pending)?;
        return Ok(pending_exit(exit_code, pending));
    };
    // The single classification (the exit wording and the posted
    // comment share one source, check.rs), then one comment post before
    // any output or exit — the post failure is the command's failure,
    // never a pretended success.
    let classification = check::classify_title(title, &plan_config);
    let predictions = match &classification {
        check::TitleClassification::Accepted(lint) => check::predict(&plan, lint, &plan_config),
        check::TitleClassification::Rejected(_) => Vec::new(),
    };
    let comment_outcome = if comment {
        Some(run_check_comment(&check::comment_input(
            title,
            &classification,
            &predictions,
        ))?)
    } else {
        None
    };
    match classification {
        check::TitleClassification::Rejected(message) => {
            return Err(CliError::new("E-CLI-TITLE", message));
        }
        check::TitleClassification::Accepted(lint) => {
            render_check_accepted(
                cli,
                title,
                &lint,
                &predictions,
                pending,
                comment_outcome.as_ref(),
            )?;
        }
    }
    Ok(pending_exit(exit_code, pending))
}

/// The pending-releases render (the no-title path).
fn render_check_pending(
    cli: &Cli,
    plan: &halyard_core::Plan,
    pending: bool,
) -> Result<(), CliError> {
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
            print_pretty_json(&serde_json::json!({ "pending": pending, "releases": releases }))?;
        }
        Format::Gha => print!("{}", render::check_pending_gha(plan)),
        Format::Human => print!("{}", render::check_pending_human(plan)),
    }
    Ok(())
}

/// The accepted-verdict renders (json/human/gha); the `comment` block
/// rides only when the comment mode ran (byte-stability otherwise).
fn render_check_accepted(
    cli: &Cli,
    title: &str,
    lint: &halyard_conventional::TitleLint,
    predictions: &[check::Prediction],
    pending: bool,
    comment_outcome: Option<&CheckCommentOutcome>,
) -> Result<(), CliError> {
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
            // Built as a map first so the conditional `comment` block
            // inserts into a known object — no `unreachable!` panic
            // surface on the live JSON print path.
            let mut document = serde_json::Map::new();
            document.insert("title".to_owned(), serde_json::json!(title));
            document.insert("verdict".to_owned(), serde_json::json!("accepted"));
            document.insert("kind".to_owned(), serde_json::json!(lint.kind));
            document.insert("scope".to_owned(), serde_json::json!(lint.scope));
            document.insert("breaking".to_owned(), serde_json::json!(lint.breaking));
            document.insert("level".to_owned(), serde_json::json!(lint.level.as_str()));
            document.insert("predicted".to_owned(), serde_json::json!(predicted));
            document.insert("pending".to_owned(), serde_json::json!(pending));
            if let Some(outcome) = comment_outcome {
                document.insert("comment".to_owned(), comment_json(outcome));
            }
            print_pretty_json(&serde_json::Value::Object(document))?;
        }
        Format::Gha => {
            print!("{}", render::check_gha(title, lint, predictions));
            if let Some(outcome) = comment_outcome {
                print!("{}", comment_line(outcome));
            }
        }
        Format::Human => {
            print!("{}", render::check_human(title, lint, predictions));
            if let Some(outcome) = comment_outcome {
                print!("{}", comment_line(outcome));
            }
        }
    }
    Ok(())
}

/// The §15.4 exit code: 3 = a release is pending (the `check
/// --exit-code` CI gate); title-lint failures exit 1 via `CliError`.
fn pending_exit(exit_code: bool, pending: bool) -> ExitCode {
    if exit_code && pending {
        ExitCode::from(3)
    } else {
        ExitCode::SUCCESS
    }
}

/// The §11.6 comment mode's outcome (M4.5 step 1): the posted comment or
/// the honest skip.
enum CheckCommentOutcome {
    /// The comment was posted (`created`) on PR `pr` at `url`.
    Posted {
        /// The PR the comment lives on.
        pr: u64,
        /// The forge's comment URL.
        url: String,
        /// `true` on the fresh create, `false` on the in-place rewrite
        /// (I1 — the marker re-run never duplicates).
        created: bool,
    },
    /// No open PR matched the head branch: nothing posted, the skip is
    /// the §11.4 not-found-is-a-value convention surfaced.
    Skipped {
        /// The head branch the probe used.
        head_branch: String,
    },
}

/// The §11.6 comment flow (M4.5 step 1): build the live forge (the
/// fail-closed `E-CLI-COMMENT` decline on missing credentials or a
/// foreign origin — the `pr` live gate's shape under the check mode's
/// own code), ride the §11.5 governor (the upsert is a content call:
/// paced, budgeted, retried), resolve the run's head branch, find the
/// open PR whose head it is (the §11.4 find-by-head probe) and
/// marker-upsert the rendered verdict. A forked PR's head (a foreign
/// owner prefix) resolves the same honest skip — the adapter's
/// `head=owner:branch` filter simply finds nothing. The adapter and the
/// governor both live inside this function: the run is one-shot, so no
/// borrow escapes.
fn run_check_comment(
    comment: &halyard_changelog::CheckComment,
) -> Result<CheckCommentOutcome, CliError> {
    let env = forge_credentials().map_err(|error| {
        CliError::new(
            "E-CLI-COMMENT",
            format!(
                "the check comment needs live forge credentials — {error}; configure the \
                 GitHub App env or export GITHUB_TOKEN (or HALYARD_FORGE_TOKEN) and \
                 re-run `halyard doctor` to verify"
            ),
        )
    })?;
    let inner: Box<dyn halyard_forge::Forge> = Box::new(live_forge(env));
    let (clock, sleeper) = halyard_forge::governor::system_time();
    let governor =
        halyard_forge::Governor::new(inner.as_ref(), governor_policy_from_env()?, clock, sleeper);
    let forge: &dyn halyard_forge::Forge = &governor;
    let head = comment_head_branch()?;
    let branch = halyard_forge::BranchName::new(&head)
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    let found = forge
        .find_release_pr(&branch)
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    let Some(pr) = found else {
        return Ok(CheckCommentOutcome::Skipped { head_branch: head });
    };
    let spec = halyard_forge::CommentSpec {
        pr_number: pr.number,
        marker: halyard_changelog::CHECK_MARKER.to_owned(),
        body: halyard_changelog::render_check_comment(comment),
    };
    let posted = forge
        .upsert_pr_comment(&spec)
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    Ok(CheckCommentOutcome::Posted {
        pr: pr.number,
        url: posted.url,
        created: posted.created,
    })
}

/// The JSON `comment` block (byte-stable: the key exists only when
/// `--comment` ran).
fn comment_json(outcome: &CheckCommentOutcome) -> serde_json::Value {
    match outcome {
        CheckCommentOutcome::Posted { pr, url, created } => serde_json::json!({
            "posted": true,
            "created": created,
            "url": url,
            "pr": pr,
        }),
        CheckCommentOutcome::Skipped { head_branch } => serde_json::json!({
            "posted": false,
            "reason": format!("no open PR on head branch {head_branch}"),
        }),
    }
}

/// The human/GHA tail line for the comment outcome.
fn comment_line(outcome: &CheckCommentOutcome) -> String {
    match outcome {
        CheckCommentOutcome::Posted { url, created, .. } => {
            let verb = if *created { "created" } else { "updated" };
            format!("check-comment: {verb} {url}\n")
        }
        CheckCommentOutcome::Skipped { head_branch } => {
            format!("check-comment: not posted — no open PR on head branch {head_branch}\n")
        }
    }
}

/// The run's head branch: `GITHUB_HEAD_REF` in Actions, else the
/// checked-out branch (in-process `gix` — no subprocess, I6). Detached
/// HEAD is an honest decline: the comment mode finds the PR by its head
/// branch, and a detached checkout names none.
fn comment_head_branch() -> Result<String, CliError> {
    if let Some(branch) = std::env::var("GITHUB_HEAD_REF")
        .ok()
        .filter(|b| !b.is_empty())
    {
        return Ok(branch);
    }
    let repo = halyard_git::gix::open(std::path::Path::new(".")).map_err(|e| {
        CliError::new(
            "E-GIT-OPEN",
            format!("cannot open the repository for the head-branch lookup: {e}"),
        )
    })?;
    let head = repo.head().map_err(|e| {
        CliError::new(
            "E-GIT-HEAD",
            format!("cannot resolve HEAD for the comment target: {e}"),
        )
    })?;
    let Some(referent) = head.try_into_referent() else {
        return Err(CliError::new(
            "E-CLI-COMMENT",
            "HEAD is detached — the comment mode finds the PR by its head branch; run in a \
             PR workflow (GITHUB_HEAD_REF carries it) or check out a branch",
        ));
    };
    // Branch names are UTF-8 in practice; a non-UTF-8 name cannot ride
    // the argv/env paths anyway — the honest decline, never a guessed
    // byte-to-string cast.
    std::str::from_utf8(referent.name().shorten())
        .map(str::to_owned)
        .map_err(|_| {
            CliError::new(
                "E-CLI-COMMENT",
                "the checked-out branch name is not valid UTF-8 — the comment mode cannot \
                 address it; run in a PR workflow (GITHUB_HEAD_REF carries the branch)",
            )
        })
}

/// The checked-out branch name, when HEAD names one (`GITHUB_HEAD_REF`
/// wins — the §11.5 workflow view of "the branch being planned").
/// Lenient by design: a detached or unborn HEAD, an unopenable repo or a
/// non-UTF-8 name is `None` — the §10.3 branch-bound channels (ADR-0025)
/// are an optimization of the unbound default, never a failure surface.
fn checked_out_branch() -> Option<String> {
    if let Some(branch) = std::env::var("GITHUB_HEAD_REF")
        .ok()
        .filter(|b| !b.is_empty())
    {
        return Some(branch);
    }
    let repo = halyard_git::gix::open(std::path::Path::new(".")).ok()?;
    let head = repo.head().ok()?;
    let referent = head.try_into_referent()?;
    std::str::from_utf8(referent.name().shorten())
        .ok()
        .map(str::to_owned)
}

/// The shared pipeline context: discovery + resolved config + git history.
struct Context {
    config: ResolvedConfig,
    discovery: halyard_discover::Discovery,
    history: GitHistory,
    /// The walk provenance the engine measured (the M2.6 stats wiring):
    /// rendered into every report, never re-measured.
    stats: render::HistoryStats,
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
    let mut config = profiler.span("config", || resolve_config(cli))?;
    // §10.3 branch-bound channels (ADR-0025): the checked-out branch
    // picks the channel — a binding wins, the unbound default fills the
    // rest. Applied before the cache key is computed, so the §8.7
    // `config_hash` carries the effective channel: planning `next`
    // (bound `rc`) and `main` (stable) never share attributions.
    if let Some(branch) = checked_out_branch() {
        config.prerelease_channel = config.channel_for_branch(Some(&branch));
        // §10.3 maintenance lines (ADR-0027): the checked-out branch
        // picks its pinned line the same way — applied before the
        // cache key, so a pinned branch and the default branch never
        // share attributions.
        config.maintenance_line = config.maintenance_line_for(Some(&branch));
    }
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
        stats: render::HistoryStats::of(&history),
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
    // The changelog stage (M2, §9.4): the resolved `changelog.style` —
    // a built-in name or a user-template path the shell reads (the
    // renderer stays pure, §4) — drives the §9.4 insertion machinery
    // per changelog file. Every file is pre-rendered in memory before
    // the first write (§9.1 all-or-none across the stage); the writes
    // go through the same atomic protocol as the plan edits, and the
    // re-run is byte-identical (I1: in-place section replacement).
    let changelog_rows = changelog_stage(&context, &plan)?;
    match cli.format {
        Format::Json => {
            print_pretty_json(&apply_report_json(
                &report,
                &native_refreshes,
                &changelog_rows,
            ))?;
        }
        Format::Gha | Format::Human => {
            print!(
                "{}",
                render::apply_report(&report, &native_refreshes, &changelog_rows)
            );
        }
    }
    Ok(())
}

/// The apply command's changelog stage (M2, §9.4): the plan's releases
/// group by their changelog file (a fixed group shares one document),
/// each group renders through the resolved style — [`halyard_changelog::StyleSource`]
/// splits built-in names from user-template paths, the shell reads the
/// template file — and the §9.4 insertion machinery anchors the section
/// after the marker or the `Unreleased` heading. A byte-identical
/// re-render is reported `unchanged` and never written (I1).
fn changelog_stage(
    context: &Context,
    plan: &halyard_core::Plan,
) -> Result<Vec<render::ChangelogRow>, CliError> {
    if plan.releases.is_empty() {
        return Ok(Vec::new());
    }
    let setting = context
        .config
        .changelog_style
        .as_deref()
        .unwrap_or("keep-a-changelog");
    let source = halyard_changelog::StyleSource::from_config(setting)
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
    // The template path is the shell's read (the renderer stays pure):
    // an unreadable file is the caller's config error, surfaced with
    // the template code before anything is written.
    let (style_label, template_source) = match &source {
        halyard_changelog::StyleSource::BuiltIn(style) => (style.name().to_owned(), None),
        halyard_changelog::StyleSource::Template(path) => {
            let text = std::fs::read_to_string(path).map_err(|error| {
                CliError::new(
                    "E-CHLOG-TEMPLATE",
                    format!("cannot read the changelog template {path:?}: {error}"),
                )
            })?;
            (format!("template:{path}"), Some(text))
        }
    };
    // Group by changelog file, stable in file order (I3). The file is
    // the package's configured changelog, or the same
    // `<root>/CHANGELOG.md` default `pending_specs` already establishes
    // (§5.3/§6.1) — the adapters leave `changelog` unset today, so the
    // default is the zero-config shape.
    let mut groups: std::collections::BTreeMap<String, Vec<&halyard_core::plan::PlanRelease>> =
        std::collections::BTreeMap::new();
    for release in &plan.releases {
        let file = changelog_file_for(&context.discovery.workspace, &release.package)?;
        groups.entry(file).or_default().push(release);
    }
    // Pre-render everything in memory (§9.1: all files render or none
    // is written), then stage the atomic writes.
    let mut staged: Vec<(String, String, &'static str)> = Vec::new();
    for (file, releases) in &groups {
        let existing = std::fs::read_to_string(file).ok();
        let rendered = match &template_source {
            Some(template) => halyard_changelog::render_releases_into_template(
                existing.as_deref(),
                releases,
                template,
            ),
            None => halyard_changelog::render_releases_into(existing.as_deref(), releases, setting),
        }
        .map_err(|e| CliError::new(e.code(), e.to_string()))?;
        let outcome = if existing.as_deref() == Some(rendered.as_str()) {
            "unchanged"
        } else if existing.is_none() {
            "created"
        } else {
            "updated"
        };
        staged.push((file.clone(), rendered, outcome));
    }
    let mut rows = Vec::with_capacity(staged.len());
    for (file, rendered, outcome) in staged {
        halyard_edit::atomic_write(std::path::Path::new(&file), rendered.as_bytes())
            .map_err(|e| CliError::new(e.code(), e.to_string()))?;
        rows.push(render::ChangelogRow {
            file,
            style: style_label.clone(),
            outcome,
        });
    }
    Ok(rows)
}

/// The changelog file a release writes: the package's configured
/// `changelog`, else the `<root>/CHANGELOG.md` default (the same
/// convention `pending_specs` rides for the release-notes extraction).
fn changelog_file_for(
    workspace: &halyard_core::package::Workspace,
    package: &str,
) -> Result<String, CliError> {
    let pkg = workspace
        .packages()
        .iter()
        .find(|pkg| workspace.interner().get(pkg.name) == Some(package))
        .ok_or_else(|| {
            CliError::new(
                "E-CLI-NO-PACKAGE",
                format!(
                    "plan release {package:?} names no workspace package (internal \
                     inconsistency)"
                ),
            )
        })?;
    Ok(pkg.changelog.as_ref().map_or_else(
        || {
            let root = pkg.root.as_str();
            if root.is_empty() {
                "CHANGELOG.md".to_owned()
            } else {
                format!("{root}/CHANGELOG.md")
            }
        },
        |path| path.as_str().to_owned(),
    ))
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
    changelog_rows: &[render::ChangelogRow],
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
        "changelog": changelog_rows.iter().map(render::changelog_row_json).collect::<Vec<_>>(),
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
