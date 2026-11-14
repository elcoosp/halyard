//! The smart minimal fetch (§8.8): the fetch planner (pure) and the
//! fetch effect over the [`TagFetcher`] seam.
//!
//! §8.8 exists because CI checkouts are the *fast* path only when the
//! history halyard needs is actually present: the release tags and the
//! base branch. The subsystem is the third sanctioned subprocess effect
//! in this crate (after the tag write [`crate::tag`] and the tag push
//! [`crate::push`]), and it is the one §I5 names explicitly: network is
//! allowed only by *invocation* of `fetch`/`pr`/`release`/`publish`.
//!
//! The §8.8 shape, as implemented:
//!
//! 1. **Probe** — `git ls-remote --tags <remote>` (one cheap round trip;
//!    the wire format is parsed by [`crate::push::parse_remote_tag_targets`],
//!    pinned in the push suite);
//! 2. **Select** — per package the highest-version tag matching its tag
//!    template ([`latest_tags_for`], the §8.2 step-2 matcher over the
//!    remote listing, without the ancestry test — the remote's commits
//!    are not reachable objects until they are fetched);
//! 3. **Classify** — a selected tag already present locally at the
//!    probed target is skipped (I1 idempotence); a local tag at a
//!    *different* target is fail-closed `E-GIT-TAG-CONFLICT` (release
//!    tags are never moved — the create-only refspec would be rejected
//!    by git anyway);
//! 4. **Fetch once** — `git fetch --filter=blob:none --no-tags
//!    [--shallow-exclude=<base>…] <remote> <base-branch>
//!    refs/tags/<tag>…` ([`plan_fetch`]; explicit refspecs request
//!    exactly the selected tags — nothing rides along, I7). The
//!    shallow-exclude machinery rides **only on shallow repositories**:
//!    git fails the whole fetch with `no commits selected for shallow
//!    requests` when there is nothing to deepen (probed empirically and
//!    pinned by the tests, ADR-0016), and on a complete clone there is
//!    nothing to deepen. On a shallow clone the excludes are each
//!    package's **previous** release tag ([`deepen_boundaries_for`]) —
//!    the §8.8 "selected base tags" deepen boundary: the fetched tags
//!    are strictly newer, so the boundary never swallows the request;
//! 5. **Fallback** — a failing minimal fetch is retried once as the
//!    documented full fetch of the same refs ([§8.8 step 3]: no filter,
//!    no `--shallow-exclude`; `--deepen` loops stay deferred — they are
//!    an interactive server negotiation the full fetch supersedes as the
//!    terminal fallback, ADR-0016).
//!
//! There is no dry-run adapter for this seam, by design: every other
//! effect seam forecasts because its *mutations* are known offline (the
//! created tags, the release PR). The fetch effect's whole input is the
//! remote listing itself — an offline forecast could only repeat the
//! locally-known state back (`nothing to fetch`), so the honest offline
//! surface is the pure planner ([`latest_tags_for`] / [`plan_fetch`] /
//! [`fetch_attempts`]), which the docs, the example and the bench pin
//! without touching a network (I5). ADR-0016 records the decision.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use halyard_core::package::Workspace;
use halyard_core::planner::{PlanConfig, effective_template};

use crate::GitError;
use crate::push::parse_remote_tag_targets;

/// Which §8.8 fetch command an attempt runs: the minimal
/// blob-less/tag-bounded fetch ([`plan_fetch`]) or the documented full
/// fetch of the same refs (the terminal fallback, §8.8 step 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FetchCommand {
    /// The primary minimal fetch: `--filter=blob:none --no-tags` plus one
    /// `--shallow-exclude` per requested tag.
    Minimal,
    /// The full fetch of the same refs (no filter, no shallow-exclude) —
    /// the §8.8 step-3 fallback when the minimal fetch fails.
    Full,
}

/// One planned `git fetch` invocation: which kind and the exact argv
/// (after `git`). Deterministic: the same inputs compose the same argv.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchAttempt {
    /// Which §8.8 step this attempt implements.
    pub command: FetchCommand,
    /// The exact argv after `git` (options first, then remote, then
    /// refspecs).
    pub argv: Vec<String>,
}

