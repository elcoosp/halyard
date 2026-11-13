//! `git-diff` (Slice C remainder): differential test of the trie-guided
//! tree-diff attribution against `git diff-tree` ground truth on local
//! fixtures.
//!
//! For each fixture repository (built with `halyard-test-support`), the
//! history is built via `halyard_git::GitHistory::build` and every digest
//! commit's per-package attribution is compared with the
//! `git diff-tree --no-commit-id --name-only -r <commit> <parent>` paths
//! mapped into the package root set (root `""` = every path). Any
//! mismatch fails the run; a summary is printed per fixture. Fixtures are
//! local git repos — the check runs without network.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use halyard_core::PkgId;
use halyard_core::package::{
    DepEdge, Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource,
    Workspace,
};
use halyard_core::{HistoryProvider, Oid};
use halyard_git::GitHistory;
use halyard_test_support::fixture::{CommitSpec, GitFixture};

/// Runs the differential tree-diff check over every fixture.
pub fn git_diff() -> Result<(), String> {
    let (c1, p1) = check_fixture(
        "gitdiff-root-single",
        &[("app", "", "v{version}")],
        &[
            ("src/lib.rs", "src/main.rs"),
            ("docs/note.md", "src/lib.rs"),
        ],
    )?;
    let (c2, p2) = check_fixture(
        "gitdiff-two-packages",
        &[
            ("a", "crates/a", "{name}-v{version}"),
            ("b", "crates/b", "{name}-v{version}"),
        ],
        &[
            ("crates/a/src/lib.rs", "crates/a/src/main.rs"),
            ("crates/b/src/lib.rs", "crates/b/src/main.rs"),
            ("crates/a/src/extra.rs", "crates/b/src/lib.rs"),
        ],
    )?;
    println!(
        "git-diff: {} commits across {} packages — attribution matches `git diff-tree` ground \
         truth",
        c1 + c2,
        p1 + p2
    );
    Ok(())
}

/// Builds one fixture, runs the engine and compares attribution.
///
/// `packages` is `(name, root, tag-template)`; `changes` lists the file
/// pairs (`this commit's file`, `next commit's file`) that the commit
/// sequence rotates through.
fn check_fixture(
    label: &str,
    packages: &[(&str, &str, &str)],
    changes: &[(&str, &str)],
) -> Result<(usize, usize), String> {
    let mut fx = GitFixture::new(label).map_err(|e| format!("fixture {label}: {e:?}"))?;
    let base_files = fixture_base_files(packages)?;
    fx.commit_with(&CommitSpec::files(base_files, "feat: base"))
        .map_err(|e| format!("{label}: base commit: {e:?}"))?;
    for (name, _, template) in packages {
        let tag = template
            .replace("{name}", name)
            .replace("{version}", "0.1.0");
        fx.tag_light(&tag)
            .map_err(|e| format!("{label}: tag {tag}: {e:?}"))?;
    }
    for i in 0..10usize {
        let (f1, f2) = changes
            .get(i % changes.len())
            .ok_or("fixture needs changes")?;
        let content = format!("pub const V{i}: u32 = {i};\n");
        let spec = CommitSpec::files(
            vec![(f1.to_string(), content.clone()), (f2.to_string(), content)],
            format!("feat: change {i}"),
        );
        fx.commit_with(&spec)
            .map_err(|e| format!("{label}: commit {i}: {e:?}"))?;
    }
    let ws = ws_with(packages)?;
    let toplevel = fx
        .run_git(&["rev-parse", "--show-toplevel"])
        .map_err(|e| format!("{label}: toplevel: {e}"))?;
    let dir = PathBuf::from(toplevel.trim());
    let history =
        GitHistory::build(&dir, &ws, 10_000).map_err(|e| format!("{label}: history build: {e}"))?;
    let digest = history.digest();
    let mut actual: Vec<BTreeSet<String>> = vec![BTreeSet::new(); ws.packages().len()];
    for (idx, commit) in digest.commits.iter().enumerate() {
        for (i, list) in digest.per_pkg.iter().enumerate() {
            let hit = list.contains(&idx);
            if hit && let Some(set) = actual.get_mut(i) {
                set.insert(commit.oid.as_str().to_owned());
            }
        }
    }
    let mut expected: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    for commit in &digest.commits {
        ground_truth(&fx, &ws, &commit.oid, &mut expected)?;
    }
    let mut mismatches = Vec::new();
    for (i, pkg) in ws.packages().iter().enumerate() {
        let want: BTreeSet<String> = expected
            .iter()
            .filter(|(_, pkgs)| pkgs.contains(&i))
            .map(|(oid, _)| oid.clone())
            .collect();
        let have = actual.get(i).cloned().unwrap_or_default();
        if want != have {
            mismatches.push(format!(
                "package at root {:?}: git diff-tree attributes {:?}, engine attributed {:?}",
                pkg.root.as_str(),
                want,
                have
            ));
        }
    }
    if !mismatches.is_empty() {
        return Err(format!(
            "git-diff: attribution mismatch in fixture {label:?}:\n  {}",
            mismatches.join("\n  ")
        ));
    }
    println!(
        "ok    {label}: {} commits, {} packages, attribution matches ground truth",
        digest.commits.len(),
        ws.packages().len()
    );
    Ok((digest.commits.len(), ws.packages().len()))
}

