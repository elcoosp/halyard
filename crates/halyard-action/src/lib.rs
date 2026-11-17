//! The action-output seam (M10/§12 step 1, ADR-0037): the §12 GitHub
//! Action outputs vocabulary — `released`, `published`, `pr_number`,
//! `pr_url`, `releases_json` (array: name, version, tag, ecosystem,
//! url), `plan_json` (path), the per-package `core--version` /
//! `core--tag` keys and `paths_released` — rendered from the plan wire
//! contract (§5.8) over an injected [`ActionWriter`].
//!
//! Two writers over one trait (the injected-effect convention of
//! `PublishEffect`/`TagWriter`/`Forge`):
//!
//! * [`GithubActionsWriter`] — the real surface: the `GITHUB_OUTPUT`
//!   env file (the documented `{name}={value}` line, multi-line values
//!   in the documented heredoc `{name}<<{delimiter}` form, VERIFIED
//!   2026-11-17 against
//!   docs.github.com/en/actions/writing-workflows/choosing-what-your-workflow-does/workflow-commands-for-github-actions)
//!   and the `GITHUB_STEP_SUMMARY` env file (the §12 job summary,
//!   appended with the runner's automatic newline). The `::warning::`
//!   annotation protocol (§12's degraded-mode warnings) rides the log,
//!   one line, per the same page.
//! * [`PreviewActionWriter`] — the dry-run surface: every call is
//!   recorded in an ordered log (the `MemoryForge` call-log
//!   convention; a poisoned log is recovered, not an error) so local
//!   runs and tests observe exactly what a real workflow step would
//!   receive.
//!
//! Outside a workflow step (`GITHUB_OUTPUT` unset) [`GithubActionsWriter::from_env`]
//! is honestly `None` — the caller renders through the preview writer
//! instead; the outputs are the same, only the sink differs.
//!
//! The §12 forecast renderer ([`plan_outputs`] / [`plan_summary`] /
//! [`write_plan_file`]) feeds the `plan` mode: `released` and
//! `published` are honestly `false` (nothing was executed — the
//! forecast vocabulary is what downstream steps gate on), `releases_json`
//! carries the planned rows with the `url` left empty until a forge
//! mode fills it, `paths_released` carries the distinct edit sites
//! (I3: sorted, deduplicated), and the plan file is the canonical JSON
//! the §11.3 embed verifier accepts. The §12 job summary renders the
//! release table and the walk counters stand in for the timing
//! breakdown — wall-clock would break I3's byte-stable reports; the
//! deterministic work units are the honest timing surface.
//!
//! Fail-closed taxonomy (§15.4 `E-ACTION-*`, each variant with a page
//! under `docs/errors/`): an output name or value the env-file grammar
//! cannot represent verbatim is [`ActionError::Output`] — never
//! silently rewritten; a summary with no `GITHUB_STEP_SUMMARY` path is
//! [`ActionError::Summary`]; an unwritable plan file is
//! [`ActionError::PlanFile`].
//!
//! Layer note: `action → core` (the plan wire types) + `serde_json`
//! (the rows and the plan file) and nothing else — the env-file
//! grammar is this crate's own surface, the shell owns the wire-up.

use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use halyard_core::Plan;

/// The heredoc delimiter of the multi-line env-file form (VERIFIED
/// 2026-11-17: the docs' `{name}<<{delimiter}` multiline-string shape
/// with the caution that the delimiter "won't occur on a line of its
/// own within the value" — a fixed delimiter keeps the render
/// deterministic (I3) and a collision refuses instead of guessing).
const HEREDOC_DELIMITER: &str = "HALYARD";

/// Action-surface failures (§15.4 `E-ACTION-*`; each variant has a
/// page under `docs/errors/`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionError {
    /// An output could not be rendered or delivered (`E-ACTION-OUTPUT`):
    /// a name or value the env-file grammar cannot represent verbatim,
    /// or the `GITHUB_OUTPUT` file could not be opened or written.
    /// Fail-closed — a value is never silently rewritten, because a
    /// downstream step would gate on bytes halyard did not write.
    Output {
        /// What is wrong and what the grammar expects.
        message: String,
    },
    /// The §12 job summary could not be delivered
    /// (`E-ACTION-SUMMARY`): the writer has no `GITHUB_STEP_SUMMARY`
    /// path, or the file could not be opened or written.
    Summary {
        /// What is wrong and how to restore the sink.
        message: String,
    },
    /// The canonical plan file could not be written
    /// (`E-ACTION-PLAN`): the serialization failed or the path is not
    /// writable (a missing parent directory, a permission denial).
    PlanFile {
        /// What is wrong and what to check.
        message: String,
    },
}

