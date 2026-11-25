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
//! The §12 `pr` mode renderers ([`pr_outputs`] / [`pr_summary`],
//! ADR-0043) feed the release-PR executor: the shell runs the §11.1
//! machine pass and maps its machine report onto the plain [`PrPass`]
//! view — [`PrEffect`] carries the forge's own PR
//! number and URL (never a composed guess), `release_urls` fills the
//! `releases_json` rows the pass actually minted, `released` is
//! honestly `false` for a dry forecast (I5), and the summary names the
//! pass's posture (opened / updated / unchanged / closed / the §11.1
//! PROPAGATE tag phase). The crate stays forge-free: the mapping is the
//! shell's, the vocabulary is this crate's.
//!
//! The §12 `auto` mode renderers ([`auto_outputs`] / [`auto_summary`],
//! ADR-0047) feed the composed §11.1 pass — the same plain-data shape
//! ([`AutoPass`]): the git-side tag phase's rows, the machine's own
//! half selection (`propagated`), the PR effect when the prepare phase
//! ran, and the forge half's posture. The vocabulary is the
//! pr/release superset, so a workflow step gates on the same keys
//! whichever mode ran.
//!
//! Fail-closed taxonomy (§15.4 `E-ACTION-*`, each variant with a page
//! under `docs/errors/`): an output name or value the env-file grammar
//! cannot represent verbatim is [`ActionError::Output`] — never
//! silently rewritten; a summary with no `GITHUB_STEP_SUMMARY` path is
//! [`ActionError::Summary`]; an unwritable plan file is
//! [`ActionError::PlanFile`]; a release asset, checksum line or
//! template shape the distribution grammar cannot interpret is
//! [`ActionError::Distribution`].
//!
//! Layer note: `action → core` (the plan wire types) + `serde_json`
//! (the rows and the plan file) + `sha2` (the distribution grammar's
//! checksum digest) and nothing else — the env-file grammar and the
//! distribution contract are this crate's own surface, the shell owns
//! the wire-up.

use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use halyard_core::Plan;

pub mod distribution;

/// The M5.4 signing + SBOM grammar (ADR-0054): the keyless cosign
/// signature and the CycloneDX SBOM artifact the release pipeline
/// produces and the composite action consumes, single-sourced beside
/// [`distribution`] so the templates cannot drift from the contract.
pub mod signing;

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
    /// A distribution artifact, checksum line, or template shape the
    /// §12 distribution grammar cannot interpret (`E-ACTION-DISTRIBUTION`,
    /// ADR-0052): a release asset without a checksum
    /// line, a tampered download, a manifest the checker would
    /// misread, an unpinned runner platform, or a template block that
    /// does not fit the restricted grammar. Fail-closed — a checksum
    /// mismatch or a skipped platform is never tolerated.
    Distribution {
        /// What is wrong, what the grammar expects, and the fix.
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
            Self::Distribution { .. } => "E-ACTION-DISTRIBUTION",
        }
    }
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Output { message } => write!(f, "[E-ACTION-OUTPUT] {message}"),
            Self::Summary { message } => write!(f, "[E-ACTION-SUMMARY] {message}"),
            Self::PlanFile { message } => write!(f, "[E-ACTION-PLAN] {message}"),
            Self::Distribution { message } => {
                write!(f, "[E-ACTION-DISTRIBUTION] {message}")
            }
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
    let mut outputs = vec![
        (OutputName::new("released")?, "false".to_owned()),
        (OutputName::new("published")?, "false".to_owned()),
        (
            OutputName::new("plan_json")?,
            plan_json_path.display().to_string(),
        ),
    ];
    outputs.push(releases_json(plan, |_| String::new())?);
    outputs.extend(package_keys(plan)?);
    outputs.push(paths_released(plan)?);
    Ok(outputs)
}