/// The fetch plan (§8.8 step 2): the selected tags, the refspecs they
/// compose into, and the exact argv of the primary minimal fetch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchPlan {
    /// The remote the plan targets.
    pub remote: String,
    /// The base branch requested alongside the tags (`None` = tags only).
    pub base_branch: Option<String>,
    /// The requested tags, sorted (I3: input order cannot leak).
    pub tags: Vec<String>,
    /// The explicit refspecs: the base branch first (plain ref — a
    /// `FETCH_HEAD` fetch, never a local-branch move), then one
    /// create-only tag refspec per tag.
    pub refspecs: Vec<String>,
    /// One `--shallow-exclude` value per requested tag (§8.8 step 2).
    pub shallow_exclude: Vec<String>,
    /// The exact argv after `git` of the primary minimal fetch.
    pub argv: Vec<String>,
}

/// One fetch outcome row (per *selected* tag, name-sorted).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FetchRow {
    /// The remote accepted the fetch and the tag is now local.
    Fetched {
        /// The short tag name.
        tag: String,
    },
    /// The tag was already local at the probed target — nothing was
    /// requested (I1).
    AlreadyLocal {
        /// The short tag name.
        tag: String,
    },
    /// The tag could not be fetched (a divergent local release tag, a
    /// failed fetch). The phase continues past it (I2); the shell
    /// renders the error with its stable code.
    Failed {
        /// The short tag name.
        tag: String,
        /// The failure with its `E-GIT-*` code.
        error: GitError,
    },
}

/// The fetch report (§8.8): the probe size, the per-package selection
/// provenance, the executed argv and the per-tag outcome rows with
/// counters. Deterministic regardless of probe order (I3).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FetchReport {
    /// The remote the phase targeted. `None` when no
    /// `remote.<name>.url` is configured — an honest no-op phase (the
    /// repository has nothing to fetch from; the shell renders the skip).
    pub remote: Option<String>,
    /// The base branch requested alongside the tags.
    pub base_branch: Option<String>,
    /// Per-package selected tag (the §8.8 step-1 provenance: *why* this
    /// tag was requested), keyed by package name.
    pub selection: BTreeMap<String, String>,
    /// Per-tag rows, name-sorted (I3).
    pub rows: Vec<FetchRow>,
    /// The argv of the fetch that ran (empty when nothing was requested
    /// — no tags selected and no base branch).
    pub argv: Vec<String>,
    /// How many tags the probe listed (remote-side state size).
    pub probe_tags: usize,
    /// Tags fetched from the remote.
    pub fetched: usize,
    /// Tags already local at the probed target (nothing requested).
    pub already_local: usize,
    /// Tags that failed (the phase continues past them, I2).
    pub failed: usize,
    /// True when the primary minimal fetch failed and the §8.8 step-3
    /// full-fetch fallback succeeded (the shell surfaces the degraded
    /// mode).
    pub fallback_used: bool,
}

impl FetchReport {
    /// Tallies the rows into the outcome counters (the single source of
    /// the counters — callers cannot drift from the rows).
    fn tally(
        remote: Option<String>,
        base_branch: Option<String>,
        selection: BTreeMap<String, String>,
        rows: Vec<FetchRow>,
    ) -> Self {
        let mut report = Self {
            remote,
            base_branch,
            selection,
            rows,
            ..Self::default()
        };
        for row in &report.rows {
            match row {
                FetchRow::Fetched { .. } => report.fetched += 1,
                FetchRow::AlreadyLocal { .. } => report.already_local += 1,
                FetchRow::Failed { .. } => report.failed += 1,
            }
        }
        report
    }
}

/// The §8.8 step-1 selection (pure): per package the **highest-version**
/// tag matching its tag template wins (the §8.2 step-2 matcher over a
/// tag→target map — the remote listing for the live run, the local refs
/// for offline planning). The template is the planner's own
/// [`effective_template`] rule (config override → single-package
/// `v{version}` → discovery template), so the fetch phase, the plan and
/// the §11.1 tag phase can never disagree about a tag's name (I3).
/// Foreign templates and invalid versions are ignored (§8.2 step 3).
/// Equal versions tie-break by tag name so the outcome never depends on
/// map order (I3). The result is keyed by package name (sorted).
#[must_use]
pub fn latest_tags_for(
    ws: &Workspace,
    cfg: &PlanConfig,
    tag_targets: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut selection = BTreeMap::new();
    for pkg in ws.packages() {
        let Some(name) = ws.interner().get(pkg.name) else {
            continue;
        };
        let eco = pkg.ecosystem.resolve(ws.interner());
        let template = effective_template(ws, pkg, cfg);
        let mut best: Option<(semver::Version, &String)> = None;
        for tag in tag_targets.keys() {
            if let Some(version) = template.extract_version(tag, name, eco) {
                let better = match &best {
                    Some((current, current_tag)) => {
                        version > *current || (version == *current && tag < *current_tag)
                    }
                    None => true,
                };
                if better {
                    best = Some((version, tag));
                }
            }
        }
        if let Some((_, tag)) = best {
            selection.insert(name.to_owned(), tag.clone());
        }
    }
    selection
}

