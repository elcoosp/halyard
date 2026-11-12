//! Dependency-graph views over a [`Workspace`] (§6.6, M1.7).
//!
//! * **CSR adjacency** — compressed sparse row arrays for out-edges
//!   (`package → deps`) and in-edges (`dependents`), targets sorted by
//!   [`PkgId`] (then [`DepKind`]) so every traversal is deterministic
//!   (invariant I3).
//! * **Topological order** — Kahn's algorithm over **non-dev** edges;
//!   dev-only cycles are legal in Cargo (§6.6) and do not fail the sort.
//!   A cycle among the remaining edges yields
//!   [`GraphError::Cycle`] (`E-GRAPH-CYCLE`) carrying the printed cycle
//!   path.
//! * **SCCs** — iterative Tarjan over **all** edges (dev included), so a
//!   dev-dependency cycle is grouped as one component; only non-trivial
//!   components (size > 1 or self-loop) are reported.
//!
//! The graph is a **view**: it is rebuilt from a workspace after
//! registration completes, so it can never go stale mid-merge.

use crate::id::PkgId;
use crate::package::{DepKind, Workspace};

/// Graph construction / ordering errors (§15.4 `E-GRAPH-*`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GraphError {
    /// A cycle through non-dev edges exists (`E-GRAPH-CYCLE`). `path`
    /// lists the packages on the cycle, starting and ending at the same
    /// package; `rendered` is the human-readable `a -> b -> a` form.
    Cycle {
        /// Packages on the cycle (first == last).
        path: Vec<PkgId>,
        /// Rendered path (package names joined with ` -> `).
        rendered: String,
    },
}

impl GraphError {
    /// Stable §15.4 code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Cycle { .. } => "E-GRAPH-CYCLE",
        }
    }
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cycle { rendered, .. } => {
                write!(f, "dependency cycle through non-dev edges: {rendered}")
            }
        }
    }
}

impl std::error::Error for GraphError {}

/// CSR adjacency (compressed sparse row) over one edge direction.
#[derive(Clone, Debug, Default)]
struct Csr {
    /// Offsets of length `n + 1`; node `v`'s edges live at
    /// `targets[offsets[v]..offsets[v + 1]]`.
    offsets: Vec<u32>,
    /// Sorted edge targets.
    targets: Vec<PkgId>,
    /// Edge kinds, parallel to `targets`.
    kinds: Vec<DepKind>,
}

impl Csr {
    /// Builds the CSR from `(node, (target, kind))` edge tuples. Edges of
    /// each node are sorted by `(target, kind)` for determinism.
    fn build(n: usize, mut edges: Vec<(usize, PkgId, DepKind)>) -> Self {
        edges.sort_by_key(|(from, to, kind)| (*from, to.to_u32(), *kind));
        let mut offsets = Vec::with_capacity(n + 1);
        let mut targets = Vec::with_capacity(edges.len());
        let mut kinds = Vec::with_capacity(edges.len());
        let mut cursor = 0usize;
        for node in 0..=n {
            while cursor < edges.len() && edges[cursor].0 < node {
                targets.push(edges[cursor].1);
                kinds.push(edges[cursor].2);
                cursor += 1;
            }
            offsets.push(u32::try_from(targets.len()).unwrap_or(u32::MAX));
        }
        Self {
            offsets,
            targets,
            kinds,
        }
    }

    /// Edge slice bounds for `node` (`None` for a foreign id).
    fn bounds(&self, node: usize) -> Option<std::ops::Range<usize>> {
        let start = *self.offsets.get(node)? as usize;
        let end = *self.offsets.get(node + 1)? as usize;
        Some(start..end)
    }
}

/// Deterministic graph view over a [`Workspace`] (§6.6).
#[derive(Clone, Debug)]
pub struct WorkspaceGraph {
    count: usize,
    out: Csr,
    in_: Csr,
    /// Package names by id, captured for cycle-path rendering.
    names: Vec<String>,
}