/// The `releases_json` value over the plan's rows (the fixed five-key
/// row grammar, one entry per release in plan order). `url_of` maps a
/// row to its release URL — the plan mode renders the empty url (the
/// forecast posture), the `pr` mode fills the URLs the pass minted
/// (ADR-0043). An empty url keeps the row's default empty field.
///
/// # Errors
///
/// [`ActionError::Output`] when the rows fail to serialize.
fn releases_json(
    plan: &Plan,
    url_of: impl Fn(&ReleaseRow) -> String,
) -> Result<(OutputName, String), ActionError> {
    let rows = release_rows(plan);
    let json = serde_json::to_string(
        &rows
            .iter()
            .map(|row| {
                let mut value = row.json();
                let url = url_of(row);
                if !url.is_empty()
                    && let Some(object) = value.as_object_mut()
                {
                    object.insert("url".to_owned(), serde_json::Value::String(url));
                }
                value
            })
            .collect::<Vec<_>>(),
    )
    .map_err(|e| ActionError::Output {
        message: format!("the releases_json rows failed to serialize: {e}"),
    })?;
    Ok((OutputName::new("releases_json")?, json))
}

/// The per-package `{package}--version` / `{package}--tag` keys, in
/// plan order (the shared tail of the §12 output vocabulary).
///
/// # Errors
///
/// [`ActionError::Output`] when a composed key violates the output-key
/// grammar (a hostile package name — neither ecosystem can produce one).
fn package_keys(plan: &Plan) -> Result<Vec<(OutputName, String)>, ActionError> {
    let mut outputs = Vec::new();
    for release in &plan.releases {
        let package = &release.package;
        let version = release.to.clone();
        let tag = release.tag.clone();
        outputs.push((OutputName::package_field(package, "version")?, version));
        outputs.push((OutputName::package_field(package, "tag")?, tag));
    }
    Ok(outputs)
}

/// The `paths_released` output: the distinct edit sites, sorted and
/// newline-joined (I3; the heredoc form carries the multi-line value).
///
/// # Errors
///
/// [`ActionError::Output`] when the key fails the grammar (it cannot —
/// the name is fixed — the signature keeps the constructor's total).
fn paths_released(plan: &Plan) -> Result<(OutputName, String), ActionError> {
    let mut paths: Vec<&str> = plan.edits.iter().map(|edit| edit.file.as_str()).collect();
    paths.sort_unstable();
    paths.dedup();
    Ok((OutputName::new("paths_released")?, paths.join("\n")))
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
    summary.push_str(&release_table(plan));
    let planned = plan.releases.len();
    let skipped = plan.skipped.len();
    let _ = writeln!(summary);
    let _ = writeln!(
        summary,
        "{planned} planned, {skipped} skipped · zero API calls (offline forecast, I5)"
    );
    summary
}

/// The release table both §12 summaries render (package, version
/// transition, bump, tag) — one layout, byte-stable (I3).
fn release_table(plan: &Plan) -> String {
    let mut table = String::from("| package | version | bump | tag |\n|---|---|---|---|\n");
    for release in &plan.releases {
        let package = &release.package;
        let from = &release.from;
        let to = &release.to;
        let tag = &release.tag;
        let bump = bump_label(release.bump);
        let _ = writeln!(table, "| {package} | {from} → {to} | {bump} | {tag} |");
    }
    table
}

/// One §12 view of the machine pass's Release-PR effect: the forge's
/// own number and URL, carried verbatim (never a composed guess — the
/// preview adapter's number `0` / `preview://` URL shape marks a
/// forecast, a real forge can produce neither).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrEffect {
    /// The Release PR was opened (first prepare pass).
    Opened {
        /// The forge's PR number.
        number: u64,
        /// The forge's own PR URL.
        url: String,
    },
    /// The Release PR's body/title differed and was rewritten in place.
    Updated {
        /// The forge's PR number.
        number: u64,
        /// The forge's own PR URL.
        url: String,
    },
    /// The Release PR already carried exactly this title and body.
    Unchanged {
        /// The forge's PR number.
        number: u64,
        /// The forge's own PR URL.
        url: String,
    },
    /// The stale Release PR was closed under an empty plan.
    Closed {
        /// The forge's PR number.
        number: u64,
        /// The forge's own PR URL of the closed PR.
        url: String,
    },
}

/// The number-and-URL view of a [`PrEffect`] (the `pr_number` /
/// `pr_url` output pair).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrView {
    /// The forge's PR number.
    pub number: u64,
    /// The forge's own PR URL.
    pub url: String,
}