/// Composes the §8.8 step-2 minimal fetch (pure): `--filter=blob:none
/// --no-tags`, the base branch first (a plain ref — a `FETCH_HEAD`
/// fetch that never moves a local branch), then explicit create-only
/// tag refspecs. `shallow` gates the §8.8 shallow machinery: on a shallow
/// repository one `--shallow-exclude=<boundary>` per deepen boundary is
/// composed; on a complete repository the option is omitted (git fails
/// the fetch outright when there is nothing to deepen — pinned by the
/// tests). Deterministic: the tags and the boundaries are sorted, so
/// input order cannot leak (I3).
#[must_use]
pub fn plan_fetch(
    remote: &str,
    base_branch: Option<&str>,
    selection: &BTreeMap<String, String>,
    boundaries: &[String],
    shallow: bool,
) -> FetchPlan {
    let mut tags: Vec<String> = selection.values().cloned().collect();
    tags.sort();
    tags.dedup();
    let mut excludes: Vec<String> = if shallow {
        boundaries.to_vec()
    } else {
        Vec::new()
    };
    excludes.sort();
    excludes.dedup();
    let mut refspecs = Vec::with_capacity(tags.len() + 1);
    if let Some(base) = base_branch {
        refspecs.push(base.to_owned());
    }
    for tag in &tags {
        refspecs.push(format!("refs/tags/{tag}:refs/tags/{tag}"));
    }
    let mut argv = vec![
        "fetch".to_owned(),
        "--filter=blob:none".to_owned(),
        "--no-tags".to_owned(),
    ];
    for boundary in &excludes {
        argv.push(format!("--shallow-exclude={boundary}"));
    }
    argv.push(remote.to_owned());
    argv.extend(refspecs.iter().cloned());
    FetchPlan {
        remote: remote.to_owned(),
        base_branch: base_branch.map(str::to_owned),
        tags: tags.clone(),
        refspecs,
        shallow_exclude: excludes,
        argv,
    }
}

/// The §8.8 deepen boundaries (pure): per package the **previous**
/// release tag — the highest version strictly below the selected latest
/// ([`latest_tags_for`]). These are the `--shallow-exclude` values of
/// the shallow fetch: the fetched tags are strictly newer than their
/// package's boundary, so the boundary bounds the deepening without
/// swallowing the request. Packages with a single matching tag
/// contribute nothing. Sorted and deduplicated.
#[must_use]
pub fn deepen_boundaries_for(
    ws: &Workspace,
    cfg: &PlanConfig,
    tag_targets: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut boundaries: Vec<String> = Vec::new();
    for pkg in ws.packages() {
        let Some(name) = ws.interner().get(pkg.name) else {
            continue;
        };
        let eco = pkg.ecosystem.resolve(ws.interner());
        let template = effective_template(ws, pkg, cfg);
        let mut candidates: Vec<(semver::Version, &String)> = Vec::new();
        for tag in tag_targets.keys() {
            if let Some(version) = template.extract_version(tag, name, eco) {
                candidates.push((version, tag));
            }
        }
        // Highest version first; equal versions tie-break by tag name
        // (the §8.2 candidate order, so both functions agree on which
        // tag is "latest" and which is the boundary).
        candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
        if let Some((_, boundary)) = candidates.get(1) {
            boundaries.push((*boundary).clone());
        }
    }
    boundaries.sort();
    boundaries.dedup();
    boundaries
}

/// The §8.8 fetch attempt sequence (pure): the primary minimal fetch
/// first, then the documented full fetch of the same refs as the
/// terminal fallback (§8.8 step 3 — no `--filter`, no
/// `--shallow-exclude`; `--deepen` loops stay deferred, ADR-0016). Both
/// attempts request exactly the same refspecs, so a fallback can never
/// fetch something the plan did not name. The shallow machinery rides
/// only on shallow repositories (`shallow = true`) and only when
/// `boundaries` is non-empty.
#[must_use]
pub fn fetch_attempts(
    remote: &str,
    base_branch: Option<&str>,
    tags: &[String],
    boundaries: &[String],
    shallow: bool,
) -> Vec<FetchAttempt> {
    let mut requested = tags.to_vec();
    requested.sort();
    requested.dedup();
    let mut excludes: Vec<String> = if shallow {
        boundaries.to_vec()
    } else {
        Vec::new()
    };
    excludes.sort();
    excludes.dedup();
    let mut refspecs = Vec::with_capacity(requested.len() + 1);
    if let Some(base) = base_branch {
        refspecs.push(base.to_owned());
    }
    for tag in &requested {
        refspecs.push(format!("refs/tags/{tag}:refs/tags/{tag}"));
    }
    let compose = |command: FetchCommand| -> FetchAttempt {
        let mut argv = Vec::with_capacity(requested.len() + excludes.len() + 4);
        argv.push("fetch".to_owned());
        if command == FetchCommand::Minimal {
            argv.push("--filter=blob:none".to_owned());
        }
        argv.push("--no-tags".to_owned());
        if command == FetchCommand::Minimal {
            for boundary in &excludes {
                argv.push(format!("--shallow-exclude={boundary}"));
            }
        }
        argv.push(remote.to_owned());
        argv.extend(refspecs.iter().cloned());
        FetchAttempt { command, argv }
    };
    vec![compose(FetchCommand::Minimal), compose(FetchCommand::Full)]
}

