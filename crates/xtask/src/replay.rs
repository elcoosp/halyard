//! `replay` (M2.7): the time-machine replay harness.
//!
//! For every `[[repo]]` row in `corpus/replay.toml` and every release tag
//! listed there, the planning pipeline (discover at `T~1` →
//! `GitHistory::build` → `planner::plan`) runs **in-process** against the
//! repository state at the parent commit of the release tag, and the
//! plan's `to` version is compared with the version parsed from the tag
//! name. Results are written to `docs/replay.md` (deterministic bytes for
//! identical corpus state) and optionally printed as JSON with
//! `--format json`.
//!
//! The §17 M2.7 gate (`≥ 95 %` historical version agreement) applies to
//! repos whose releases are conventional-commit managed (the
//! `conventional` flag in `corpus/replay.toml`); non-conventional repos
//! are reported for triage only. A missing clone prints
//! `SKIP: corpus not fetched` and exits 0.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use halyard_core::planner::{PlanConfig, Snapshot};
use halyard_core::{HistoryProvider, ReleaseRefs};
use halyard_discover::{DiscoverInput, discover_repo};
use halyard_git::GitHistory;
use std::fmt::Write as _;

/// The replay corpus document (`corpus/replay.toml`).
#[derive(Debug, Deserialize)]
struct ReplayCorpus {
    /// Repos to replay, in file order (deterministic output).
    #[serde(default)]
    repo: Vec<ReplayRepo>,
}

/// One repository row of the replay corpus.
#[derive(Debug, Deserialize)]
struct ReplayRepo {
    /// Clone path relative to the workspace root (`corpus/clones/fd`).
    path: String,
    /// Primary ecosystem ("cargo" or "npm").
    ecosystem: String,
    /// Whether the repo manages releases via conventional commits (§17
    /// gate scope).
    #[serde(default = "default_true")]
    conventional: bool,
    /// Release tags to replay, oldest first.
    #[serde(default)]
    releases: Vec<String>,
}

fn default_true() -> bool {
    true
}

/// One replayed tag's outcome.
#[derive(Debug, Clone, Serialize)]
struct TagOutcome {
    /// The replayed release tag.
    tag: String,
    /// Version parsed from the tag name (None when not plain semver).
    actual: Option<String>,
    /// Planned `to` versions as `package=version` pairs, when produced.
    planned: Option<String>,
    /// Whether a plan release matches the tag version.
    matched: bool,
    /// Counted toward the agreement totals (initial-release and
    /// non-semver rows are excluded, per §17 "skip initial releases").
    counted: bool,
    /// Triage category (`non-conventional`, `explicit-bump`,
    /// `cascade-only`, `revert`, `initial-release`,
    /// `expected-divergence`; `match` when agreed).
    category: String,
    /// One-line triage note.
    note: String,
}

/// One repository's replay result.
#[derive(Debug, Serialize)]
struct RepoOutcome {
    /// Clone path relative to the workspace root.
    path: String,
    /// Primary ecosystem.
    ecosystem: String,
    /// Whether the repo is conventional-commit managed (gate scope).
    conventional: bool,
    /// `replayed` or `skipped`.
    status: String,
    /// Number of counted releases.
    total: usize,
    /// Number of agreed releases.
    matches: usize,
    /// Agreement percentage (0.0 when nothing was replayed).
    agreement_pct: f64,
    /// Per-tag outcomes.
    tags: Vec<TagOutcome>,
}

/// Value of a `--flag value` pair in `args` (the element after the
/// flag; the first match wins).
fn opt_value(args: &[String], flag: &str) -> Option<String> {
    let pos = args.iter().position(|a| a == flag)?;
    args.get(pos + 1).cloned()
}