impl ActionError {
    /// Stable §15.4 code of this error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Output { .. } => "E-ACTION-OUTPUT",
            Self::Summary { .. } => "E-ACTION-SUMMARY",
            Self::PlanFile { .. } => "E-ACTION-PLAN",
        }
    }
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Output { message } => write!(f, "[E-ACTION-OUTPUT] {message}"),
            Self::Summary { message } => write!(f, "[E-ACTION-SUMMARY] {message}"),
            Self::PlanFile { message } => write!(f, "[E-ACTION-PLAN] {message}"),
        }
    }
}

impl std::error::Error for ActionError {}

/// One §12 output name — a validated key of the `GITHUB_OUTPUT` file.
///
/// The docs define no output-name character set (VERIFIED
/// 2026-11-17), so the fail-closed grammar is the file format's own
/// delimiters: a name is non-empty ASCII without control characters
/// and without `=` (the key/value separator). Everything else rides —
/// per-package keys compose the package name verbatim
/// (`@scope/name--version` is a legal key; consumers access the
/// outputs object by key).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputName(String);

impl OutputName {
    /// Validates `name` against the output-key grammar.
    ///
    /// # Errors
    ///
    /// [`ActionError::Output`] when the name is empty, non-ASCII,
    /// carries a control character or the `=` separator.
    pub fn new(name: &str) -> Result<Self, ActionError> {
        let valid = !name.is_empty()
            && name.bytes().all(|b| b.is_ascii() && !b.is_ascii_control())
            && !name.contains('=');
        if valid {
            Ok(Self(name.to_owned()))
        } else {
            Err(ActionError::Output {
                message: format!(
                    "{name:?} is not a valid output name (non-empty ASCII, no control \
                     characters, no '=') — the GITHUB_OUTPUT grammar cannot carry it verbatim"
                ),
            })
        }
    }

    /// Composes a §12 per-package key: `{package}--{field}` (the
    /// `core--version` / `core--tag` shape). The composed key rides the
    /// same grammar — a package name carrying `=` (neither the cargo
    /// nor the npm grammar can) is refused.
    ///
    /// # Errors
    ///
    /// [`ActionError::Output`] when the composed key violates the
    /// output-key grammar.
    pub fn package_field(package: &str, field: &str) -> Result<Self, ActionError> {
        Self::new(&format!("{package}--{field}"))
    }

    /// The validated key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for OutputName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One §12 `releases_json` row: name, version, tag, ecosystem, url
/// (the §12 superset-of-release-please row grammar). The `url` stays
/// empty in a forecast — a forge mode fills it once the release exists.
/// Serialization is shape-pinned by [`ReleaseRow::json`] (no `serde`
/// derive: the row is a fixed five-key object, the ADR-0036 "no
/// serialization dependency" posture).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseRow {
    /// Package name as discovered (crate name or npm package name).
    pub name: String,
    /// The release version (the plan's `to`).
    pub version: String,
    /// The rendered tag the release carries.
    pub tag: String,
    /// Ecosystem identifier (`cargo`, `npm`, …; open set, §5.2).
    pub ecosystem: String,
    /// The release URL; empty in a forecast (§12 renders it for the
    /// executed modes, where the forge has minted the release).
    pub url: String,
}

impl ReleaseRow {
    /// The fixed five-key wire shape of one `releases_json` row.
    #[must_use]
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "version": self.version,
            "tag": self.tag,
            "ecosystem": self.ecosystem,
            "url": self.url,
        })
    }
}

/// The §12 output sink (the injected-effect convention): one method
/// per §12 surface. `Send + Sync` — the callers share one writer
/// across the render loop by construction.
pub trait ActionWriter: Send + Sync {
    /// Sets one output (appends the env-file entry to the sink).
    ///
    /// # Errors
    ///
    /// [`ActionError::Output`] when the name/value cannot ride the
    /// grammar verbatim or the sink cannot be written.
    fn set_output(&self, name: &OutputName, value: &str) -> Result<(), ActionError>;

