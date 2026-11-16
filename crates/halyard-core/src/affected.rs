//! The affected-package selector (§16.3 monorepo ergonomics, the M10
//! selector's step 1): the pure transitive-dependent closure over a
//! workspace's reverse dependency edges.
//!
//! The selection answers the CI question — *which packages must rebuild
//! and test because of this change?* — as a pure function of two inputs:
//! the workspace's dependency graph and the set of packages with pending
//! commits. Directly changed packages carry [`AffectedReason::Changed`];
//! every package that transitively depends on an affected package
//! carries [`AffectedReason::Dependent`] with the **nearest** affected
//! ancestor as `via` (the first discoverer over the sorted CSR in-edges,
//! so one graph yields one attribution — invariant I3).
//!
//! Two policy points, both documented divergences from the release
//! planner (§10.4):
//!
//! * **Dev edges count.** A dev-dependency still runs the dependent's
//!   test suite against the changed package, which is exactly what the
//!   CI selector is for. The publish order (§6.6) excludes dev edges;
//!   the selector does not.
//! * **Changed outranks dependent.** A package with its own pending
//!   commits is never demoted to a dependent row of something it
//!   depends on — the row stays the stronger reason.

use std::collections::VecDeque;

use crate::graph::WorkspaceGraph;
use crate::history::HistoryDigest;
use crate::id::PkgId;

/// Why a package is in the selection (§16.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AffectedReason {
    /// The package has pending commits of its own since its base (a
    /// change root).
    Changed,
    /// The package transitively depends on an affected dependency;
    /// `via` is the nearest affected ancestor (first discoverer over
    /// the sorted CSR in-edges, I3).
    Dependent {
        /// The nearest affected ancestor the discovery came through.
        via: PkgId,
    },
}

/// One row of the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AffectedRow {
    /// The affected package.
    pub package: PkgId,
    /// Why it is affected.
    pub reason: AffectedReason,
}

/// The deterministic selection: rows in ascending [`PkgId`] order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AffectedSelection {
    /// Rows sorted by package id (I3).
    rows: Vec<AffectedRow>,
}

impl AffectedSelection {
    /// The rows, ascending by package id.
    pub fn rows(&self) -> &[AffectedRow] {
        &self.rows
    }

    /// Number of affected packages.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// True when no package is affected.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The reason for one package (`None` when it is not affected).
    pub fn reason_of(&self, pkg: PkgId) -> Option<&AffectedReason> {
        self.rows
            .iter()
            .find(|row| row.package == pkg)
            .map(|row| &row.reason)
    }
}

/// The packages with pending commits (a non-empty per-package
/// attribution in the digest), in package order — the selector's change
/// roots as the shell derives them from the history walk.
pub fn changed_from_digest(digest: &HistoryDigest) -> Vec<PkgId> {
    digest
        .per_pkg
        .iter()
        .enumerate()
        .filter(|(_, commits)| !commits.is_empty())
        .map(|(idx, _)| PkgId::new(u32::try_from(idx).unwrap_or(u32::MAX)))
        .collect()
}