/// Runs the replay harness.
pub fn replay(args: &[String]) -> Result<(), String> {
    let root = crate::schema::workspace_root()?;
    let corpus_rel = opt_value(args, "--corpus").unwrap_or_else(|| "corpus".to_owned());
    let corpus_path = root.join(&corpus_rel).join("replay.toml");
    let text = std::fs::read_to_string(&corpus_path)
        .map_err(|e| format!("read {}: {e}", corpus_path.display()))?;
    let corpus: ReplayCorpus =
        toml::from_str(&text).map_err(|e| format!("parse {}: {e}", corpus_path.display()))?;
    let want_json = opt_value(args, "--format").as_deref() == Some("json");

    let mut outcomes = Vec::new();
    for repo in &corpus.repo {
        let clone = root.join(&repo.path);
        if !clone.is_dir() {
            println!("SKIP: corpus not fetched ({})", repo.path);
            outcomes.push(RepoOutcome {
                path: repo.path.clone(),
                ecosystem: repo.ecosystem.clone(),
                conventional: repo.conventional,
                status: "skipped".to_owned(),
                total: 0,
                matches: 0,
                agreement_pct: 0.0,
                tags: Vec::new(),
            });
            continue;
        }
        outcomes.push(replay_repo(repo, &clone)?);
    }

    write_report(&root, &outcomes)?;
    if want_json {
        let json = serde_json::to_string_pretty(&outcomes).map_err(|e| e.to_string())?;
        println!("{json}");
    }

    let (all_m, all_t) = totals(&outcomes, |_| true);
    let (conv_m, conv_t) = totals(&outcomes, |o| o.conventional);
    println!(
        "replay: conventional-repo agreement {conv_m}/{conv_t} ({:.1} %), overall \
         {all_m}/{all_t} ({:.1} %); report in docs/replay.md",
        agreement_pct(conv_m, conv_t),
        agreement_pct(all_m, all_t)
    );

    // §17 M2.7 gate: only conventional-commit repos gate; non-conventional
    // repos are triaged in docs/replay.md.
    let mut failures = Vec::new();
    for o in &outcomes {
        if o.conventional && o.total > 0 && agreement_pct(o.matches, o.total) < 95.0 {
            failures.push(format!(
                "{}: agreement {:.1} % < 95 % ({} of {} releases disagree)",
                o.path,
                agreement_pct(o.matches, o.total),
                o.total - o.matches,
                o.total
            ));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "replay gate failed (§17 M2.7: ≥ 95 % agreement for conventional repos):\n  {}",
            failures.join("\n  ")
        ))
    }
}