/// The fetch effect (§8.8): probe → select → classify → fetch once,
/// with the documented full-fetch fallback. Object-safe (`&dyn`) so the
/// shell and tests can bind the live engine; there is no preview
/// adapter by design (the module doc explains why — the remote listing
/// is the input an offline forecast cannot have).
pub trait TagFetcher {
    /// Fetches the per-package latest release tags plus the base branch
    /// from `remote` (§8.8). The templates resolve through the planner's
    /// [`effective_template`] rule (`cfg`), so the fetch phase and the
    /// plan can never disagree about a tag's name. A remote without a
    /// configured URL yields an honest no-op report
    /// ([`FetchReport::remote`] `None`).
    ///
    /// # Errors
    ///
    /// [`GitError::Fetch`] when the probe or both fetch attempts cannot
    /// run (no `git` binary, unreachable remote, auth refused — never a
    /// prompt). Per-tag divergences surface as report rows, not this
    /// error.
    fn fetch_release_tags(
        &self,
        ws: &Workspace,
        cfg: &PlanConfig,
        remote: &str,
        base_branch: Option<&str>,
    ) -> Result<FetchReport, GitError>;
}

/// Runs a `git` effect subprocess (argv arrays only, I7) rooted at `cwd`
/// (the opened repository — the process cwd is *not* inherited, so the
/// effect cannot touch an enclosing repository by accident) with
/// `GIT_TERMINAL_PROMPT=0`: a credential prompt would hang CI, so the
/// effect fails closed instead (`E-GIT-FETCH`).
fn run_git(cwd: &Path, args: &[String]) -> Result<std::process::Output, GitError> {
    Command::new("git")
        .current_dir(cwd)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| GitError::Fetch {
            message: format!(
                "cannot run `git {}` (the fetch effect needs the git binary): {e}",
                args.first().unwrap_or(&String::new()),
            ),
        })
}

/// The stderr text of a finished process (lossy: diagnostics only).
fn stderr_text(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_owned()
}

/// The live fetcher: owns the opened [`gix::Repository`] (local tag
/// reads, remote-URL configuration) and drives the real `git` probe +
/// fetch subprocesses, rooted at the opened path.
pub struct GixTagFetcher {
    repo: gix::Repository,
    path: std::path::PathBuf,
}