/// Closes the changed set over the workspace's reverse dependency edges.
///
/// The breadth-first visit seeds the changed packages (ascending id
/// order) and then walks the `in_neighbors` of every affected package —
/// all edge kinds, dev included (the module docs record why). A package
/// already in the selection is never re-attributed, so a change root is
/// never demoted to a dependent row and the `via` hop is the nearest
/// affected ancestor. Ids outside the graph (foreign or stale) are
/// skipped, not fatal. Deterministic (I3): the CSR in-edges are sorted
/// by target id and the queue is FIFO seeded in id order, so the same
/// inputs yield the same rows.
pub fn affected_selection(graph: &WorkspaceGraph, changed: &[PkgId]) -> AffectedSelection {
    let mut reasons: Vec<Option<AffectedReason>> = vec![None; graph.len()];
    let mut queue: VecDeque<PkgId> = VecDeque::new();
    let mut seed: Vec<PkgId> = changed.to_vec();
    seed.sort_unstable();
    seed.dedup();
    for pkg in seed {
        let idx = pkg.to_usize();
        // A foreign id has no in-edge slice bounds and no row slot.
        if idx >= graph.len() || reasons.get(idx).is_none() {
            continue;
        }
        let Some(slot) = reasons.get_mut(idx) else {
            continue;
        };
        if slot.is_none() {
            *slot = Some(AffectedReason::Changed);
            queue.push_back(pkg);
        }
    }
    while let Some(current) = queue.pop_front() {
        let Some(neighbors) = graph.in_neighbors(current) else {
            continue;
        };
        for dependent in neighbors {
            let idx = dependent.to_usize();
            let Some(slot) = reasons.get_mut(idx) else {
                continue;
            };
            if slot.is_some() {
                // Already affected: a change root stays Changed and a
                // discovered dependent keeps its nearest ancestor.
                continue;
            }
            *slot = Some(AffectedReason::Dependent { via: current });
            queue.push_back(*dependent);
        }
    }
    let rows = reasons
        .iter()
        .enumerate()
        .filter_map(|(idx, reason)| {
            reason.map(|why| AffectedRow {
                package: PkgId::new(u32::try_from(idx).unwrap_or(u32::MAX)),
                reason: why,
            })
        })
        .collect();
    AffectedSelection { rows }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::{
        DepEdge, DepKind, Ecosystem, GroupId, ManifestSite, Package, PathFilter, RelPath,
        TagTemplate, VersionSource, Workspace,
    };
    use semver::Version;

    /// A minimal two-package workspace wired `lib ← app`.
    fn two_package_workspace() -> (Workspace, PkgId, PkgId) {
        let mut ws = Workspace::new();
        let mut ids = Vec::new();
        for (name, root) in [("lib", "crates/lib"), ("app", "crates/app")] {
            let pkg = Package {
                id: PkgId::FIRST,
                name: ws.intern(name),
                ecosystem: Ecosystem::Cargo,
                root: RelPath::new(root).expect("fixture root"),
                manifest: RelPath::new(&format!("{root}/Cargo.toml")).expect("fixture manifest"),
                version: Some(Version::parse("0.1.0").expect("fixture version")),
                version_source: VersionSource::Local,
                publishable: true,
                deps: Vec::new(),
                tag_template: TagTemplate::parse("{name}-v{version}").expect("fixture template"),
                changelog: None,
                version_site: None,
                include: PathFilter::new(Vec::new(), Vec::new()),
                group: GroupId::new(0),
                facets: crate::Facets::default(),
            };
            ids.push(ws.push_package(pkg));
        }
        let lib = ids
            .first()
            .copied()
            .expect("the fixture registered two packages");
        let app = ids
            .get(1)
            .copied()
            .expect("the fixture registered two packages");
        let req = semver::VersionReq::parse(">=0.1.0")
            .ok()
            .map(crate::package::ReqSpec::VersionReq)
            .expect("fixture req");
        if let Some(pkg) = ws.package_mut(app) {
            pkg.deps.push(DepEdge {
                to: lib,
                kind: DepKind::Normal,
                req,
                site: ManifestSite {
                    file: RelPath::new("crates/app/Cargo.toml").expect("fixture site"),
                    spans: vec![crate::package::Span { start: 0, end: 1 }],
                },
                raw: ">=0.1.0".into(),
            });
        }
        (ws, lib, app)
    }

    #[test]
    fn closure_reaches_the_dependent() {
        let (ws, lib, app) = two_package_workspace();
        let graph = WorkspaceGraph::build(&ws);
        let selection = affected_selection(&graph, &[lib]);
        assert_eq!(selection.rows().len(), 2);
        assert_eq!(selection.reason_of(lib), Some(&AffectedReason::Changed));
        assert_eq!(
            selection.reason_of(app),
            Some(&AffectedReason::Dependent { via: lib })
        );
    }
}
