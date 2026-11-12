//! The [`GitFixture`] declarative DSL (M0.5).
//!
//! Design constraints:
//!
//! * **Deterministic** — commits use fixed author/committer dates derived
//!   from a per-fixture sequence counter, fixed identity, and isolated
//!   configuration, so identical builds produce identical OIDs.
//! * **No shell interpolation** — every invocation is an argv array
//!   (invariant I7); fixture content is never interpolated into a shell.
//! * **Self-cleaning** — an owning fixture holds a `tempfile::TempDir`; the
//!   repo lives at `<tmp>/repo`, auxiliary worktrees at `<tmp>/wt-*`.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use crate::error::FixtureError;

/// Unix epoch seconds for `2025-01-01T00:00:00Z`, the fixture time base.
const BASE_SECS: i64 = 1_735_689_600;
/// Identity used for every commit and tag.
const FIXTURE_USER_NAME: &str = "Halyard Fixture";
/// Email used for every commit and tag.
const FIXTURE_USER_EMAIL: &str = "fixture@halyard.local";

/// Specification for one commit, as accepted by [`GitFixture::commit_with`].
#[derive(Clone, Debug, Default)]
pub struct CommitSpec {
    /// First line of the commit message.
    pub subject: String,
    /// Remaining message lines (trailers when placed at the end, e.g.
    /// `Release-As: 2.0.0`, `Co-authored-by: …`).
    pub body: Vec<String>,
    /// Files to write before staging, as `(repo-relative path, content)`
    /// pairs (always LF, UTF-8).
    pub files: Vec<(String, String)>,
    /// Whether an empty commit (no staged change) is allowed.
    pub allow_empty: bool,
}

impl CommitSpec {
    /// Convenience builder: a commit that writes files and stages them.
    pub fn files(files: Vec<(String, String)>, subject: impl Into<String>) -> Self {
        Self {
            subject: subject.into(),
            body: Vec::new(),
            files,
            allow_empty: false,
        }
    }
}

/// A deterministic, disposable git repository built by the scenario DSL.
///
/// * Owning fixtures (`new`, clones) clean up their temp directory on drop.
/// * Non-owning fixtures (`init_in`) live at a caller-chosen path (used by
///   `xtask gen-repo`) and are cleaned up by the caller.
#[derive(Debug)]
pub struct GitFixture {
    dir: Option<TempDir>,
    root: PathBuf,
    /// Empty config file handed to git as `GIT_CONFIG_GLOBAL` so the
    /// developer's/host's git configuration cannot leak in.
    empty_config: PathBuf,
    /// Commit counter driving deterministic author/committer dates.
    seq: i64,
}

impl GitFixture {
    /// Create a new fixture in a fresh temporary directory.
    pub fn new(label: &str) -> Result<Self, FixtureError> {
        let fixture = Self::new_uninit(label)?;
        fixture.git(&[
            "-c",
            "init.defaultBranch=main",
            "init",
            "--quiet",
            "-b",
            "main",
        ])?;
        Ok(fixture)
    }

    /// Create the fixture shell (temp dir + empty `repo/`) without
    /// initializing git — used as a clone target.
    fn new_uninit(label: &str) -> Result<Self, FixtureError> {
        let dir = TempDir::with_prefix(format!("halyard-fixture-{label}-"))
            .map_err(|e| FixtureError::io(format!("tempdir for {label}: {e}")))?;
        let root = dir.path().join("repo");
        std::fs::create_dir(&root)
            .map_err(|e| FixtureError::io(format!("create {}: {e}", root.display())))?;
        let empty_config = dir.path().join("empty-git-config");
        write_empty(&empty_config)?;
        Ok(Self {
            dir: Some(dir),
            root,
            empty_config,
            seq: 0,
        })
    }

