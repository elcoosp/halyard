//! The tag-push effect (§11.1 step 2: "push tags (single `git push`)",
//! §11.2) — the remote half of the tag phase, over the [`TagPusher`]
//! seam.
//!
//! The planning read path stays subprocess-free (I6) and network-free
//! (I5): pushing is an *explicitly listed effect step* of the release
//! flow (§11.1), sanctioned to spawn `git` with argv arrays (I7). The
//! wire formats are pinned by tests and benches against the output
//! probed empirically (ADR-0012):
//!
//! * `git ls-remote --tags <remote>` — `<oid>\t<ref>` rows, with a
//!   `<ref>^{}` peeled row following each annotated tag;
//! * `git push --porcelain <remote> <refspecs…>` — `<flag>\t<from>:<to>
//!   \t<summary>` rows (`*` new tag, `=` up to date, `!` rejected),
//!   terminated by a `Done` line.
//!
//! Conventions mirrored from the write path ([`crate::tag`]):
//!
//! * **I1 idempotence** — a tag already on the remote (probe or `=`
//!   row) reports [`TagPushRow::AlreadyOnRemote`] and sends nothing;
//! * **I2 resumability** — a divergent remote tag is a fail-closed
//!   `E-GIT-TAG-CONFLICT` (never forced, never moved); a partially
//!   completed phase re-runs to completion with zero duplicates;
//! * **I3 determinism** — the report is name-sorted regardless of the
//!   caller's input order;
//! * **I7 least privilege** — subprocesses receive argv arrays only,
//!   run with `GIT_TERMINAL_PROMPT=0` (a credential prompt would hang
//!   CI; auth failures surface as `E-GIT-PUSH` instead), and never pass
//!   `--force`, `--delete` or shell strings.
//!
//! The live writer ([`GixTagPusher`]) owns the real effects; the offline
//! forecast ([`PreviewTagPusher`]) probes what is probe-able without a
//! process (the configured remote) and forecasts every mutation — the
//! same split as [`crate::tag::GixTagWriter`] / [`crate::tag::PreviewTagWriter`].

use std::collections::BTreeMap;
use std::process::Command;

use crate::GitError;

/// One requested tag push, classified against the remote's tag state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TagPushRow {
    /// The remote accepted the ref (`[new tag]` / a non-`=` accepted
    /// flag).
    Pushed {
        /// The short tag name.
        tag: String,
    },
    /// The remote already carried the tag (the probe, or an `=` row) —
    /// nothing was sent (I1).
    AlreadyOnRemote {
        /// The short tag name.
        tag: String,
    },
    /// Forecast-only row of [`PreviewTagPusher`]: the offline run cannot
    /// observe the remote, so every requestable tag reports what a live
    /// run would send.
    WouldPush {
        /// The short tag name.
        tag: String,
    },
    /// The tag could not be pushed (missing locally, rejected by the
    /// remote, divergent remote tag). The phase continues past it (I2);
    /// the shell renders the error with its stable code.
    Failed {
        /// The short tag name.
        tag: String,
        /// The failure with its `E-GIT-*` code.
        error: GitError,
    },
}

/// The tag-push report (§11.1 step 2, remote half): one row per
/// (deduplicated) requested tag, name-sorted, with outcome counters.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TagPushReport {
    /// The remote the phase targeted. `None` when no
    /// `remote.<name>.url` is configured — an honest no-op phase (the
    /// tags stay local; the shell renders the skip).
    pub remote: Option<String>,
    /// Per-tag rows, name-sorted (I3: input order cannot leak).
    pub rows: Vec<TagPushRow>,
    /// Tags pushed to the remote.
    pub pushed: usize,
    /// Tags already on the remote (nothing sent).
    pub already_on_remote: usize,
    /// Tags that failed (the phase continues past them, I2).
    pub failed: usize,
}

impl TagPushReport {
    /// Tallies the rows into the outcome counters (the single source of
    /// the counters — callers cannot drift from the rows).
    fn tally(remote: Option<String>, rows: Vec<TagPushRow>) -> Self {
        let mut report = Self {
            remote,
            rows,
            ..Self::default()
        };
        for row in &report.rows {
            match row {
                TagPushRow::Pushed { .. } => report.pushed += 1,
                TagPushRow::AlreadyOnRemote { .. } => report.already_on_remote += 1,
                TagPushRow::Failed { .. } => report.failed += 1,
                TagPushRow::WouldPush { .. } => {}
            }
        }
        report
    }
}

