//! Declarative manifest fixtures for the discovery differential tests
//! (M1.5/M1.6).
//!
//! [`CargoWorkspaceSpec`] rows describe complete Cargo workspaces as raw
//! TOML strings; [`NpmWorkspaceSpec`] rows describe npm/pnpm trees. The
//! writer materializes them inside a [`GitFixture`] so the committed-tree
//! layout matches what `cargo metadata` / `pnpm` see. The differential
//! comparator lives here too so both the unit tests and
//! `xtask discover-diff` share one implementation.

use std::collections::BTreeSet;
use std::process::Command;

use serde_json::Value;

use crate::error::FixtureError;
use crate::fixture::GitFixture;

/// A complete Cargo workspace fixture (M1.5 corpus of 25).
#[derive(Clone, Copy, Debug)]
pub struct CargoWorkspaceSpec {
    /// Fixture name (used for reporting and temp-dir prefixes).
    pub name: &'static str,
    /// Root `Cargo.toml` content.
    pub root: &'static str,
    /// Member directories with their `Cargo.toml` content; the empty
    /// directory name writes to the root (root-package workspaces).
    pub members: &'static [(&'static str, &'static str)],
}

/// A complete npm/pnpm workspace fixture (M1.6 corpus).
#[derive(Clone, Copy, Debug)]
pub struct NpmWorkspaceSpec {
    /// Fixture name.
    pub name: &'static str,
    /// Root `package.json` content (absent when `None`).
    pub root_json: Option<&'static str>,
    /// `pnpm-workspace.yaml` content (absent when `None`).
    pub pnpm_workspace: Option<&'static str>,
    /// Member directories with their `package.json` content.
    pub members: &'static [(&'static str, &'static str)],
}

/// Materializes a [`CargoWorkspaceSpec`] into a disposable git fixture
/// (deterministic identity per ADR-0003; discovery does not read history,
/// but fixtures stay real repositories for §8.6 parity later).
pub fn build_cargo_fixture(spec: &CargoWorkspaceSpec) -> Result<GitFixture, FixtureError> {
    let fixture = GitFixture::new(spec.name)?;
    fixture.write("Cargo.toml", spec.root)?;
    for (dir, manifest) in spec.members {
        let path = if dir.is_empty() {
            "Cargo.toml".to_owned()
        } else {
            format!("{dir}/Cargo.toml")
        };
        fixture.write(&path, manifest)?;
    }
    Ok(fixture)
}

/// Materializes an [`NpmWorkspaceSpec`].
pub fn build_npm_fixture(spec: &NpmWorkspaceSpec) -> Result<GitFixture, FixtureError> {
    let fixture = GitFixture::new(spec.name)?;
    if let Some(root) = spec.root_json {
        fixture.write("package.json", root)?;
    }
    if let Some(yaml) = spec.pnpm_workspace {
        fixture.write("pnpm-workspace.yaml", yaml)?;
    }
    for (dir, json) in spec.members {
        fixture.write(&format!("{dir}/package.json"), json)?;
    }
    Ok(fixture)
}

/// One workspace member as cargo metadata reports it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MetadataMember {
    /// Package name.
    pub name: String,
    /// Version string (`0.0.0` is cargo's rendering of a missing version).
    pub version: String,
}

/// One internal dependency edge as cargo metadata reports it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MetadataEdge {
    /// Depending package name.
    pub from: String,
    /// Dependency package name.
    pub to: String,
    /// Normalized kind (`normal` / `dev` / `build` / `optional`).
    pub kind: String,
}

/// The differential expectation extracted from `cargo metadata
/// --no-deps --format-version 1`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CargoMetadataExpectation {
    /// Workspace members (name + version), sorted.
    pub members: BTreeSet<MetadataMember>,
    /// Internal edges (member → member), sorted.
    pub edges: BTreeSet<MetadataEdge>,
}