    /// Appends the §12 job summary (markdown).
    ///
    /// # Errors
    ///
    /// [`ActionError::Summary`] when the sink cannot be written (or is
    /// absent).
    fn append_summary(&self, markdown: &str) -> Result<(), ActionError>;

    /// Emits one §12 degraded-mode warning (the `::warning::`
    /// annotation protocol, one log line).
    ///
    /// # Errors
    ///
    /// Implementation-defined surface failures (the preview writer
    /// records, the real writer prints — neither fails in practice;
    /// the signature keeps the seam total).
    fn emit_warning(&self, message: &str) -> Result<(), ActionError>;
}

/// The real workflow-step writer: the `GITHUB_OUTPUT` and
/// `GITHUB_STEP_SUMMARY` env files. Both are append-only from the
/// step's perspective (the runner reads them at step end).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GithubActionsWriter {
    outputs_path: PathBuf,
    summary_path: Option<PathBuf>,
}

impl GithubActionsWriter {
    /// Resolves the writer from the workflow-step environment:
    /// `GITHUB_OUTPUT` must be set and non-empty; `GITHUB_STEP_SUMMARY`
    /// is optional (its §12 summary is part of the contract, so a
    /// summary call without the path refuses `E-ACTION-SUMMARY`).
    /// `None` outside a workflow step — the caller renders through the
    /// preview writer instead (the honest local shape).
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let outputs = env_path("GITHUB_OUTPUT")?;
        let summary_path = env_path("GITHUB_STEP_SUMMARY");
        Some(Self {
            outputs_path: outputs,
            summary_path,
        })
    }

    /// A writer over explicit paths (contract tests, mirrors). No
    /// validation: the grammar failures surface on the calls.
    #[must_use]
    pub fn from_paths(outputs: PathBuf, summary: Option<PathBuf>) -> Self {
        Self {
            outputs_path: outputs,
            summary_path: summary,
        }
    }

    /// The `GITHUB_OUTPUT` sink.
    #[must_use]
    pub fn outputs_path(&self) -> &Path {
        &self.outputs_path
    }

    /// The `GITHUB_STEP_SUMMARY` sink, when the environment provides one.
    #[must_use]
    pub fn summary_path(&self) -> Option<&Path> {
        self.summary_path.as_deref()
    }

    /// Appends `bytes` to `path` (create or append; the `>>`
    /// convention). The caller owns the error code — the sink selects
    /// the taxonomy family.
    fn append(path: &Path, bytes: &str) -> Result<(), String> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| format!("{} cannot be opened for append: {e}", path.display()))?;
        file.write_all(bytes.as_bytes())
            .map_err(|e| format!("{} cannot be written: {e}", path.display()))?;
        file.flush()
            .map_err(|e| format!("{} cannot be flushed: {e}", path.display()))?;
        Ok(())
    }
}

impl ActionWriter for GithubActionsWriter {
    fn set_output(&self, name: &OutputName, value: &str) -> Result<(), ActionError> {
        let entry = env_file_entry(name, value)?;
        Self::append(&self.outputs_path, &entry).map_err(|message| ActionError::Output { message })
    }

    fn append_summary(&self, markdown: &str) -> Result<(), ActionError> {
        let Some(path) = &self.summary_path else {
            return Err(ActionError::Summary {
                message: "no GITHUB_STEP_SUMMARY path — outside a workflow step the §12 job \
                          summary has no sink; render through the preview writer instead"
                    .to_owned(),
            });
        };
        // The runner's automatic newline: a chunk without a trailing
        // newline gets one, so appends never fuse headings.
        let chunk = if markdown.ends_with('\n') {
            markdown.to_owned()
        } else {
            format!("{markdown}\n")
        };
        Self::append(path, &chunk).map_err(|message| ActionError::Summary { message })
    }

    fn emit_warning(&self, message: &str) -> Result<(), ActionError> {
        // The annotation protocol rides the log (stdout), one line —
        // the runner parses it wherever it appears in the step's
        // output. Newlines collapse: an annotation is a single line by
        // grammar.
        let line = message.replace(['\n', '\r'], " ");
        println!("::warning::{line}");
        Ok(())
    }
}