/// The remote half of the tag phase (§11.1 step 2, §11.2). Object-safe
/// (`&dyn`) so the live shell runs [`GixTagPusher`] and the offline
/// forecast runs [`PreviewTagPusher`] — one seam, two effects, mirroring
/// the [`crate::tag::TagWriter`] split.
pub trait TagPusher {
    /// Pushes `tags` (short names) to `remote` (a configured git remote
    /// name, e.g. `origin`), create-only: never forces, never deletes.
    ///
    /// A remote without a configured URL yields an honest no-op report
    /// ([`TagPushReport::remote`] `None`). A configured-but-unreachable
    /// remote fails the phase with `E-GIT-PUSH`; a per-tag divergence
    /// (the remote carries the tag elsewhere) fails that row with
    /// `E-GIT-TAG-CONFLICT` — release tags are never moved.
    ///
    /// # Errors
    ///
    /// [`GitError::TagPush`] when the remote probe or the push itself
    /// cannot run (no `git` binary, unreachable remote, no parseable
    /// status output).
    fn push_tags(&self, remote: &str, tags: &[String]) -> Result<TagPushReport, GitError>;
}

/// One parsed `git push --porcelain` row (`<flag>\t<from>:<to>\t<summary>`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PorcelainRow {
    /// The status flag (`*` new tag, `=` up to date, `!` rejected —
    /// the format probed in ADR-0012).
    pub flag: char,
    /// The pushed tag's short name (parsed from the `to` refspec half).
    pub tag: String,
    /// The summary text (`[new tag]`, `[up to date]`,
    /// `[rejected] (already exists)`, …).
    pub summary: String,
}

/// Builds the explicit refspecs (`refs/tags/X:refs/tags/X`) for one
/// push. Explicit full-name refspecs (not `--tags`) push exactly the
/// requested tags — no surprise refs ride along (I7 least privilege).
#[must_use]
pub fn refspecs_for(tags: &[String]) -> Vec<String> {
    tags.iter()
        .map(|tag| format!("refs/tags/{tag}:refs/tags/{tag}"))
        .collect()
}

/// Parses `git ls-remote --tags` output into `tag → commit target` (the
/// `^{}`-peeled row wins for annotated tags; the object row stands in
/// for lightweight tags). Non-tag refs are skipped. Deterministic:
/// `BTreeMap` keyed by short name.
#[must_use]
pub fn parse_remote_tag_targets(output: &str) -> BTreeMap<String, String> {
    let mut targets = BTreeMap::new();
    for line in output.lines() {
        let Some((oid, reference)) = line.split_once('\t') else {
            continue;
        };
        let Some(name) = reference.strip_prefix("refs/tags/") else {
            continue;
        };
        if let Some(peeled) = name.strip_suffix("^{}") {
            targets.insert(peeled.to_owned(), oid.to_owned());
        } else {
            targets
                .entry(name.to_owned())
                .or_insert_with(|| oid.to_owned());
        }
    }
    targets
}

/// Parses `git push --porcelain` output into per-ref status rows for the
/// tag push (the `refs/tags/` refspec prefix). Header (`To …`), footer
/// (`Done`) and non-conforming lines are skipped; the wire format is
/// pinned by [`crate::push`] tests and benches.
#[must_use]
pub fn parse_porcelain_push(output: &str) -> Vec<PorcelainRow> {
    parse_porcelain_ref_push(output, "refs/tags/")
}

/// Parses `git push --porcelain` output into per-ref status rows for any
/// ref prefix (the tag push passes `refs/tags/`, the §11.2 branch push
/// passes `refs/heads/`). `PorcelainRow.tag` carries the pushed ref's
/// short name (the prefix stripped). The wire format is pinned by
/// [`crate::push`] tests and benches.
#[must_use]
pub fn parse_porcelain_ref_push(output: &str, prefix: &str) -> Vec<PorcelainRow> {
    let mut rows = Vec::new();
    for line in output.lines() {
        let mut fields = line.split('\t');
        let Some(flag_field) = fields.next() else {
            continue;
        };
        let Some(refspec_field) = fields.next() else {
            continue;
        };
        let Some(summary_field) = fields.next() else {
            continue;
        };
        // The flag is a single-character code on its own field (`*` new
        // ref, `=` up to date, `!` rejected — probed in ADR-0012); the
        // header (`To …`), footer (`Done`) and foreign lines carry other
        // shapes and are skipped.
        let flag = match flag_field.chars().collect::<Vec<_>>().as_slice() {
            [code] => *code,
            _ => continue,
        };
        let Some(short) = refspec_field
            .rsplit(':')
            .next()
            .and_then(|to| to.strip_prefix(prefix))
        else {
            continue;
        };
        rows.push(PorcelainRow {
            flag,
            tag: short.to_owned(),
            summary: summary_field.to_owned(),
        });
    }
    rows
}