impl PrEffect {
    /// The number-and-URL view of this effect.
    #[must_use]
    pub fn view(&self) -> PrView {
        let (number, url) = match self {
            Self::Opened { number, url }
            | Self::Updated { number, url }
            | Self::Unchanged { number, url }
            | Self::Closed { number, url } => (*number, url.clone()),
        };
        PrView { number, url }
    }
}

/// The §12 view of one §11.1 machine pass: plain, deterministic data
/// (I3 — comparable across runs). The shell runs the pass and maps the
/// forge's machine report onto it; this crate renders the vocabulary
/// and stays forge-free (the layer note).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrPass {
    /// The pass's Release-PR effect (`None` on the §11.1 PROPAGATE
    /// pass, where the tag phase owns the run, or on the one-probe
    /// no-op — empty plan, no stale PR).
    pub pr_effect: Option<PrEffect>,
    /// `true` when the tag phase owned the pass (§11.1 step 2:
    /// pending forge releases gate the prepare phase — the Release PR
    /// lands on the next pass).
    pub propagated: bool,
    /// `true` when the pass executed a forge release creation (the
    /// live tag phase). A dry forecast stays `false` — nothing was
    /// executed (I5), even though its minted URLs ride
    /// [`PrPass::release_urls`] in the forecast's `preview://` shape.
    pub released: bool,
    /// The release URLs minted this pass, keyed by tag (the created
    /// releases; pre-existing ones keep the empty `url` in
    /// `releases_json`).
    pub release_urls: std::collections::BTreeMap<String, String>,
    /// The release branch the pass operated on (§11.2
    /// `halyard/release/<base>`).
    pub branch: String,
    /// `true` for a dry forecast — the summary states the posture.
    pub forecast: bool,
}

/// The §12 outputs for the `pr` mode (ADR-0043), in the documented
/// order: `released` (the pass's honest execution value), `published`
/// (always `false` — the pr mode never publishes), the
/// `pr_number` / `pr_url` pair when the pass has a Release PR to
/// report (absent on the PROPAGATE and no-op passes), `plan_json`,
/// `releases_json` (the pass's minted URLs filled by tag), the
/// per-package keys and `paths_released` — the plan vocabulary
/// unchanged.
///
/// # Errors
///
/// [`ActionError::Output`] when a key violates the output-key grammar
/// or the rows fail to serialize.
pub fn pr_outputs(
    plan: &Plan,
    plan_json_path: &Path,
    pass: &PrPass,
) -> Result<Vec<(OutputName, String)>, ActionError> {
    let mut outputs = Vec::new();
    let released = if pass.released { "true" } else { "false" };
    outputs.push((OutputName::new("released")?, released.to_owned()));
    outputs.push((OutputName::new("published")?, "false".to_owned()));
    if let Some(view) = pass.pr_effect.as_ref().map(PrEffect::view) {
        outputs.push((OutputName::new("pr_number")?, view.number.to_string()));
        outputs.push((OutputName::new("pr_url")?, view.url));
    }
    outputs.push((
        OutputName::new("plan_json")?,
        plan_json_path.display().to_string(),
    ));
    let release_urls = &pass.release_urls;
    outputs.push(releases_json(plan, |row| {
        release_urls.get(&row.tag).cloned().unwrap_or_default()
    })?);
    outputs.extend(package_keys(plan)?);
    outputs.push(paths_released(plan)?);
    Ok(outputs)
}

/// The §12 job summary for the `pr` mode: the release table (the plan
/// vocabulary's layout) and the pass line — what the machine pass did
/// and its posture (forecast vs executed), byte-stable (I3).
#[must_use]
pub fn pr_summary(plan: &Plan, pass: &PrPass) -> String {
    let mut summary = String::from("## halyard release PR\n\n");
    summary.push_str(&release_table(plan));
    let planned = plan.releases.len();
    let skipped = plan.skipped.len();
    let _ = writeln!(summary);
    let _ = writeln!(
        summary,
        "{planned} planned, {skipped} skipped · {}",
        pr_pass_line(pass)
    );
    summary
}