impl GixTagFetcher {
    /// Opens `path` (worktree or `.git`) for fetching.
    ///
    /// # Errors
    ///
    /// [`GitError::Open`] when the repository cannot be opened.
    pub fn open(path: &Path) -> Result<Self, GitError> {
        let repo = gix::open(path).map_err(|e| GitError::Open {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        Ok(Self {
            repo,
            path: path.to_path_buf(),
        })
    }

    /// Whether `remote` has a configured URL (`remote.<name>.url`).
    fn remote_configured(&self, remote: &str) -> bool {
        self.repo
            .config_snapshot()
            .string(&format!("remote.{remote}.url"))
            .is_some()
    }

    /// The local peeled commit target of `tag` (annotated tags peel;
    /// `None` when the tag does not exist locally).
    ///
    /// # Errors
    ///
    /// [`GitError::RefScan`] when the reference cannot be read or peeled.
    fn local_tag_target(&self, tag: &str) -> Result<Option<String>, GitError> {
        let full_name = format!("refs/tags/{tag}");
        let Some(existing) = self
            .repo
            .try_find_reference(full_name.as_str())
            .map_err(|e| {
                crate::map_git_err("E-GIT-REF", format!("cannot read {full_name}: {e}"))
            })?
        else {
            return Ok(None);
        };
        let peeled = existing.into_fully_peeled_id().map_err(|e| {
            crate::map_git_err("E-GIT-REF", format!("cannot peel {full_name}: {e}"))
        })?;
        Ok(Some(peeled.to_string()))
    }
}

impl TagFetcher for GixTagFetcher {
    fn fetch_release_tags(
        &self,
        ws: &Workspace,
        cfg: &PlanConfig,
        remote: &str,
        base_branch: Option<&str>,
    ) -> Result<FetchReport, GitError> {
        // An unconfigured remote is a state, not an error: local-only
        // repositories have nothing to fetch from (the same
        // state-not-error convention the push phase uses).
        if !self.remote_configured(remote) {
            return Ok(FetchReport {
                base_branch: base_branch.map(str::to_owned),
                ..FetchReport::default()
            });
        }
        // §8.8 step 1: one cheap round trip; the wire format is the
        // ls-remote parse the push suite pins.
        let probe_args = vec![
            "ls-remote".to_owned(),
            "--tags".to_owned(),
            remote.to_owned(),
        ];
        let probe = run_git(&self.path, &probe_args)?;
        if !probe.status.success() {
            return Err(GitError::Fetch {
                message: format!(
                    "cannot probe remote {remote:?} (git ls-remote failed): {}",
                    stderr_text(&probe)
                ),
            });
        }
        let targets = parse_remote_tag_targets(&String::from_utf8_lossy(&probe.stdout));
        let selection = latest_tags_for(ws, cfg, &targets);
        let boundaries = deepen_boundaries_for(ws, cfg, &targets);
        // Classify each selected tag against the local refs: absent →
        // request it (create-only refspec), same target → skip (I1),
        // divergent → fail closed (release tags are never moved).
        let mut outcomes: BTreeMap<String, FetchRow> = BTreeMap::new();
        let mut to_fetch: Vec<String> = Vec::new();
        for tag in selection.values() {
            let remote_target = targets.get(tag).cloned().unwrap_or_default();
            match self.local_tag_target(tag)? {
                None => to_fetch.push(tag.clone()),
                Some(local) if local == remote_target => {
                    outcomes.insert(tag.clone(), FetchRow::AlreadyLocal { tag: tag.clone() });
                }
                Some(local) => {
                    outcomes.insert(
                        tag.clone(),
                        FetchRow::Failed {
                            tag: tag.clone(),
                            error: GitError::TagConflict {
                                tag: tag.clone(),
                                existing: remote_target,
                                requested: local,
                            },
                        },
                    );
                }
            }
        }
        let mut requested = to_fetch.clone();
        requested.sort();
        let base = base_branch.map(str::to_owned);
        // Nothing requested and no base branch: the effect stops before
        // spawning anything (the report carries the empty plan).
        if requested.is_empty() && base.is_none() {
            let mut report = FetchReport::tally(
                Some(remote.to_owned()),
                base,
                selection,
                outcomes.into_values().collect(),
            );
            report.probe_tags = targets.len();
            return Ok(report);
        }
        // §8.8 steps 2–3: the minimal fetch, retried once as the full
        // fetch of the same refs. Both attempts name exactly the same
        // refspecs — the fallback can never fetch beyond the plan. The
        // shallow machinery rides only when this repository is shallow
        // (on a complete clone git fails the fetch outright — nothing
        // to deepen).
        let shallow = crate::is_shallow(&self.repo);
        let attempts = fetch_attempts(remote, base_branch, &requested, &boundaries, shallow);
        let mut fallback_used = false;
        let mut executed: Vec<String> = Vec::new();
        let mut result: Result<(), GitError> = Err(GitError::Fetch {
            message: "the fetch plan is empty".to_owned(),
        });
        for (index, attempt) in attempts.iter().enumerate() {
            if index > 0 {
                fallback_used = true;
            }
            executed.clone_from(&attempt.argv);
            let output = run_git(&self.path, &attempt.argv)?;
            if output.status.success() {
                result = Ok(());
                break;
            }
            result = Err(GitError::Fetch {
                message: format!(
                    "`git {}` failed against remote {remote:?} ({}): {}",
                    attempt.command == FetchCommand::Minimal,
                    attempt.argv.join(" "),
                    stderr_text(&output)
                ),
            });
        }
        result?;
        for tag in &requested {
            outcomes.insert(tag.clone(), FetchRow::Fetched { tag: tag.clone() });
        }
        let mut report = FetchReport::tally(
            Some(remote.to_owned()),
            base,
            selection,
            outcomes.into_values().collect(),
        );
        report.probe_tags = targets.len();
        report.argv = executed;
        report.fallback_used = fallback_used;
        Ok(report)
    }
}