/// The canonical request order (I3 armor on top of the caller's order,
/// mirroring [`crate::tag::run_tag_phase`]): sorted by name, deduplicated.
fn canonical(tags: &[String]) -> Vec<String> {
    let mut requested = tags.to_vec();
    requested.sort();
    requested.dedup();
    requested
}

/// The local commit target of `tag` (peeled), read in-process through
/// `gix` — no subprocess for a read.
fn local_tag_target(repo: &gix::Repository, tag: &str) -> Result<String, GitError> {
    let full_name = format!("refs/tags/{tag}");
    let Some(existing) =
        repo.try_find_reference(full_name.as_str())
            .map_err(|e| GitError::TagPush {
                message: format!("cannot read {full_name}: {e}"),
            })?
    else {
        return Err(GitError::TagPush {
            message: format!("tag {tag:?} does not exist locally; nothing to push"),
        });
    };
    let peeled = existing
        .into_fully_peeled_id()
        .map_err(|e| GitError::TagPush {
            message: format!("cannot peel {full_name}: {e}"),
        })?;
    Ok(peeled.to_string())
}

/// Runs a `git` effect subprocess (argv arrays only, I7) rooted at
/// `cwd` (the opened repository — the process cwd is *not* inherited,
/// so the effect cannot touch an enclosing repository by accident) with
/// `GIT_TERMINAL_PROMPT=0`: an auth prompt would hang CI, so the effect
/// fails closed instead (`E-GIT-PUSH`).
fn run_git(cwd: &std::path::Path, args: &[&str]) -> Result<std::process::Output, GitError> {
    Command::new("git")
        .current_dir(cwd)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| GitError::TagPush {
            message: format!(
                "cannot run `git {}` (the push effect needs the git binary): {e}",
                args.first().unwrap_or(&""),
            ),
        })
}

/// The stderr text of a finished process (lossy: diagnostics only).
fn stderr_text(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_owned()
}

/// The live writer: owns the opened [`gix::Repository`] (local tag
/// reads, remote-URL configuration) and drives the real `git` probe +
/// push subprocesses, rooted at the opened path.
pub struct GixTagPusher {
    repo: gix::Repository,
    path: std::path::PathBuf,
}