/// The pass line the `pr` summary renders: the Release-PR effect (or
/// the PROPAGATE / no-op posture), the pass's minted releases (named,
/// so a PROPAGATE forecast's `preview://` mints are observable beside
/// the plan's next-release rows) and the forecast marker.
fn pr_pass_line(pass: &PrPass) -> String {
    let mut line = if pass.propagated {
        "tag phase owns this pass (the Release PR lands on the next pass)".to_owned()
    } else {
        match &pass.pr_effect {
            Some(PrEffect::Opened { number, url }) => format!("PR #{number} opened ({url})"),
            Some(PrEffect::Updated { number, url }) => format!("PR #{number} updated ({url})"),
            Some(PrEffect::Unchanged { number, url }) => {
                format!("PR #{number} already current ({url})")
            }
            Some(PrEffect::Closed { number, url }) => format!("PR #{number} closed ({url})"),
            None => "no Release PR (nothing to open, none stale)".to_owned(),
        }
    };
    for (tag, url) in &pass.release_urls {
        let _ = write!(line, " · minted {tag} ({url})");
    }
    if pass.forecast {
        line.push_str(" · forecast (nothing executed)");
    }
    line
}

/// The §12 view of one release pass (ADR-0046): plain, deterministic
/// data (I3) — the git-side tags the §11.1 tag phase created, the forge
/// half's posture and the forecast marker. The shell runs the pass and
/// maps its reports onto it; this crate renders the vocabulary and
/// stays git-free and forge-free (the layer note).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleasePass {
    /// `true` when the live tag phase created at least one tag — the
    /// release act. A dry forecast stays `false`: nothing was executed
    /// (I5), even though its would-create tags ride
    /// [`ReleasePass::created_tags`].
    pub released: bool,
    /// `true` for a dry forecast — the summary states the posture.
    pub forecast: bool,
    /// The tags the pass created (would create, in the forecast), in
    /// the tag phase's own name-sorted row order.
    pub created_tags: Vec<String>,
    /// The release URLs minted by the forge half this pass, keyed by
    /// tag (the created releases; rows the forge half did not touch
    /// keep the empty `url` in `releases_json`).
    pub release_urls: std::collections::BTreeMap<String, String>,
    /// The forge half's posture (§11.1 step 2, M4.1b): what ran, or the
    /// honest skip's reason — never a pretended release.
    pub forge: ForgePosture,
}

/// The forge half's posture for one release pass (plain data — the
/// summary renders the reason verbatim, so a skip is observable with
/// its cause).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForgePosture {
    /// The forge half executed over the adapter: the repo slug and how
    /// many releases it minted this pass.
    Ran {
        /// The `owner/repo` slug the half operated on.
        repo: String,
        /// The releases the half minted this pass.
        releases: usize,
    },
    /// The forge half did not run — the reason is stated, never implied
    /// (the dry forecast's I5 posture, missing credentials, a foreign
    /// origin).
    Skipped {
        /// The honest skip's reason.
        reason: String,
    },
}

/// The §12 outputs for the `release` mode (ADR-0046), in the documented
/// order: `released` (the pass's honest execution value), `published`
/// (always `false` — the release mode never publishes), `plan_json`,
/// `releases_json` (the pass's minted URLs filled by tag), the
/// per-package keys and `paths_released` — the plan vocabulary
/// unchanged. No `pr_number`/`pr_url` keys exist: the release mode
/// opens no Release PR.
///
/// # Errors
///
/// [`ActionError::Output`] when a key violates the output-key grammar
/// or the rows fail to serialize.
pub fn release_outputs(
    plan: &Plan,
    plan_json_path: &Path,
    pass: &ReleasePass,
) -> Result<Vec<(OutputName, String)>, ActionError> {
    let mut outputs = Vec::new();
    let released = if pass.released { "true" } else { "false" };
    outputs.push((OutputName::new("released")?, released.to_owned()));
    outputs.push((OutputName::new("published")?, "false".to_owned()));
    outputs.push((
        OutputName::new("plan_json")?,
        plan_json_path.display().to_string(),
    ));
    let release_urls = &pass.release_urls;
    outputs.push(releases_json(plan, |row| {
        release_urls.get(&row.tag).cloned().unwrap_or_default()
    })?);
    outputs.extend(package_keys(plan)?);
    outputs.push(paths_released(plan)?);
    Ok(outputs)
}