impl WorkspaceGraph {
    /// Builds both CSR directions plus the name table from `ws`. In-edges
    /// mirror out-edges; duplicates (same target under two requirement
    /// sites) are preserved — each is a distinct edit site.
    pub fn build(ws: &Workspace) -> Self {
        let count = ws.packages().len();
        let mut out_edges: Vec<(usize, PkgId, DepKind)> = Vec::new();
        let mut in_edges: Vec<(usize, PkgId, DepKind)> = Vec::new();
        for pkg in ws.packages() {
            for dep in &pkg.deps {
                out_edges.push((pkg.id.to_usize(), dep.to, dep.kind));
                in_edges.push((dep.to.to_usize(), pkg.id, dep.kind));
            }
        }
        let names = ws
            .packages()
            .iter()
            .map(|p| ws.interner().get(p.name).unwrap_or_default().to_owned())
            .collect();
        Self {
            count,
            out: Csr::build(count, out_edges),
            in_: Csr::build(count, in_edges),
            names,
        }
    }

    /// Number of packages.
    pub fn len(&self) -> usize {
        self.count
    }

    /// True when the workspace had no packages.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Out-neighbors of `id` (sorted, may repeat under different edge
    /// kinds); `None` for a foreign id.
    pub fn out_neighbors(&self, id: PkgId) -> Option<&[PkgId]> {
        let bounds = self.out.bounds(id.to_usize())?;
        self.out.targets.get(bounds)
    }

    /// In-neighbors (dependents) of `id`; `None` for a foreign id.
    pub fn in_neighbors(&self, id: PkgId) -> Option<&[PkgId]> {
        let bounds = self.in_.bounds(id.to_usize())?;
        self.in_.targets.get(bounds)
    }

    /// Outgoing `(target, kind)` pairs of `id`, sorted; `None` for a
    /// foreign id.
    pub fn out_edges(&self, id: PkgId) -> Option<Vec<(PkgId, DepKind)>> {
        let bounds = self.out.bounds(id.to_usize())?;
        Some(
            (bounds)
                .map(|i| (self.out.targets[i], self.out.kinds[i]))
                .collect(),
        )
    }

    /// Topological order over **non-dev** edges, **dependencies first**
    /// (the publish order of §5.8 `order`: a package appears only after
    /// everything it depends on; §10.4 cascades over the reverse). A
    /// cycle through non-dev edges yields [`GraphError::Cycle`]
    /// (`E-GRAPH-CYCLE`) with the printed path; dev-only cycles are legal
    /// (§6.6). Deterministic: the ready set is processed in [`PkgId`]
    /// order, so one graph yields one total order.
    pub fn topo_order(&self) -> Result<Vec<PkgId>, GraphError> {
        let n = self.count;
        // A node is ready when every package it depends on (non-dev) has
        // already been emitted.
        let mut pending = vec![0u32; n];
        for node in 0..n {
            if let Some(bounds) = self.out.bounds(node) {
                for i in bounds {
                    if self.out.kinds[i] != DepKind::Dev {
                        pending[node] += 1;
                    }
                }
            }
        }
        let mut ready: std::collections::BTreeSet<u32> = (0..n as u32)
            .filter(|v| pending[*v as usize] == 0)
            .collect();
        let mut order = Vec::with_capacity(n);
        let mut seen = vec![false; n];
        while let Some(&v) = ready.first() {
            ready.remove(&v);
            let v = v as usize;
            seen[v] = true;
            order.push(PkgId::new(v as u32));
            // Emitting v unblocks every dependent u (u depends on v).
            if let Some(bounds) = self.in_.bounds(v) {
                for i in bounds {
                    if self.in_.kinds[i] == DepKind::Dev {
                        continue;
                    }
                    let u = self.in_.targets[i].to_usize();
                    pending[u] -= 1;
                    if pending[u] == 0 {
                        ready.insert(u as u32);
                    }
                }
            }
        }
        if order.len() == n {
            return Ok(order);
        }
        let path = self.find_cycle(&seen);
        let rendered = self.render(&path);
        Err(GraphError::Cycle { path, rendered })
    }