/// Builds the fixture workspace: one package per `(name, root, template)`.
fn ws_with(specs: &[(&str, &str, &str)]) -> Result<Workspace, String> {
    let mut ws = Workspace::new();
    for (name, root, template) in specs {
        push_pkg(&mut ws, name, root, template)?;
    }
    Ok(ws)
}

/// Registers one synthetic package (same shape as the engine tests).
fn push_pkg(ws: &mut Workspace, name: &str, root: &str, template: &str) -> Result<(), String> {
    let manifest = if root.is_empty() {
        "Cargo.toml".to_owned()
    } else {
        format!("{root}/Cargo.toml")
    };
    let pkg = Package {
        id: PkgId::FIRST,
        name: ws.intern(name),
        ecosystem: Ecosystem::Cargo,
        root: RelPath::new(root)
            .ok_or_else(|| format!("package root {root:?} is not a normalized path"))?,
        manifest: RelPath::new(&manifest)
            .ok_or_else(|| format!("manifest {manifest:?} is not a normalized path"))?,
        version: Some(semver::Version::new(0, 1, 0)),
        version_source: VersionSource::Local,
        publishable: true,
        deps: Vec::<DepEdge>::new(),
        tag_template: TagTemplate::parse(template)
            .map_err(|e| format!("tag template {template:?}: {e:?}"))?,
        changelog: None,
        version_site: None,
        include: PathFilter::new(Vec::new(), Vec::new()),
        group: GroupId::new(0),
        facets: halyard_core::Facets::default(),
    };
    ws.push_package(pkg);
    Ok(())
}

/// Computes the ground-truth attribution for one digest commit from
/// `git diff-tree` and maps the changed paths into package roots. Root
/// commits (no parent) are skipped: the engine never attributes them.
fn ground_truth(
    fx: &GitFixture,
    ws: &Workspace,
    oid: &Oid,
    expected: &mut BTreeMap<String, BTreeSet<usize>>,
) -> Result<(), String> {
    let oid_hex = oid.as_str();
    let parent = match fx.run_git(&["rev-parse", &format!("{oid_hex}^")]) {
        Ok(p) => p.trim().to_owned(),
        Err(_) => return Ok(()),
    };
    let out = fx
        .run_git(&[
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "-r",
            oid_hex,
            &parent,
        ])
        .map_err(|e| format!("diff-tree for {oid_hex}: {e}"))?;
    let mut pkgs = BTreeSet::new();
    for path in out.lines() {
        for (i, pkg) in ws.packages().iter().enumerate() {
            let root = pkg.root.as_str();
            let inside = root.is_empty()
                || (path.starts_with(root) && path.as_bytes().get(root.len()) == Some(&b'/'));
            if inside {
                pkgs.insert(i);
            }
        }
    }
    expected.insert(oid_hex.to_owned(), pkgs);
    Ok(())
}

/// The base-commit manifest files for a fixture: single-package root
/// manifest, or a `[workspace]` root plus one member manifest per
/// non-empty package root.
fn fixture_base_files(packages: &[(&str, &str, &str)]) -> Result<Vec<(String, String)>, String> {
    let mut base_files: Vec<(String, String)> = Vec::new();
    let nested: Vec<&(&str, &str, &str)> = packages
        .iter()
        .filter(|(_, root, _)| !root.is_empty())
        .collect();
    if nested.is_empty() {
        let (name, _, _) = packages.first().ok_or("fixture needs a package")?;
        base_files.push((
            "Cargo.toml".to_owned(),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n"),
        ));
    } else {
        let members = nested
            .iter()
            .map(|(name, _, _)| format!("\"crates/{name}\""))
            .collect::<Vec<_>>()
            .join(", ");
        base_files.push((
            "Cargo.toml".to_owned(),
            format!("[workspace]\nmembers = [{members}]\n"),
        ));
        for (name, _, _) in &nested {
            base_files.push((
                format!("crates/{name}/Cargo.toml"),
                format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n"),
            ));
        }
    }
    Ok(base_files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_to_package_mapping_respects_root_boundaries() {
        let ws = ws_with(&[
            ("a", "crates/a", "{name}-v{version}"),
            ("b", "crates/b", "{name}-v{version}"),
        ])
        .expect("workspace");
        let mut hits = vec![false; ws.packages().len()];
        for (i, pkg) in ws.packages().iter().enumerate() {
            let root = pkg.root.as_str();
            for path in [
                "crates/a/src/lib.rs",
                "crates/b/src/main.rs",
                "docs/note.md",
            ] {
                let m = root.is_empty()
                    || (path.starts_with(root) && path.as_bytes().get(root.len()) == Some(&b'/'));
                if m && let Some(slot) = hits.get_mut(i) {
                    *slot = true;
                }
            }
        }
        assert!(
            hits.first().copied().unwrap_or(false),
            "a hits its own path"
        );
        assert!(hits.get(1).copied().unwrap_or(false), "b hits its own path");
        assert!(
            !hits.iter().any(|h| !h),
            "no package may claim all paths here"
        );
    }
}