/// The §12 job summary for the `release` mode: the release table (the
/// plan vocabulary's layout) and the pass line — the created tags, the
/// forge half's posture and the forecast marker, byte-stable (I3).
#[must_use]
pub fn release_summary(plan: &Plan, pass: &ReleasePass) -> String {
    let mut summary = String::from("## halyard release\n\n");
    summary.push_str(&release_table(plan));
    let planned = plan.releases.len();
    let skipped = plan.skipped.len();
    let _ = writeln!(summary);
    let _ = writeln!(
        summary,
        "{planned} planned, {skipped} skipped · {}",
        release_pass_line(pass)
    );
    summary
}

/// The pass line the `release` summary renders: the created tags (the
/// git-side half's rows), the forge half's posture (the skip's reason
/// verbatim, so a forecast or a credential gap is observable) and the
/// forecast marker.
fn release_pass_line(pass: &ReleasePass) -> String {
    let mut line = if pass.created_tags.is_empty() {
        "no tags created (nothing pending, already tagged or failed rows — the report names them)"
            .to_owned()
    } else {
        format!("created {}", pass.created_tags.join(", "))
    };
    line.push_str(" · ");
    match &pass.forge {
        ForgePosture::Ran { repo, releases } => {
            let _ = write!(line, "forge releases in {repo} ({releases} minted)");
        }
        ForgePosture::Skipped { reason } => {
            let _ = write!(line, "forge releases skipped ({reason})");
        }
    }
    if pass.forecast {
        line.push_str(" · forecast (nothing executed)");
    }
    line
}

/// The §11.1 step-3 content effect a pass's prepare phase carried (the
/// machine's own verdict over its `content_commit` input, ADR-0047):
/// plain data on the pass view — the machine compared the plan's
/// content tree against the remote release branch's tip and either
/// found the branch already carrying it or committed it through the
/// forge. The git-side CAS push (the shell's `commit_mode = "git"`
/// half, §11.2) reports as a match too: the branch carries the tree,
/// which is all the machine's compare keys on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContentEffect {
    /// The release branch already carried the plan's content tree (the
    /// I1 match — the git-side half pushed it under the §11.2 lease, or
    /// the branch was already current): the machine spent no content
    /// call.
    Matched {
        /// The content tree OID both sides agreed on.
        tree_oid: String,
    },
    /// The content was committed through the forge's `commit_via_api`
    /// (the live `api` mode's real oid, or the dry forecast's preview
    /// oid — I5, the preview marks it like every forecast value).
    Committed {
        /// The forge-reported commit oid.
        oid: String,
    },
}

/// The §12 view of one `auto` pass (ADR-0047): plain, deterministic
/// data (I3) — the git-side tags the §11.1 tag phase created, which
/// half owned the run (the §11.1 machine's own PROPAGATE rule, never a
/// caller guess), the Release-PR effect when the prepare phase ran,
/// and the forge half's posture. The shell composes the two executors
/// and maps their reports onto this view; this crate renders the
/// vocabulary and stays git-free and forge-free (the layer note).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoPass {
    /// `true` when the live tag phase created at least one tag — the
    /// release act (§11.1 step 2). A dry forecast stays `false`:
    /// nothing was executed (I5), even though its would-create tags
    /// ride [`AutoPass::created_tags`].
    pub released: bool,
    /// `true` when the §11.1 machine's own PROPAGATE rule owned the
    /// pass: pending releases gated the prepare phase, so the Release
    /// PR lands on the next pass (§11.1 step 2).
    pub propagated: bool,
    /// `true` for a dry forecast — the summary states the posture.
    pub forecast: bool,
    /// The tags the pass created (would create, in the forecast), in
    /// the tag phase's own name-sorted row order.
    pub created_tags: Vec<String>,
    /// The pass's Release-PR effect (`None` on the §11.1 PROPAGATE
    /// pass, where the tag phase owns the run, or on the one-probe
    /// no-op — empty plan, no stale PR).
    pub pr_effect: Option<PrEffect>,
    /// The prepare phase's content verdict (§11.1 step 3) — `None`
    /// whenever the prepare phase did not run (the PROPAGATE pass, the
    /// empty plan's close probe) or the plan staged no content: the
    /// machine's `content_commit` input was absent, so no content
    /// effect exists to report.
    pub content_effect: Option<ContentEffect>,
    /// The release URLs minted this pass, keyed by tag (the created
    /// releases; rows the machine did not touch keep the empty `url`
    /// in `releases_json`).
    pub release_urls: std::collections::BTreeMap<String, String>,
    /// The forge half's posture (the machine's tag phase, §11.1 step
    /// 2): what ran, or the honest skip's reason — never a pretended
    /// release.
    pub forge: ForgePosture,
}

