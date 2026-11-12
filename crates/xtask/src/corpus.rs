//! `corpus` (M0.2): list/fetch the pinned corpus repositories.
//!
//! `corpus/manifest.toml` pins each repository to a commit SHA (captured via
//! `git ls-remote` at authoring time, 2026-11-12). `corpus fetch` materializes
//! clones under `corpus/clones/<name>/` (gitignored) and **verifies HEAD
//! equals the pinned SHA**:
//!
//! * `--shallow` (default): `git clone --depth 1 --filter=blob:none <url>` —
//!   fast; succeeds while the upstream default branch is still at the pinned
//!   SHA.
//! * `--full`: full clone + `checkout --detach <sha>` — always exact, used
//!   once upstream has moved past the pin.
//!
//! Either mode is reproducible by commit SHA: a re-fetch yields the same
//! tree, and any drift is a loud error, never a silent update.

use std::path::Path;
use std::process::Command;

use serde::Deserialize;

/// One pinned corpus repository.
#[derive(Debug, Deserialize)]
pub(crate) struct Repo {
    /// Short name used for the clone directory and reports.
    pub name: String,
    /// Clone URL.
    pub url: String,
    /// Pinned commit SHA (40 hex chars).
    pub sha: String,
    /// Ecosystems present (`cargo`, `npm`, …).
    pub ecosystems: Vec<String>,
    /// Corpus category (`cargo-only`, `pnpm-only`, `polyglot`, …).
    pub category: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Manifest {
    /// Manifest format version.
    #[serde(default = "default_version")]
    version: u32,
    /// Date the pins were captured.
    pinned_at: String,
    /// Pinned repositories.
    #[serde(default)]
    repo: Vec<Repo>,
}

fn default_version() -> u32 {
    1
}

/// Entry point for `xtask corpus <list|fetch>`.
pub fn corpus(args: &[String]) -> Result<(), String> {
    let (verb, rest) = match args.split_first() {
        Some((v, r)) => (v.as_str(), r),
        None => return Err("corpus needs a verb: list|fetch".to_owned()),
    };
    let root = crate::schema::workspace_root()?;
    let manifest = load_manifest(&root)?;
    match verb {
        "list" => {
            println!("name                 category       ecosystems                   pinned sha");
            for r in &manifest.repo {
                println!(
                    "{:<18} {:<12} {:<28} {}",
                    r.name,
                    r.category,
                    r.ecosystems.join(","),
                    &r.sha[..12.min(r.sha.len())]
                );
            }
            println!(
                "\n{} repos pinned at {} (manifest corpus/manifest.toml)",
                manifest.repo.len(),
                manifest.pinned_at
            );
            Ok(())
        }
        "fetch" => fetch(&root, &manifest, rest),
        other => Err(format!("unknown corpus verb {other:?} (list|fetch)")),
    }
}

/// Load and validate `corpus/manifest.toml`.
pub(crate) fn load_manifest(root: &Path) -> Result<Manifest, String> {
    let path = root.join("corpus/manifest.toml");
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let manifest: Manifest =
        toml::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))?;
    if manifest.version != 1 {
        return Err(format!(
            "corpus manifest version {} is not supported by this xtask (expected 1)",
            manifest.version
        ));
    }
    if manifest.repo.len() < 12 {
        return Err(format!(
            "corpus manifest pins {} repos; M0.2 requires >= 12",
            manifest.repo.len()
        ));
    }
    for r in &manifest.repo {
        if r.sha.len() != 40 || !r.sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!(
                "repo {}: pinned sha {:?} is not a 40-hex SHA",
                r.name, r.sha
            ));
        }
    }
    Ok(manifest)
}

fn fetch(root: &Path, manifest: &Manifest, args: &[String]) -> Result<(), String> {
    let mut shallow = true;
    let mut only: Option<String> = None;
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        match a.as_str() {
            "--shallow" => shallow = true,
            "--full" => shallow = false,
            "--only" => {
                only = Some(iter.next().cloned().ok_or("--only needs a value")?);
            }
            other => return Err(format!("unknown corpus fetch flag {other:?}")),
        }
    }
    let clones = root.join("corpus/clones");
    std::fs::create_dir_all(&clones).map_err(|e| format!("mkdir {}: {e}", clones.display()))?;
    let mut failures = Vec::new();
    for r in &manifest.repo {
        if let Some(want) = &only
            && r.name != *want
        {
            continue;
        }
        let target = clones.join(&r.name);
        match fetch_one(&target, r, shallow) {
            Ok(status) => println!("ok    {:<18} {status}", r.name),
            Err(e) => {
                println!("fail  {:<18} {e}", r.name);
                failures.push(format!("{}: {e}", r.name));
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} corpus fetch failures:\n  {}",
            failures.len(),
            failures.join("\n  ")
        ))
    }
}

fn fetch_one(target: &Path, repo: &Repo, shallow: bool) -> Result<String, String> {
    if target.join(".git").exists() {
        let head = git(target, &["rev-parse", "HEAD"])?;
        let head = head.trim().to_owned();
        if head == repo.sha {
            return Ok("already pinned".to_owned());
        }
        return Err(format!(
            "clone exists at HEAD {head} but manifest pins {}; remove the clone or update the pin",
            repo.sha
        ));
    }
    if shallow {
        git(
            target.parent().unwrap_or(Path::new(".")),
            &[
                "clone",
                "--depth",
                "1",
                "--filter=blob:none",
                &repo.url,
                &target.to_string_lossy(),
            ],
        )?;
        let head = git(target, &["rev-parse", "HEAD"])?.trim().to_owned();
        if head != repo.sha {
            return Err(format!(
                "shallow clone HEAD {head} != pinned {} (upstream moved); use `--full` or \
                 re-pin the manifest",
                repo.sha
            ));
        }
        Ok(format!("shallow clone at {}", &head[..12]))
    } else {
        git(
            target.parent().unwrap_or(Path::new(".")),
            &["clone", &repo.url, &target.to_string_lossy()],
        )?;
        git(target, &["checkout", "--quiet", "--detach", &repo.sha])?;
        let head = git(target, &["rev-parse", "HEAD"])?.trim().to_owned();
        if head != repo.sha {
            return Err(format!("checkout produced {head}, expected {}", repo.sha));
        }
        Ok("full clone at pin".to_owned())
    }
}

fn git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(args);
    let out = cmd
        .output()
        .map_err(|e| format!("spawn git {args:?}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "git {args:?} failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_has_at_least_twelve_valid_pins() {
        let root = crate::schema::workspace_root().expect("root");
        let m = load_manifest(&root).expect("manifest valid");
        let mut names: Vec<&str> = m.repo.iter().map(|r| r.name.as_str()).collect();
        names.sort_unstable();
        assert!(
            names.len() >= 12,
            "M0.2 requires >= 12 pinned repos, found {}",
            names.len()
        );
        names.dedup();
        assert_eq!(names.len(), m.repo.len(), "repo names must be unique");
    }
}