    /// Create a fixture whose repository lives at the given directory (used
    /// by `xtask gen-repo`). Cleanup is the caller's responsibility.
    pub fn init_in(path: &Path) -> Result<Self, FixtureError> {
        if !path.is_dir() {
            return Err(FixtureError::io(format!(
                "fixture target {} is not a directory",
                path.display()
            )));
        }
        std::fs::create_dir_all(path.join(".git"))
            .map_err(|e| FixtureError::io(format!("mkdir .git: {e}")))?;
        let empty_config = path.join(".git").join("halyard-empty-config");
        write_empty(&empty_config)?;
        let fixture = Self {
            dir: None,
            root: path.to_path_buf(),
            empty_config,
            seq: 0,
        };
        fixture.git(&[
            "-c",
            "init.defaultBranch=main",
            "init",
            "--quiet",
            "-b",
            "main",
        ])?;
        Ok(fixture)
    }

    /// Repository working-tree root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `.git` directory of the fixture repository.
    pub fn git_dir(&self) -> PathBuf {
        self.root.join(".git")
    }

    /// Write a UTF-8/LF file inside the repo (parent dirs created).
    pub fn write(&self, rel: &str, content: &str) -> Result<(), FixtureError> {
        let path = self.root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| FixtureError::io(format!("mkdir {}: {e}", parent.display())))?;
        }
        std::fs::write(&path, content.as_bytes())
            .map_err(|e| FixtureError::io(format!("write {}: {e}", path.display())))
    }

    /// Stage every change in the working tree.
    pub fn stage_all(&self) -> Result<(), FixtureError> {
        self.git(&["add", "-A", "."])
    }

    /// Create a commit with the given subject; dates advance deterministically.
    pub fn commit(&mut self, subject: impl Into<String>) -> Result<String, FixtureError> {
        self.commit_with(&CommitSpec {
            subject: subject.into(),
            ..CommitSpec::default()
        })
    }

    /// Create a commit from a full [`CommitSpec`]; returns the new OID.
    pub fn commit_with(&mut self, spec: &CommitSpec) -> Result<String, FixtureError> {
        for (rel, content) in &spec.files {
            self.write(rel, content)?;
        }
        self.stage_all()?;
        let mut message = spec.subject.clone();
        if !spec.body.is_empty() {
            message.push_str("\n\n");
            message.push_str(&spec.body.join("\n"));
        }
        let mut args = vec!["commit", "--no-gpg-sign", "--quiet", "-m", message.as_str()];
        if spec.allow_empty {
            args.push("--allow-empty");
        }
        self.git_commit(&args)?;
        self.head_oid()?.ok_or_else(|| {
            FixtureError::git_output("commit succeeded but HEAD does not resolve".to_owned())
        })
    }

    /// Create an annotated tag at HEAD (no signing: fixtures never load
    /// keys).
    pub fn tag_annotated(&mut self, name: &str, message: &str) -> Result<(), FixtureError> {
        self.git(&["tag", "-a", name, "-m", message])
    }

    /// Create a lightweight tag at HEAD.
    pub fn tag_light(&mut self, name: &str) -> Result<(), FixtureError> {
        self.git(&["tag", name])
    }

    /// Create a branch at `at` (a revision; `None` = HEAD).
    pub fn branch(&mut self, name: &str, at: Option<&str>) -> Result<(), FixtureError> {
        match at {
            Some(rev) => self.git(&["branch", name, rev]),
            None => self.git(&["branch", name]),
        }
    }

    /// Check out an existing branch or revision (detached for OIDs).
    pub fn checkout(&mut self, target: &str) -> Result<(), FixtureError> {
        self.git(&["checkout", "--quiet", target])
    }

    /// Create and check out a new branch at `at` (`None` = HEAD).
    pub fn checkout_new(&mut self, name: &str, at: Option<&str>) -> Result<(), FixtureError> {
        match at {
            Some(rev) => self.git(&["checkout", "--quiet", "-b", name, rev]),
            None => self.git(&["checkout", "--quiet", "-b", name]),
        }
    }

    /// Merge `branch` into the current branch with a real merge commit
    /// (`--no-ff`).
    pub fn merge_no_ff(&mut self, branch: &str, message: &str) -> Result<(), FixtureError> {
        self.git_commit(&[
            "merge",
            "--no-ff",
            "--no-gpg-sign",
            "--quiet",
            "-m",
            message,
            branch,
        ])
    }

    /// Merge two or more branches in one octopus merge commit.
    pub fn merge_octopus(&mut self, branches: &[&str], message: &str) -> Result<(), FixtureError> {
        if branches.len() < 2 {
            return Err(FixtureError::scenario(
                "octopus merge needs at least two heads".to_owned(),
            ));
        }
        let mut args = vec![
            "merge".to_owned(),
            "--no-ff".to_owned(),
            "--no-gpg-sign".to_owned(),
            "--quiet".to_owned(),
            "-m".to_owned(),
            message.to_owned(),
        ];
        args.extend(branches.iter().map(|b| (*b).to_owned()));
        let args_ref: Vec<&str> = args.iter().map(String::as_str).collect();
        self.git_commit(&args_ref)
    }

    /// `git revert --no-edit <oid>`: produces the canonical
    /// `Revert "<subject>"` subject plus a `This reverts commit <oid>` body,
    /// matching the revert detection rules of §8.5.
    pub fn revert(&mut self, oid: &str) -> Result<String, FixtureError> {
        self.git_commit(&["revert", "--no-edit", "--no-gpg-sign", "--quiet", oid])?;
        self.head_oid()?.ok_or_else(|| {
            FixtureError::git_output("revert succeeded but HEAD does not resolve".to_owned())
        })
    }

    /// Write the commit-graph file (optionally with Bloom changed-path
    /// filters).
    pub fn commit_graph_write(&mut self, changed_paths: bool) -> Result<(), FixtureError> {
        if changed_paths {
            self.git(&["commit-graph", "write", "--reachable", "--changed-paths"])
        } else {
            self.git(&["commit-graph", "write", "--reachable"])
        }
    }

    /// Pack all refs into `packed-refs`.
    pub fn pack_refs(&mut self) -> Result<(), FixtureError> {
        self.git(&["pack-refs", "--all", "--no-prune"])
    }

    /// Allow this repo to serve `--filter=…` clones (used by
    /// [`GitFixture::partial_clone`]).
    pub fn enable_filter_serving(&mut self) -> Result<(), FixtureError> {
        self.git(&["config", "uploadpack.allowFilter", "true"])?;
        self.git(&["config", "uploadpack.allowAnySHA1InWant", "true"])
    }

    /// Add a linked worktree on a new branch; returns its path.
    ///
    /// Only available on owning fixtures (temp-dir based).
    pub fn worktree_add(&mut self, branch: &str) -> Result<PathBuf, FixtureError> {
        let dir = self.dir.as_ref().ok_or_else(|| {
            FixtureError::scenario(
                "worktrees are only supported on owning (temp-dir) fixtures".to_owned(),
            )
        })?;
        let name = branch.replace('/', "-");
        let path = dir.path().join(format!("wt-{name}"));
        self.git(&[
            "worktree",
            "add",
            "--quiet",
            "-b",
            branch,
            path.to_string_lossy().as_ref(),
        ])?;
        Ok(path)
    }

    /// Current HEAD commit OID, or `None` on an unborn branch.
    pub fn head_oid(&self) -> Result<Option<String>, FixtureError> {
        match self.git_raw(&["rev-parse", "--verify", "--quiet", "HEAD"])? {
            Some(out) => Ok(Some(one_oid(&out)?)),
            None => Ok(None),
        }
    }

    /// Resolve any revision to a full OID.
    pub fn resolve(&self, rev: &str) -> Result<String, FixtureError> {
        let out = self
            .git_raw(&["rev-parse", "--verify", rev])?
            .ok_or_else(|| FixtureError::git_output(format!("rev {rev} did not resolve")))?;
        one_oid(&out)
    }

    /// List tags (short names), sorted.
    pub fn ls_tags(&self) -> Result<Vec<String>, FixtureError> {
        let out = self
            .git_raw(&["for-each-ref", "--format=%(refname:short)", "refs/tags/"])?
            .unwrap_or_default();
        let mut tags: Vec<String> = out
            .lines()
            .map(str::to_owned)
            .filter(|l| !l.is_empty())
            .collect();
        tags.sort();
        Ok(tags)
    }

    /// Run `git rev-list <args…>` and return one OID per line.
    pub fn rev_list(&self, args: &[&str]) -> Result<Vec<String>, FixtureError> {
        let mut full = vec!["rev-list".to_owned()];
        full.extend(args.iter().map(|a| (*a).to_owned()));
        let args_ref: Vec<&str> = full.iter().map(String::as_str).collect();
        let out = self
            .git_raw(&args_ref)?
            .ok_or_else(|| FixtureError::git_output("rev-list produced no output".to_owned()))?;
        Ok(out
            .lines()
            .map(str::to_owned)
            .filter(|l| !l.is_empty())
            .collect())
    }

    /// Escape hatch: run an arbitrary git command (argv array) and return
    /// stdout. Commit-date env is **not** applied.
    pub fn run_git(&self, args: &[&str]) -> Result<String, FixtureError> {
        self.git_raw(args)?
            .ok_or_else(|| FixtureError::git_output("command produced no stdout".to_owned()))
    }

    /// Clone this fixture with `--depth=<depth>` (shallow) into a new
    /// fixture. Uses `file://` transport (`--no-local`) so the shallow
    /// machinery is genuinely exercised.
    pub fn shallow_clone(&self, depth: u32) -> Result<GitFixture, FixtureError> {
        let target = GitFixture::new_uninit("shallow")?;
        let target_root = target_path(&target)?;
        target.git(&[
            "clone",
            "--quiet",
            "--no-local",
            "--depth",
            &depth.to_string(),
            &file_uri(self.root()),
            &target_root,
        ])?;
        Ok(target)
    }

    /// Clone this fixture with `--filter=blob:none` (partial, blob-less)
    /// into a new fixture. The source must have called
    /// [`GitFixture::enable_filter_serving`].
    pub fn partial_clone(&self) -> Result<GitFixture, FixtureError> {
        let target = GitFixture::new_uninit("partial")?;
        let target_root = target_path(&target)?;
        target.git(&[
            "clone",
            "--quiet",
            "--no-local",
            "--filter=blob:none",
            &file_uri(self.root()),
            &target_root,
        ])?;
        Ok(target)
    }

    /// Absolute path of the repo as a `file://` URI.
    pub fn file_uri(&self) -> String {
        file_uri(self.root())
    }

    /// Run git with commit-date env derived from the current sequence, then
    /// advance the sequence. Used by every command that records history.
    fn git_commit(&mut self, args: &[&str]) -> Result<(), FixtureError> {
        let date = iso_from_epoch(BASE_SECS + self.seq);
        self.seq += 1;
        self.git_with(Some(&date), args)
    }

    /// Run git without commit-date env.
    fn git(&self, args: &[&str]) -> Result<(), FixtureError> {
        self.git_with(None, args)
    }

    fn git_with(&self, date: Option<&str>, args: &[&str]) -> Result<(), FixtureError> {
        self.git_raw_with(date, args).map(|_| ())
    }

    /// Core invocation returning stdout; `None` when the command exits with
    /// status 1 and no output (used for `--verify`-style probes). Any other
    /// failure is an error carrying argv + stderr.
    fn git_raw(&self, args: &[&str]) -> Result<Option<String>, FixtureError> {
        self.git_raw_with(None, args)
    }

    fn git_raw_with(
        &self,
        date: Option<&str>,
        args: &[&str],
    ) -> Result<Option<String>, FixtureError> {
        let mut cmd = Command::new("git");
        cmd.current_dir(&self.root);
        cmd.env("GIT_CONFIG_NOSYSTEM", "1");
        cmd.env("GIT_CONFIG_GLOBAL", &self.empty_config);
        cmd.env("GIT_AUTHOR_NAME", FIXTURE_USER_NAME);
        cmd.env("GIT_AUTHOR_EMAIL", FIXTURE_USER_EMAIL);
        cmd.env("GIT_COMMITTER_NAME", FIXTURE_USER_NAME);
        cmd.env("GIT_COMMITTER_EMAIL", FIXTURE_USER_EMAIL);
        if let Some(d) = date {
            cmd.env("GIT_AUTHOR_DATE", d);
            cmd.env("GIT_COMMITTER_DATE", d);
        }
        cmd.args(args);
        let output = cmd
            .output()
            .map_err(|e| FixtureError::git_invocation(format!("spawn git {args:?}: {e}")))?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if output.status.success() {
            Ok(Some(stdout))
        } else if output.status.code() == Some(1)
            && stdout.trim().is_empty()
            && stderr.trim().is_empty()
        {
            Ok(None)
        } else {
            Err(FixtureError::git_invocation(format!(
                "git {args:?} failed ({}): {}",
                output.status,
                stderr.trim()
            )))
        }
    }
}