/// The §12 outputs for the `auto` mode (ADR-0047), in the documented
/// order: `released` (the pass's honest execution value — the git-side
/// tag phase's), `published` (always `false` — the auto mode never
/// publishes), the `pr_number` / `pr_url` pair when the pass has a
/// Release PR to report (absent on the PROPAGATE and no-op passes —
/// the §11.1 machine's own half selection), `plan_json`,
/// `releases_json` (the pass's minted URLs filled by tag), the
/// per-package keys and `paths_released` — the plan vocabulary
/// unchanged, the same superset the `pr` and `release` modes speak.
///
/// # Errors
///
/// [`ActionError::Output`] when a key violates the output-key grammar
/// or the rows fail to serialize.
pub fn auto_outputs(
    plan: &Plan,
    plan_json_path: &Path,
    pass: &AutoPass,
) -> Result<Vec<(OutputName, String)>, ActionError> {
    let mut outputs = Vec::new();
    let released = if pass.released { "true" } else { "false" };
    outputs.push((OutputName::new("released")?, released.to_owned()));
    outputs.push((OutputName::new("published")?, "false".to_owned()));
    if let Some(view) = pass.pr_effect.as_ref().map(PrEffect::view) {
        outputs.push((OutputName::new("pr_number")?, view.number.to_string()));
        outputs.push((OutputName::new("pr_url")?, view.url));
    }
    outputs.push((
        OutputName::new("plan_json")?,
        plan_json_path.display().to_string(),
    ));
    let release_urls = &pass.release_urls;
    outputs.push(releases_json(plan, |row| {
        release_urls.get(&row.tag).cloned().unwrap_or_default()
    })?);
    outputs.extend(package_keys(plan)?);
    outputs.push(paths_released(plan)?);
    Ok(outputs)
}

/// The §12 job summary for the `auto` mode: the release table (the
/// plan vocabulary's layout) and the pass line — the created tags, the
/// machine's own half selection, the minted URLs, the forge posture
/// and the forecast marker, byte-stable (I3).
#[must_use]
pub fn auto_summary(plan: &Plan, pass: &AutoPass) -> String {
    let mut summary = String::from("## halyard auto\n\n");
    summary.push_str(&release_table(plan));
    let planned = plan.releases.len();
    let skipped = plan.skipped.len();
    let _ = writeln!(summary);
    let _ = writeln!(
        summary,
        "{planned} planned, {skipped} skipped · {}",
        auto_pass_line(pass)
    );
    summary
}

/// The pass line the `auto` summary renders: the release half's rows
/// (the created tags), the machine's own half selection (the §11.1
/// PROPAGATE rule or the prepare phase's content verdict then PR
/// effect, in the machine's own §11.1 order), the minted URLs, the
/// forge posture and the forecast marker.
fn auto_pass_line(pass: &AutoPass) -> String {
    let mut line = if pass.created_tags.is_empty() {
        "no tags created (nothing pending, already tagged or failed rows — the report names them)"
            .to_owned()
    } else {
        format!("created {}", pass.created_tags.join(", "))
    };
    line.push_str(" · ");
    if pass.propagated {
        line.push_str("tag phase owns this pass (the Release PR lands on the next pass)");
    } else {
        // The prepare phase's §11.1 order: the content step commits (or
        // matches) before the PR half rides the branch it just carried.
        match &pass.content_effect {
            Some(ContentEffect::Matched { tree_oid }) => {
                let _ = write!(
                    line,
                    "the release branch already carries the content tree {tree_oid} · "
                );
            }
            Some(ContentEffect::Committed { oid }) => {
                let _ = write!(line, "content tree committed ({oid}) · ");
            }
            None => {}
        }
        match &pass.pr_effect {
            Some(PrEffect::Opened { number, url }) => {
                let _ = write!(line, "PR #{number} opened ({url})");
            }
            Some(PrEffect::Updated { number, url }) => {
                let _ = write!(line, "PR #{number} updated ({url})");
            }
            Some(PrEffect::Unchanged { number, url }) => {
                let _ = write!(line, "PR #{number} already current ({url})");
            }
            Some(PrEffect::Closed { number, url }) => {
                let _ = write!(line, "PR #{number} closed ({url})");
            }
            None => line.push_str("no Release PR (nothing to open, none stale)"),
        }
    }
    for (tag, url) in &pass.release_urls {
        let _ = write!(line, " · minted {tag} ({url})");
    }
    line.push_str(" · ");
    match &pass.forge {
        ForgePosture::Ran { repo, releases } => {
            let _ = write!(line, "forge releases in {repo} ({releases} minted)");
        }
        ForgePosture::Skipped { reason } => {
            let _ = write!(line, "forge releases skipped ({reason})");
        }
    }
    if pass.forecast {
        line.push_str(" · forecast (nothing executed)");
    }
    line
}