    /// Strongly connected components over **all** edges (dev included),
    /// non-trivial only (size > 1 or a self-loop), each sorted ascending,
    /// the list sorted by first member. Dev-dependency cycles therefore
    /// appear here as ordinary components without failing
    /// [`Self::topo_order`].
    pub fn sccs(&self) -> Vec<Vec<PkgId>> {
        let n = self.count;
        const UNVISITED: u32 = u32::MAX;
        let mut index = vec![UNVISITED; n];
        let mut low = vec![0u32; n];
        let mut on_stack = vec![false; n];
        let mut counter: u32 = 0;
        let mut stack: Vec<usize> = Vec::new();
        let mut components: Vec<Vec<PkgId>> = Vec::new();
        for start in 0..n {
            if index[start] != UNVISITED {
                continue;
            }
            index[start] = counter;
            low[start] = counter;
            counter += 1;
            stack.push(start);
            on_stack[start] = true;
            // Frame = (node, next out-edge slot to visit).
            let mut frames: Vec<(usize, usize)> = vec![(start, 0)];
            while let Some(frame) = frames.last_mut() {
                let (v, slot) = *frame;
                let bounds = self.out.bounds(v).unwrap_or(0..0);
                if slot < bounds.len() {
                    frame.1 += 1;
                    let w = self.out.targets[bounds.start + slot].to_usize();
                    if index[w] == UNVISITED {
                        index[w] = counter;
                        low[w] = counter;
                        counter += 1;
                        stack.push(w);
                        on_stack[w] = true;
                        frames.push((w, 0));
                    } else if on_stack[w] {
                        low[v] = low[v].min(index[w]);
                    }
                } else {
                    frames.pop();
                    if let Some(parent) = frames.last_mut() {
                        low[parent.0] = low[parent.0].min(low[v]);
                    }
                    if low[v] == index[v] {
                        let mut component: Vec<PkgId> = Vec::new();
                        while let Some(w) = stack.pop() {
                            on_stack[w] = false;
                            component.push(PkgId::new(w as u32));
                            if w == v {
                                break;
                            }
                        }
                        component.sort();
                        let self_loop = component.len() == 1 && self.self_loop(component[0]);
                        if component.len() > 1 || self_loop {
                            components.push(component);
                        }
                    }
                }
            }
        }
        components.sort();
        components
    }

    /// True when `id` has an out-edge to itself.
    pub fn self_loop(&self, id: PkgId) -> bool {
        self.out_neighbors(id)
            .is_some_and(|targets| targets.contains(&id))
    }

    /// Finds one cycle among the not-yet-ordered nodes, deterministically
    /// (DFS from the smallest remaining id, first back-edge wins).
    fn find_cycle(&self, ordered: &[bool]) -> Vec<PkgId> {
        let n = self.count;
        let remaining: Vec<usize> = (0..n).filter(|v| !ordered[*v]).collect();
        let start = match remaining.first() {
            Some(&s) => s,
            None => return Vec::new(),
        };
        let mut on_path: Vec<bool> = vec![false; n];
        let mut path: Vec<usize> = Vec::new();
        // Iterative DFS with explicit child cursors.
        let mut frames: Vec<(usize, usize)> = vec![(start, 0)];
        path.push(start);
        on_path[start] = true;
        while let Some(frame) = frames.last_mut() {
            let (v, slot) = *frame;
            let bounds = self.out.bounds(v).unwrap_or(0..0);
            if slot < bounds.len() {
                frame.1 += 1;
                let w = self.out.targets[bounds.start + slot].to_usize();
                if on_path[w] {
                    // Cycle: from the first occurrence of w on the path.
                    let pos = path.iter().position(|x| *x == w).unwrap_or(0);
                    let cycle = &path[pos..];
                    let mut out: Vec<PkgId> = cycle.iter().map(|x| PkgId::new(*x as u32)).collect();
                    out.push(PkgId::new(w as u32));
                    return out;
                }
                if !ordered[w] {
                    path.push(w);
                    on_path[w] = true;
                    frames.push((w, 0));
                }
            } else {
                frames.pop();
                if let Some(last) = path.pop() {
                    on_path[last] = false;
                }
            }
        }
        Vec::new()
    }