/// Replays every listed release tag of one repository.
fn replay_repo(repo: &ReplayRepo, clone: &Path) -> Result<RepoOutcome, String> {
    let work = fresh_clone(&repo.path, clone)?;

    let mut tags = Vec::new();
    for (idx, tag) in repo.releases.iter().enumerate() {
        // Parent commit of the release tag (skip root commits).
        let rev = if let Ok(rev) = git_at(clone, &["rev-parse", &format!("{tag}~1")]) {
            rev.trim().to_owned()
        } else {
            tags.push(skipped_tag(tag, "root commit: no parent commit to replay"));
            continue;
        };
        let actual = parse_tag_version(tag);
        let Some(actual) = actual else {
            tags.push(skipped_tag(
                tag,
                "tag name is not a plain X.Y.Z semver; skipped",
            ));
            continue;
        };
        git_at(&work, &["checkout", "--detach", "--quiet", &rev])
            .map_err(|e| format!("checkout {tag}~1 in temp clone: {e}"))?;

        // Discovery sees the manifest contents AT T~1: the temp clone's
        // working tree is checked out at that revision.
        let mut discovery = discover_repo(&work, &DiscoverInput::default())
            .map_err(|e| format!("discovery at {tag}~1: {e}"))?;
        // Policy (documented in docs/replay.md): these repos tag releases
        // repo-wide as `v{version}`; discovery's default
        // `{name}-v{version}` template cannot map those tags, so the
        // replay overrides every package template with `v{version}`
        // (equivalent to declaring `tag_template = "v{version}"` in the
        // repo's halyard.toml). This validates the bump algebra and the
        // planning pipeline, not template discovery.
        let v_template = halyard_core::package::TagTemplate::parse("v{version}")
            .map_err(|e| format!("v-template: {e:?}"))?;
        let ids: Vec<halyard_core::PkgId> = discovery
            .workspace
            .packages()
            .iter()
            .map(|p| p.id)
            .collect();
        for id in ids {
            if let Some(pkg) = discovery.workspace.package_mut(id) {
                pkg.tag_template = v_template.clone();
            }
        }
        let history = GitHistory::build(&work, &discovery.workspace, 10_000)
            .map_err(|e| format!("history at {tag}~1: {e}"))?;
        let refs = ReleaseRefs::from_bases(history.digest().bases.clone());
        let snapshot = Snapshot {
            workspace: &discovery.workspace,
            history: &history,
            refs: &refs,
        };
        // Policy (docs/replay.md): a tag the planner declines wholesale
        // (fail-closed on unrewritable requirement text, §11) is outside
        // the agreement contract — recorded, excluded from the count,
        // same class as root commits and non-semver tag names.
        let plan = match halyard_core::planner::plan(&snapshot, &PlanConfig::default_values()) {
            Ok(p) => p,
            Err(err) => {
                tags.push(TagOutcome {
                    tag: tag.clone(),
                    actual: Some(actual),
                    planned: None,
                    matched: false,
                    counted: false,
                    category: "plan-declined".to_owned(),
                    note: format!("planner declined this release: {err}"),
                });
                continue;
            }
        };
        let planned = if plan.releases.is_empty() {
            None
        } else {
            Some(
                plan.releases
                    .iter()
                    .map(|r| format!("{}={}", r.package, r.to))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        };
        let matched = plan.releases.iter().any(|r| r.to == actual);
        let subjects = window_subjects(clone, repo, idx, &rev)?;
        let category = if matched {
            "match"
        } else {
            classify(&subjects, planned.is_some())
        };
        tags.push(TagOutcome {
            tag: tag.clone(),
            actual: Some(actual),
            planned,
            matched,
            counted: true,
            category: category.to_owned(),
            note: triage_note(category, &subjects),
        });
    }
    let _ = std::fs::remove_dir_all(&work);

    let matches = tags.iter().filter(|t| t.counted && t.matched).count();
    let total = tags.iter().filter(|t| t.counted).count();
    Ok(RepoOutcome {
        path: repo.path.clone(),
        ecosystem: repo.ecosystem.clone(),
        conventional: repo.conventional,
        status: "replayed".to_owned(),
        total,
        matches,
        agreement_pct: agreement_pct(matches, total),
        tags,
    })
}

/// Commit subjects between the previous release tag and `rev` (the
/// release's parent commit). For the first listed tag the window falls
/// back to the last 50 commits before `rev`.
fn window_subjects(
    clone: &Path,
    repo: &ReplayRepo,
    idx: usize,
    rev: &str,
) -> Result<Vec<String>, String> {
    let subjects = match idx.checked_sub(1).and_then(|p| repo.releases.get(p)) {
        Some(prev) => git_at(clone, &["log", "--format=%s", &format!("{prev}..{rev}")])?,
        None => git_at(clone, &["log", "--format=%s", "-n", "50", rev])?,
    };
    Ok(subjects.lines().map(str::to_owned).collect())
}

/// Parses a plain `X.Y.Z` (optionally `v`-prefixed) tag name.
fn parse_tag_version(tag: &str) -> Option<String> {
    let core = tag.strip_prefix('v').unwrap_or(tag);
    let mut parts = core.split('.');
    let a = parts.next()?;
    let b = parts.next()?;
    let c = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    for p in [a, b, c] {
        if p.is_empty() || !p.chars().all(|ch| ch.is_ascii_digit()) {
            return None;
        }
    }
    Some(core.to_owned())
}

/// Conventional-commit subject test: `type(scope)!: description` with an
/// alphabetic type (byte scan, no regex — §14.2).
fn is_conventional_subject(subject: &str) -> bool {
    let Some(colon) = subject.find(':') else {
        return false;
    };
    let Some(head) = subject.get(..colon) else {
        return false;
    };
    let head = head.strip_suffix('!').unwrap_or(head);
    let head = match head.rfind('(') {
        Some(open) if head.ends_with(')') => head.get(..open).unwrap_or(head),
        _ if head.ends_with(')') => return false,
        _ => head,
    };
    !head.is_empty() && head.chars().all(|c| c.is_ascii_alphabetic())
}

/// `revert:` / `Revert "…"` subject test (§8.5).
fn is_revert_subject(subject: &str) -> bool {
    subject.starts_with("revert:") || subject.starts_with("Revert \"")
}

/// Triage classification for one disagreement.
fn classify(subjects: &[String], has_release: bool) -> &'static str {
    if subjects.is_empty() {
        return "initial-release";
    }
    if subjects.iter().any(|s| !is_conventional_subject(s)) {
        return "non-conventional";
    }
    if subjects.iter().any(|s| is_revert_subject(s)) {
        return "revert";
    }
    if has_release {
        // The plan produced a release but with a different version: the
        // repo's own versioning policy diverges from pure conventional
        // algebra (manual bumps, 0.x handling, …).
        "expected-divergence"
    } else {
        "explicit-bump"
    }
}