impl GixTagPusher {
    /// Opens `path` (worktree or `.git`) for pushing.
    ///
    /// # Errors
    ///
    /// [`GitError::Open`] when the repository cannot be opened.
    pub fn open(path: &std::path::Path) -> Result<Self, GitError> {
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
}

impl TagPusher for GixTagPusher {
    fn push_tags(&self, remote: &str, tags: &[String]) -> Result<TagPushReport, GitError> {
        // An unconfigured remote is a state, not an error: local-only
        // repositories release their tags without a push phase.
        if !self.remote_configured(remote) {
            return Ok(TagPushReport::default());
        }
        let requested = canonical(tags);
        if requested.is_empty() {
            return Ok(TagPushReport {
                remote: Some(remote.to_owned()),
                ..TagPushReport::default()
            });
        }
        // Probe once (§8.8 pattern: one cheap round trip), then classify
        // against the local targets: already-on-remote is a no-op (I1),
        // a divergent remote tag is fail-closed (never forced).
        let probe = run_git(&self.path, &["ls-remote", "--tags", remote])?;
        if !probe.status.success() {
            return Err(GitError::TagPush {
                message: format!(
                    "cannot probe remote {remote:?} (git ls-remote failed): {}",
                    stderr_text(&probe)
                ),
            });
        }
        let remote_targets = parse_remote_tag_targets(&String::from_utf8_lossy(&probe.stdout));
        // Per-tag outcomes keyed by name (deterministic order, I3); the
        // pre-push classification lands first, the push statuses replace
        // the pending entries.
        let mut outcomes: BTreeMap<String, TagPushRow> = BTreeMap::new();
        let mut to_push: Vec<String> = Vec::new();
        for tag in &requested {
            let local = match local_tag_target(&self.repo, tag) {
                Ok(target) => target,
                Err(error) => {
                    outcomes.insert(
                        tag.clone(),
                        TagPushRow::Failed {
                            tag: tag.clone(),
                            error,
                        },
                    );
                    continue;
                }
            };
            match remote_targets.get(tag) {
                Some(target) if *target == local => {
                    outcomes.insert(
                        tag.clone(),
                        TagPushRow::AlreadyOnRemote { tag: tag.clone() },
                    );
                }
                Some(target) => {
                    outcomes.insert(
                        tag.clone(),
                        TagPushRow::Failed {
                            tag: tag.clone(),
                            error: GitError::TagConflict {
                                tag: tag.clone(),
                                existing: target.clone(),
                                requested: local,
                            },
                        },
                    );
                }
                None => {
                    to_push.push(tag.clone());
                }
            }
        }
        // One push for the whole batch (§11.1: "push tags (single `git
        // push`)"); `--porcelain` attributes per-ref outcomes without a
        // fallback round trip. No `--force`, no `--delete` (I7): a race
        // rejection surfaces as a Failed row the re-run fixes up (I2).
        if !to_push.is_empty() {
            let refspecs = refspecs_for(&to_push);
            let mut args = vec!["push", "--porcelain", remote];
            args.extend(refspecs.iter().map(String::as_str));
            let pushed_output = run_git(&self.path, &args)?;
            let statuses = parse_porcelain_push(&String::from_utf8_lossy(&pushed_output.stdout));
            let by_tag: BTreeMap<&str, &PorcelainRow> =
                statuses.iter().map(|row| (row.tag.as_str(), row)).collect();
            for tag in &to_push {
                let row = match by_tag.get(tag.as_str()) {
                    Some(status) => match status.flag {
                        '=' => TagPushRow::AlreadyOnRemote { tag: tag.clone() },
                        '!' => TagPushRow::Failed {
                            tag: tag.clone(),
                            error: GitError::TagPush {
                                message: format!("git rejected {tag:?}: {}", status.summary),
                            },
                        },
                        _ => TagPushRow::Pushed { tag: tag.clone() },
                    },
                    None => TagPushRow::Failed {
                        tag: tag.clone(),
                        error: GitError::TagPush {
                            message: format!(
                                "git push produced no status for {tag:?}: {}",
                                stderr_text(&pushed_output)
                            ),
                        },
                    },
                };
                outcomes.insert(tag.clone(), row);
            }
        }
        Ok(TagPushReport::tally(
            Some(remote.to_owned()),
            outcomes.into_values().collect(),
        ))
    }
}

/// The dry-run writer (the push counterpart of
/// [`crate::tag::PreviewTagWriter`]): probes what is probe-able without
/// a process — the configured remote — and forecasts every mutation as
/// [`TagPushRow::WouldPush`]. No `git` subprocess runs and no network is
/// touched (I5), so `halyard release --dry` stays offline.
pub struct PreviewTagPusher {
    repo: gix::Repository,
}

impl PreviewTagPusher {
    /// Opens `path` for forecasting (probes only).
    ///
    /// # Errors
    ///
    /// [`GitError::Open`] when the repository cannot be opened.
    pub fn open(path: &std::path::Path) -> Result<Self, GitError> {
        let repo = gix::open(path).map_err(|e| GitError::Open {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        Ok(Self { repo })
    }

    /// Whether `remote` has a configured URL (the real probe — a
    /// configured remote is exactly what the live run would target).
    fn remote_configured(&self, remote: &str) -> bool {
        self.repo
            .config_snapshot()
            .string(&format!("remote.{remote}.url"))
            .is_some()
    }
}

impl TagPusher for PreviewTagPusher {
    fn push_tags(&self, remote: &str, tags: &[String]) -> Result<TagPushReport, GitError> {
        if !self.remote_configured(remote) {
            return Ok(TagPushReport::default());
        }
        let requested = canonical(tags);
        let rows = requested
            .into_iter()
            .map(|tag| TagPushRow::WouldPush { tag })
            .collect();
        Ok(TagPushReport::tally(Some(remote.to_owned()), rows))
    }
}