/// Runs `cargo metadata --no-deps` in `repo_root` and extracts members +
/// internal edges. Subprocess use is test/tooling-only (invariant I6
/// applies to planning, not to ground-truth extraction).
pub fn cargo_metadata_expectation(repo_root: &std::path::Path) -> Result<CargoMetadataExpectation, FixtureError> {
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1", "--offline"])
        .current_dir(repo_root)
        .output()
        .map_err(|e| FixtureError::git_invocation(format!("spawn cargo metadata: {e}")))?;
    if !output.status.success() {
        return Err(FixtureError::git_output(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| FixtureError::git_output(format!("parse cargo metadata: {e}")))?;
    expectation_from_metadata(&value)
}

/// Pure extraction from already-parsed metadata (shared with xtask).
pub fn expectation_from_metadata(
    value: &Value,
) -> Result<CargoMetadataExpectation, FixtureError> {
    let packages = value
        .get("packages")
        .and_then(Value::as_array)
        .ok_or_else(|| FixtureError::git_output("cargo metadata: no packages".to_owned()))?;
    let members_ids: Vec<&str> = value
        .get("workspace_members")
        .and_then(Value::as_array)
        .ok_or_else(|| FixtureError::git_output("cargo metadata: no members".to_owned()))?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let id_to_name: std::collections::BTreeMap<&str, &str> = packages
        .iter()
        .filter_map(|p| {
            let id = p.get("id")?.as_str()?;
            let name = p.get("name")?.as_str()?;
            Some((id, name))
        })
        .collect();
    let mut members = BTreeSet::new();
    for pkg in packages {
        let id = pkg
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| FixtureError::git_output("package without id".to_owned()))?;
        if !members_ids.contains(&id) {
            continue;
        }
        members.insert(MetadataMember {
            name: pkg
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            version: pkg
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        });
    }
    let mut edges = BTreeSet::new();
    for pkg in packages {
        let id = pkg
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| FixtureError::git_output("package without id".to_owned()))?;
        if !members_ids.contains(&id) {
            continue;
        }
        let from = pkg
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let deps = pkg.get("deps").and_then(Value::as_array);
        for dep in deps.into_iter().flatten() {
            let target_id = dep.get("package").and_then(Value::as_str).unwrap_or("");
            if !members_ids.contains(&target_id) {
                continue; // external or path-outside-workspace
            }
            let to = id_to_name
                .get(target_id)
                .copied()
                .unwrap_or_default()
                .to_owned();
            let optional = dep.get("optional").and_then(Value::as_bool).unwrap_or(false);
            let kinds = dep.get("dep_kinds").and_then(Value::as_array);
            let kind_list: Vec<String> = kinds
                .into_iter()
                .flatten()
                .map(|k| {
                    k.get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or("normal")
                        .to_owned()
                })
                .collect();
            // A dep entry with an empty dep_kinds list is a plain
            // `[dependencies]` edge; otherwise one edge per kind.
            if kind_list.is_empty() {
                let kind = if optional { "optional" } else { "normal" };
                edges.insert(MetadataEdge {
                    from: from.clone(),
                    to: to.clone(),
                    kind: kind.to_owned(),
                });
            } else {
                for kind in &kind_list {
                    let kind = if optional { "optional" } else { kind.as_str() };
                    edges.insert(MetadataEdge {
                        from: from.clone(),
                        to: to.clone(),
                        kind: kind.to_owned(),
                    });
                }
            }
        }
    }
    Ok(CargoMetadataExpectation { members, edges })
}

/// Our discovery side of the differential: the same shape extracted from
/// a `Workspace`. Missing versions normalize to `0.0.0` (cargo's
/// rendering of a version-less package).
pub fn workspace_expectation(
    workspace: &halyard_core::package::Workspace,
) -> CargoMetadataExpectation {
    let interner = workspace.interner();
    let mut members = BTreeSet::new();
    let mut edges = BTreeSet::new();
    for pkg in workspace.packages() {
        let name = interner.get(pkg.name).unwrap_or_default().to_owned();
        members.insert(MetadataMember {
            name: name.clone(),
            version: pkg
                .version
                .as_ref()
                .map_or_else(|| "0.0.0".to_owned(), |v| v.to_string()),
        });
        for dep in &pkg.deps {
            let Some(target) = workspace.package(dep.to) else {
                continue;
            };
            let to = interner.get(target.name).unwrap_or_default().to_owned();
            let kind = match dep.kind {
                halyard_core::package::DepKind::Normal => "normal",
                halyard_core::package::DepKind::Dev => "dev",
                halyard_core::package::DepKind::Build => "build",
                halyard_core::package::DepKind::Peer => "peer",
                halyard_core::package::DepKind::Optional => "optional",
            };
            // Cargo metadata splits target-specific deps into separate
            // dep_kinds entries; our model keeps one edge per section, so
            // `[target]` + root sections on the same pair collapse here
            // and in the expectation builder below.
            edges.insert(MetadataEdge {
                from: name.clone(),
                to,
                kind: kind.to_owned(),
            });
        }
    }
    CargoMetadataExpectation { members, edges }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: CargoWorkspaceSpec = CargoWorkspaceSpec {
        name: "meta-extract",
        root: "[workspace]\nmembers = [\"crates/*\"]\n",
        members: &[(
            "crates/core",
            "[package]\nname = \"core\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1.0\"\n",
        )],
    };

    #[test]
    fn fixture_materializes_and_metadata_agrees() {
        let fixture = build_cargo_fixture(&SPEC).expect("fixture");
        let expectation = cargo_metadata_expectation(fixture.root()).expect("cargo metadata");
        assert_eq!(expectation.members.len(), 1);
        let member = expectation.members.iter().next().expect("member");
        assert_eq!(member.name, "core");
        assert!(expectation.edges.is_empty());
    }
}