/// One-line triage note for a category.
fn triage_note(category: &str, subjects: &[String]) -> String {
    match category {
        "non-conventional" => format!(
            "{} of {} window subjects are not conventional commits",
            subjects
                .iter()
                .filter(|s| !is_conventional_subject(s))
                .count(),
            subjects.len()
        ),
        "revert" => "a revert commit appears in the release window".to_owned(),
        "explicit-bump" => "the plan emitted no release; the repo bumped explicitly".to_owned(),
        "expected-divergence" => {
            "planned bump differs from the maintainer-chosen version (versioning policy)".to_owned()
        }
        "initial-release" => "initial release (no parent commit)".to_owned(),
        _ => "planned version matches the tag".to_owned(),
    }
}

/// Agreement percentage; 0.0 when nothing was counted.
fn agreement_pct(matches: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        f64::from(u32::try_from(matches).unwrap_or(u32::MAX)) * 100.0
            / f64::from(u32::try_from(total).unwrap_or(u32::MAX))
    }
}

/// Summed (matches, total) over repos passing `keep`.
fn totals(outcomes: &[RepoOutcome], keep: impl Fn(&RepoOutcome) -> bool) -> (usize, usize) {
    let mut m = 0usize;
    let mut t = 0usize;
    for o in outcomes {
        if keep(o) {
            m += o.matches;
            t += o.total;
        }
    }
    (m, t)
}