    /// Renders a package-id path as `a -> b -> a`.
    fn render(&self, path: &[PkgId]) -> String {
        path.iter()
            .map(|id| {
                self.names
                    .get(id.to_usize())
                    .map(String::as_str)
                    .unwrap_or("?")
            })
            .collect::<Vec<&str>>()
            .join(" -> ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::Facets;
    use crate::package::{
        DepEdge, Ecosystem, GroupId, ManifestSite, Package, PathFilter, RelPath, ReqSpec,
        TagTemplate, VersionSource,
    };

    /// Builds a workspace from `(name, [(dep-index, kind)])` specs. Node
    /// `i` gets id `i`, so graphs can be written index-first in tests.
    fn ws_from(specs: &[(&str, &[(usize, DepKind)])]) -> Workspace {
        let mut ws = Workspace::new();
        for (name, _) in specs {
            let pkg = Package {
                id: PkgId::FIRST,
                name: ws.intern(name),
                ecosystem: Ecosystem::Cargo,
                root: RelPath::new(&format!("crates/{name}")).unwrap(),
                manifest: RelPath::new(&format!("crates/{name}/Cargo.toml")).unwrap(),
                version: Some(semver::Version::parse("0.1.0").unwrap()),
                version_source: VersionSource::Local,
                publishable: true,
                deps: Vec::new(),
                tag_template: TagTemplate::parse("{name}-v{version}").unwrap(),
                changelog: None,
                include: PathFilter::new(Vec::new(), Vec::new()),
                group: GroupId::new(0),
                facets: Facets::default(),
            };
            ws.push_package(pkg);
        }
        for ((_, deps), index) in specs.iter().zip(0u32..) {
            let mut edges = Vec::new();
            for (dep_index, kind) in deps.iter() {
                edges.push(DepEdge {
                    to: PkgId::new(*dep_index as u32),
                    kind: *kind,
                    req: ReqSpec::PathOnly,
                    site: ManifestSite {
                        file: RelPath::new("Cargo.toml").unwrap(),
                        spans: Vec::new(),
                    },
                });
            }
            let id = PkgId::new(index);
            if let Some(pkg) = ws.package_mut(id) {
                pkg.deps = edges;
            }
        }
        ws
    }

    #[test]
    fn csr_construction_and_neighbors() {
        let ws = ws_from(&[("a", &[(1, DepKind::Normal)]), ("b", &[(0, DepKind::Dev), (2, DepKind::Normal)]), ("c", &[])]);
        let g = WorkspaceGraph::build(&ws);
        assert_eq!(g.len(), 3);
        assert_eq!(g.out_neighbors(PkgId::new(0)).unwrap(), &[PkgId::new(1)]);
        assert_eq!(g.in_neighbors(PkgId::new(2)).unwrap(), &[PkgId::new(1)]);
        let edges = g.out_edges(PkgId::new(1)).unwrap();
        assert_eq!(edges, vec![(PkgId::new(0), DepKind::Dev), (PkgId::new(2), DepKind::Normal)]);
        assert!(g.out_neighbors(PkgId::new(9)).is_none());
    }

    #[test]
    fn topo_is_total_order_on_dags() {
        let ws = ws_from(&[("a", &[(1, DepKind::Normal)]), ("b", &[(2, DepKind::Normal)]), ("c", &[])]);
        let g = WorkspaceGraph::build(&ws);
        let order = g.topo_order().unwrap();
        assert_eq!(order, vec![PkgId::new(2), PkgId::new(1), PkgId::new(0)]);
        // Same graph built twice gives the same order (I3).
        let again = WorkspaceGraph::build(&ws).topo_order().unwrap();
        assert_eq!(order, again);
    }

    #[test]
    fn dev_only_cycle_is_allowed_by_topo_and_grouped_by_scc() {
        let ws = ws_from(&[("a", &[(1, DepKind::Dev)]), ("b", &[(0, DepKind::Dev)]), ("c", &[])]);
        let g = WorkspaceGraph::build(&ws);
        assert!(g.topo_order().is_ok());
        let sccs = g.sccs();
        assert_eq!(sccs, vec![vec![PkgId::new(0), PkgId::new(1)]]);
    }

    #[test]
    fn normal_cycle_errors_with_printed_path() {
        let ws = ws_from(&[("a", &[(1, DepKind::Normal)]), ("b", &[(0, DepKind::Normal)])]);
        let g = WorkspaceGraph::build(&ws);
        let err = g.topo_order().unwrap_err();
        assert_eq!(err.code(), "E-GRAPH-CYCLE");
        let GraphError::Cycle { path, rendered } = &err else {
            panic!("expected Cycle");
        };
        assert_eq!(path.first(), path.last(), "cycle path is closed");
        assert_eq!(rendered, "a -> b -> a");
        assert!(err.to_string().contains("a -> b -> a"));
    }

    #[test]
    fn scc_finds_components_across_kinds() {
        // a -> b normal, b -> a dev (allowed pair) plus d -> e -> d normal
        // (real cycle): SCC reports both; topo fails on the normal one.
        // Indices: a=0, b=1, skip=2, d=3, e=4.
        let ws = ws_from(&[
            ("a", &[(1, DepKind::Normal)]),
            ("b", &[(0, DepKind::Dev)]),
            ("skip", &[]),
            ("d", &[(4, DepKind::Normal)]),
            ("e", &[(3, DepKind::Normal)]),
        ]);
        let g = WorkspaceGraph::build(&ws);
        assert!(g.topo_order().is_err());
        let sccs = g.sccs();
        assert_eq!(sccs.len(), 2);
        assert!(sccs.contains(&vec![PkgId::new(0), PkgId::new(1)]));
        assert!(sccs.contains(&vec![PkgId::new(2), PkgId::new(3)]));
    }

    #[test]
    fn self_loop_is_a_cycle() {
        let ws = ws_from(&[("a", &[(0, DepKind::Normal)])]);
        let g = WorkspaceGraph::build(&ws);
        assert!(g.self_loop(PkgId::new(0)));
        assert!(g.topo_order().is_err());
        assert_eq!(g.sccs().len(), 1);
    }

    #[test]
    fn empty_graph_orders_to_empty() {
        let ws = Workspace::new();
        let g = WorkspaceGraph::build(&ws);
        assert!(g.is_empty());
        assert_eq!(g.topo_order().unwrap(), Vec::<PkgId>::new());
        assert!(g.sccs().is_empty());
    }

    #[test]
    fn deep_chain_does_not_overflow_the_stack() {
        // 2000-node linear chain: every walk in this module is iterative,
        // so no recursion-depth failure (M1.7 requirement for large
        // synthetic monorepos).
        let mut specs: Vec<(String, Vec<(usize, DepKind)>)> = Vec::new();
        for i in 0..2000 {
            let deps = if i + 1 < 2000 {
                vec![(i + 1, DepKind::Normal)]
            } else {
                Vec::new()
            };
            specs.push((format!("p{i}"), deps));
        }
        let refs: Vec<(&str, &[(usize, DepKind)])> =
            specs.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
        let ws = ws_from(&refs);
        let g = WorkspaceGraph::build(&ws);
        assert_eq!(g.topo_order().unwrap().len(), 2000);
    }

    #[test]
    fn deep_cycle_is_found_iteratively() {
        // One 2000-node cycle: cycle search must terminate with a closed
        // path, not recurse to death.
        let mut specs: Vec<(String, Vec<(usize, DepKind)>)> = Vec::new();
        for i in 0..2000 {
            let next = (i + 1) % 2000;
            specs.push((format!("p{i}"), vec![(next, DepKind::Normal)]));
        }
        let refs: Vec<(&str, &[(usize, DepKind)])> =
            specs.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
        let ws = ws_from(&refs);
        let g = WorkspaceGraph::build(&ws);
        let err = g.topo_order().unwrap_err();
        let GraphError::Cycle { path, .. } = err else {
            panic!("expected Cycle");
        };
        assert_eq!(path.len(), 2001, "full cycle plus the closing node");
    }
}