/// One recorded preview call (the observability convention: the
/// preview's log is the real writer's transcript).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionCall {
    /// A `set_output` call — the exact name and value a real step
    /// would write.
    SetOutput {
        /// The validated output name.
        name: OutputName,
        /// The value exactly as delivered.
        value: String,
    },
    /// An `append_summary` call — the markdown chunk.
    AppendSummary {
        /// The summary markdown exactly as delivered.
        markdown: String,
    },
    /// An `emit_warning` call — the message before the protocol line.
    Warning {
        /// The warning message exactly as delivered.
        message: String,
    },
}

/// The dry-run writer: every call is recorded in an ordered log,
/// nothing is written anywhere (the `PreviewForge`/`PreviewTagWriter`
/// convention — probes honest, mutations forecast).
#[derive(Debug)]
pub struct PreviewActionWriter {
    calls: Mutex<Vec<ActionCall>>,
}

impl PreviewActionWriter {
    /// An empty preview.
    #[must_use]
    pub fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
        }
    }

    /// The ordered call log (a poisoned log is recovered, not an
    /// error — the `MemoryForge` convention; the recorded calls are
    /// the observation, never the mutation).
    #[must_use]
    pub fn calls(&self) -> Vec<ActionCall> {
        self.lock().clone()
    }

    /// The log guard (the `MemoryForge` poison-recovery shape).
    fn lock(&self) -> MutexGuard<'_, Vec<ActionCall>> {
        self.calls.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Default for PreviewActionWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionWriter for PreviewActionWriter {
    fn set_output(&self, name: &OutputName, value: &str) -> Result<(), ActionError> {
        self.lock().push(ActionCall::SetOutput {
            name: name.clone(),
            value: value.to_owned(),
        });
        Ok(())
    }

    fn append_summary(&self, markdown: &str) -> Result<(), ActionError> {
        self.lock().push(ActionCall::AppendSummary {
            markdown: markdown.to_owned(),
        });
        Ok(())
    }

    fn emit_warning(&self, message: &str) -> Result<(), ActionError> {
        self.lock().push(ActionCall::Warning {
            message: message.to_owned(),
        });
        Ok(())
    }
}

/// Renders one `GITHUB_OUTPUT` entry in the documented grammar
/// (VERIFIED 2026-11-17): a value without line breaks rides the
/// `{name}={value}` line form; a multi-line value rides the documented
/// heredoc `{name}<<{delimiter}` … `{delimiter}` form. A trailing
/// newline in the value is not preserved (the heredoc's natural shape;
/// consumers split on newlines — a preserved trailing newline would
/// surface as an extra empty item). A value carrying the delimiter on
/// its own line is refused fail-closed (the docs' own caution), never
/// rewritten.
///
/// # Errors
///
/// [`ActionError::Output`] when the value cannot ride the grammar
/// verbatim.
pub fn env_file_entry(name: &OutputName, value: &str) -> Result<String, ActionError> {
    if !value.contains('\n') && !value.contains('\r') {
        return Ok(format!("{name}={value}\n"));
    }
    let trimmed = value.trim_end_matches('\n');
    if trimmed.lines().any(|line| line == HEREDOC_DELIMITER) {
        return Err(ActionError::Output {
            message: format!(
                "the value for {name} carries the {HEREDOC_DELIMITER} delimiter on its own \
                 line — the heredoc form cannot represent it verbatim, and rewriting is \
                 refused (a downstream step would gate on bytes halyard did not write)"
            ),
        });
    }
    Ok(format!(
        "{name}<<{HEREDOC_DELIMITER}\n{trimmed}\n{HEREDOC_DELIMITER}\n"
    ))
}

/// The §12 `releases_json` rows over a plan: one row per release, in
/// plan order, the row grammar verbatim (name, version, tag,
/// ecosystem, url — the url empty in a forecast).
#[must_use]
pub fn release_rows(plan: &Plan) -> Vec<ReleaseRow> {
    plan.releases
        .iter()
        .map(|release| ReleaseRow {
            name: release.package.clone(),
            version: release.to.clone(),
            tag: release.tag.clone(),
            ecosystem: release.ecosystem.clone(),
            url: String::new(),
        })
        .collect()
}