/// One package's conveyed-bump row of the §12 `check` verdict (the
/// plain view of the check surface's prediction — the shell maps its
/// own prediction type onto it; this crate stays check-logic-free).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckRow {
    /// The package name (plan order).
    pub package: String,
    /// The version in the manifest (the prediction's `from`).
    pub from: String,
    /// The bump the title conveys for this package (`major|minor|patch`
    /// — after the shell's pre-major remap where the package is
    /// `0.y.z`, so the verdict teaches its own cause).
    pub bump: String,
}

/// The §12 view of one linted title (the shell's classification,
/// mirrored as plain data).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TitleOutcome {
    /// The title is a conventional commit and conveys a bump.
    Accepted {
        /// The commit kind exactly as written (`feat`, `FEAT`, …).
        kind: String,
        /// The conveyed level (`major|minor|patch|none`).
        level: String,
        /// One row per pending release, plan order.
        predictions: Vec<CheckRow>,
    },
    /// The parser declined the title — the reason states the expected
    /// shape (§11.6: errors teach). The shell refuses fail-closed
    /// before the mode writes anything; the outcome exists so the
    /// summary can render the diagnosis when a caller chooses to.
    Rejected {
        /// Why the title was declined.
        message: String,
    },
}

/// The §12 `check` verdict: the linted title's outcome, when the
/// caller passed one (`None` — the pending forecast is the verdict).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckVerdict {
    /// The title outcome (`None` for the no-title posture).
    pub title: Option<TitleOutcome>,
}

/// The §12 job summary for the `check` mode (ADR-0044): the pending
/// forecast's release table (the plan vocabulary's layout) and the
/// verdict line — the lint's kind → level and the conveyed bumps per
/// pending package, the rejection's diagnosis, or the no-title
/// posture. Byte-stable (I3); the outputs themselves ride the plan
/// vocabulary unchanged ([`plan_outputs`]).
#[must_use]
pub fn check_summary(plan: &Plan, verdict: &CheckVerdict) -> String {
    let mut summary = String::from("## halyard check\n\n");
    summary.push_str(&release_table(plan));
    let planned = plan.releases.len();
    let skipped = plan.skipped.len();
    let _ = writeln!(summary);
    let _ = writeln!(
        summary,
        "{planned} planned, {skipped} skipped · {}",
        check_verdict_line(verdict)
    );
    summary
}

/// The verdict line the `check` summary renders.
fn check_verdict_line(verdict: &CheckVerdict) -> String {
    match &verdict.title {
        Some(TitleOutcome::Accepted {
            kind,
            level,
            predictions,
        }) => {
            let mut line = format!("title accepted: {kind} → {level}");
            if predictions.is_empty() {
                let _ = write!(line, " · conveyed nothing pending");
            } else {
                let _ = write!(line, " · conveyed ");
                for (index, row) in predictions.iter().enumerate() {
                    if index > 0 {
                        let _ = write!(line, ", ");
                    }
                    let _ = write!(line, "{} {}", row.package, row.bump);
                }
            }
            line
        }
        Some(TitleOutcome::Rejected { message }) => format!("title rejected: {message}"),
        None => "no title to lint — the pending forecast is the verdict".to_owned(),
    }
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