fn target_path(fixture: &GitFixture) -> Result<String, FixtureError> {
    fixture
        .root()
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| FixtureError::io("clone target path is not valid UTF-8".to_owned()))
}

fn write_empty(path: &Path) -> Result<(), FixtureError> {
    let mut f = std::fs::File::create(path)
        .map_err(|e| FixtureError::io(format!("create {}: {e}", path.display())))?;
    f.write_all(b"")
        .map_err(|e| FixtureError::io(format!("write {}: {e}", path.display())))
}

fn one_oid(out: &str) -> Result<String, FixtureError> {
    let line = out.lines().next().unwrap_or_default().trim();
    if line.len() != 40 || !line.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(FixtureError::git_output(format!(
            "expected a 40-char OID, got {line:?}"
        )));
    }
    Ok(line.to_owned())
}

/// Convert an absolute path to a `file://` URI (Windows-aware).
fn file_uri(path: &Path) -> String {
    let p = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        format!("file:///{p}")
    } else {
        format!("file://{p}")
    }
}

/// Format epoch seconds as `YYYY-MM-DDTHH:MM:SS+00:00` (UTC) with pure
/// integer math — no clock reads anywhere in fixture construction.
fn iso_from_epoch(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}+00:00")
}

/// Howard Hinnant's `civil_from_days` (public-domain algorithm): converts a
/// count of days since 1970-01-01 into `(year, month, day)`. All components
/// are provably in-range for the fixture epoch window; `i64` throughout
/// avoids lossy casts.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_format_known_instants() {
        assert_eq!(iso_from_epoch(0), "1970-01-01T00:00:00+00:00");
        assert_eq!(iso_from_epoch(BASE_SECS), "2025-01-01T00:00:00+00:00");
        // 2025-12-31T23:59:59Z
        assert_eq!(iso_from_epoch(1_767_225_599), "2025-12-31T23:59:59+00:00");
        // Leap year: 2024-02-29T12:00:00Z
        assert_eq!(iso_from_epoch(1_709_208_000), "2024-02-29T12:00:00+00:00");
    }

    #[test]
    fn commit_dates_advance_deterministically() {
        let mut fx = GitFixture::new("dates").expect("fixture");
        fx.write("a.txt", "one\n").expect("write");
        let _c1 = fx.commit("feat: one").expect("commit");
        fx.write("a.txt", "two\n").expect("write");
        let c2 = fx.commit("feat: two").expect("commit");
        let shown = fx
            .run_git(&["log", "--format=%H %aI %an %ae"])
            .expect("log");
        let first = shown.lines().next().expect("first log line");
        assert!(
            first.contains("2025-01-01T00:00:01Z"),
            "second commit at +1s: {first}"
        );
        assert!(first.contains("Halyard Fixture fixture@halyard.local"));
        let head = fx.head_oid().expect("head").expect("head exists");
        assert_eq!(head, c2);
    }
}