/// The §12 outputs for the `plan` mode, in the documented order: the
/// scalars (`released`, `published`, `plan_json`), `releases_json`,
/// the per-package `{package}--version` / `{package}--tag` keys in
/// plan order, and `paths_released` (the distinct edit sites, sorted,
/// newline-joined — I3). The forecast is honest: `released` and
/// `published` are `false` because the plan mode executes nothing
/// (I5) — downstream steps gate on `releases_json` / the per-package
/// keys.
///
/// # Errors
///
/// [`ActionError::Output`] when a per-package key violates the
/// output-key grammar (a hostile package name — neither the cargo nor
/// the npm grammar can produce one) or the rows fail to serialize.
pub fn plan_outputs(
    plan: &Plan,
    plan_json_path: &Path,
) -> Result<Vec<(OutputName, String)>, ActionError> {
    let mut outputs = Vec::new();
    outputs.push((OutputName::new("released")?, "false".to_owned()));
    outputs.push((OutputName::new("published")?, "false".to_owned()));
    let plan_json = plan_json_path.display().to_string();
    outputs.push((OutputName::new("plan_json")?, plan_json));
    let rows = release_rows(plan);
    let json = serde_json::to_string(&rows.iter().map(ReleaseRow::json).collect::<Vec<_>>())
        .map_err(|e| ActionError::Output {
            message: format!("the releases_json rows failed to serialize: {e}"),
        })?;
    outputs.push((OutputName::new("releases_json")?, json));
    for release in &plan.releases {
        let package = &release.package;
        let version = release.to.clone();
        let tag = release.tag.clone();
        outputs.push((OutputName::package_field(package, "version")?, version));
        outputs.push((OutputName::package_field(package, "tag")?, tag));
    }
    let mut paths: Vec<&str> = plan.edits.iter().map(|edit| edit.file.as_str()).collect();
    paths.sort_unstable();
    paths.dedup();
    let joined = paths.join("\n");
    outputs.push((OutputName::new("paths_released")?, joined));
    Ok(outputs)
}

/// The §12 job summary for the `plan` mode: the release table
/// (package, version transition, bump, tag) and the honest counts.
/// The §12 "timing breakdown" renders as the deterministic work
/// counters the caller composes from the walk provenance — wall-clock
/// would break I3's byte-stable reports; zero API calls is stated, not
/// implied (the plan mode touches no network, I5).
#[must_use]
pub fn plan_summary(plan: &Plan) -> String {
    let mut summary = String::from("## halyard plan\n\n");
    summary.push_str("| package | version | bump | tag |\n");
    summary.push_str("|---|---|---|---|\n");
    for release in &plan.releases {
        let package = &release.package;
        let from = &release.from;
        let to = &release.to;
        let tag = &release.tag;
        let bump = bump_label(release.bump);
        let _ = writeln!(summary, "| {package} | {from} → {to} | {bump} | {tag} |");
    }
    let planned = plan.releases.len();
    let skipped = plan.skipped.len();
    let _ = writeln!(summary);
    let _ = writeln!(
        summary,
        "{planned} planned, {skipped} skipped · zero API calls (offline forecast, I5)"
    );
    summary
}

/// Writes the canonical plan JSON (`halyard_core::canonical_json` over
/// the plan, trailing newline) — the file the `plan_json` output names
/// and downstream steps read verbatim.
///
/// # Errors
///
/// [`ActionError::PlanFile`] when the plan fails to serialize or the
/// path cannot be written (a missing parent directory is the caller's
/// contract, never invented).
pub fn write_plan_file(plan: &Plan, path: &Path) -> Result<(), ActionError> {
    let value = serde_json::to_value(plan).map_err(|e| ActionError::PlanFile {
        message: format!("the plan failed to serialize: {e}"),
    })?;
    let document = format!("{}\n", halyard_core::canonical_json(&value));
    fs::write(path, document).map_err(|e| ActionError::PlanFile {
        message: format!("{} cannot be written: {e}", path.display()),
    })?;
    Ok(())
}

/// The bump label the summary table renders (the wire spelling, §5.5).
fn bump_label(bump: halyard_core::Bump) -> &'static str {
    match bump {
        halyard_core::Bump::None => "none",
        halyard_core::Bump::Patch => "patch",
        halyard_core::Bump::Minor => "minor",
        halyard_core::Bump::Major => "major",
    }
}

/// An env-var path (set and non-empty → `Some`); the honest
/// `from_env` posture: an unset or empty path means "no sink here".
fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}