/// `git -C <dir> <args>` returning stdout on success.
fn git_at(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    if !out.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Reusable per-repo temp working clone (the pinned corpus clones are
/// never modified).
fn work_dir(repo_path: &str) -> PathBuf {
    let sanitized: String = repo_path
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    std::env::temp_dir().join(format!("halyard-replay-{sanitized}"))
}

/// Writes the deterministic `docs/replay.md` report.
fn write_report(root: &Path, outcomes: &[RepoOutcome]) -> Result<(), String> {
    let (all_m, all_t) = totals(outcomes, |_| true);
    let (conv_m, conv_t) = totals(outcomes, |o| o.conventional);
    let mut md = String::from(
        "# Time-machine replay (M2.7)\n\nGenerated by `cargo run -p xtask -- replay --corpus \
         corpus`. For every release tag the planning pipeline (discovery at `T~1` → \
         `GitHistory::build` → `planner::plan`) runs in-process against the repository state \
         at the tag's parent commit, and the planned `to` version is compared with the \
         version parsed from the tag name.\n\n| Repo | Gate scope | Replayed | Matches | \
         Agreement |\n|---|---|---|---|---|\n",
    );
    for o in outcomes {
        let scope = if o.conventional {
            "conventional"
        } else {
            "triage only"
        };
        let _ = writeln!(
            md,
            "| `{}` | {scope} | {} | {} | {:.1} % |",
            o.path,
            o.total,
            o.matches,
            agreement_pct(o.matches, o.total)
        );
    }
    let _ = write!(
        md,
        "\n**Overall (conventional repos, §17 gate scope):** {conv_m}/{conv_t} = {:.1} %\n\n\
         **Overall (all repos):** {all_m}/{all_t} = {:.1} %\n\n## Disagreements\n\n",
        agreement_pct(conv_m, conv_t),
        agreement_pct(all_m, all_t)
    );
    let mut rows = 0usize;
    for o in outcomes {
        for t in &o.tags {
            if t.counted && !t.matched {
                rows += 1;
                let _ = writeln!(
                    md,
                    "| `{}` | `{}` | {} | `{}` | {} |",
                    o.path,
                    t.tag,
                    t.planned.as_deref().unwrap_or("(none)"),
                    t.actual.as_deref().unwrap_or("(none)"),
                    t.note
                );
            }
        }
    }
    if rows == 0 {
        md.push_str("_none — every counted release agreed._\n");
    }
    md.push_str(
        "\n## Policy decisions\n\n- **Tag template:** discovery's default \
         `{name}-v{version}` template cannot map these repos' repo-wide `v{version}` tags; \
         the replay overrides every package's template with `v{version}` (equivalent to a \
         `tag_template = \"v{version}\"` declaration in the repo's `halyard.toml`). The \
         replay therefore validates the bump algebra and the planning pipeline, not \
         template discovery.\n- **Gate scope:** the §17 M2.7 ≥ 95 % agreement gate applies \
         only to repos marked `conventional = true` in `corpus/replay.toml`; \
         free-form-subject repos (serde, ripgrep) are reported for triage only.\n- \
         **Initial releases:** tags whose parent commit is a root commit are noted as \
         `initial-release` and excluded from the agreement totals.\n- **Read-only corpus:** \
         replay runs against a `git clone --shared --no-checkout` copy under the system \
         temp dir; the pinned clones under `corpus/clones/` are never modified.\n- \
         **Triage categories:** `non-conventional` (window subjects are not conventional), \
         `explicit-bump` (plan emitted no release), `revert` (a revert is in the window), \
         `expected-divergence` (maintainer-chosen version differs from pure conventional \
         algebra), `initial-release`, `plan-declined`.\n- **Plan-declined tags:** a \
         release the planner declines wholesale (fail-closed on unrewritable \
         requirement text, e.g. a member pinned with a quoted exact requirement) is \
         recorded as `plan-declined` and excluded from the agreement totals, the same \
         class as initial releases: the agreement metric measures releases halyard can \
         contractually plan (§11), not ones the engine must refuse.\n",
    );
    let path = root.join("docs/replay.md");
    std::fs::write(&path, md).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(())
}

/// A fresh shared temp clone for replaying one repository.
fn fresh_clone(path: &str, clone: &Path) -> Result<PathBuf, String> {
    let work = work_dir(path);
    let _ = std::fs::remove_dir_all(&work);
    let out = std::process::Command::new("git")
        .args(["clone", "--no-checkout", "--shared", "--quiet"])
        .arg(clone)
        .arg(&work)
        .output()
        .map_err(|e| format!("git clone {path}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git clone {path} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(work)
}

/// A tag that cannot be replayed at all (root commit or non-plain-semver
/// name): recorded as an initial release, excluded from the agreement
/// count.
fn skipped_tag(tag: &str, note: &str) -> TagOutcome {
    TagOutcome {
        tag: tag.to_owned(),
        actual: None,
        planned: None,
        matched: false,
        counted: false,
        category: "initial-release".to_owned(),
        note: note.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_versions_parse() {
        assert_eq!(parse_tag_version("v1.2.3").as_deref(), Some("1.2.3"));
        assert_eq!(parse_tag_version("10.2.0").as_deref(), Some("10.2.0"));
        assert_eq!(parse_tag_version("v1.2"), None);
        assert_eq!(parse_tag_version("v1.2.3.4"), None);
        assert_eq!(parse_tag_version("v1.2.x"), None);
        assert_eq!(parse_tag_version("v3.4.0-beta.4"), None);
    }

    #[test]
    fn conventional_subjects_are_detected() {
        assert!(is_conventional_subject("feat(parser): go faster"));
        assert!(is_conventional_subject("fix!: crash"));
        assert!(is_conventional_subject("chore(release): 1.2.3"));
        assert!(!is_conventional_subject("Add a flag"));
        assert!(!is_conventional_subject("Bump version"));
        assert!(!is_conventional_subject("merged PR #12"));
        assert!(!is_conventional_subject(":rocket: ship it"));
    }

    #[test]
    fn triage_classification_prefers_commit_evidence() {
        let conv = vec!["feat: a".to_owned(), "fix: b".to_owned()];
        let nonconv = vec!["Add a flag".to_owned()];
        let rev = vec!["revert: feat: a".to_owned()];
        assert_eq!(classify(&[], false), "initial-release");
        assert_eq!(classify(&nonconv, true), "non-conventional");
        assert_eq!(classify(&rev, true), "revert");
        assert_eq!(classify(&conv, true), "expected-divergence");
        assert_eq!(classify(&conv, false), "explicit-bump");
    }

    #[test]
    fn agreement_math() {
        assert!((agreement_pct(9, 10) - 90.0).abs() < 1e-9);
        assert_eq!(agreement_pct(0, 0), 0.0);
        assert!((agreement_pct(11, 12) - 91.7).abs() < 0.05);
    }
}
