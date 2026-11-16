//! The affected-package selector's throughput (§16.3/M10).
//!
//! The closure is the CI-selector hot loop: a reverse BFS over the CSR
//! in-edges. The bench pins the two extremes the walk can hit on a real
//! workspace: the shape ceiling (a 40-package chain — one changed root
//! reaches every dependent) and the fan-out (one changed root with 39
//! direct dependents), plus the empty-selection baseline.

use std::hint::black_box;

use criterion::Criterion;
use halyard_core::PkgId;
use halyard_core::affected::affected_selection;
use halyard_core::graph::WorkspaceGraph;
use halyard_core::package::{
    DepEdge, DepKind, Ecosystem, GroupId, ManifestSite, Package, PathFilter, RelPath, ReqSpec,
    Span, TagTemplate, VersionSource, Workspace,
};
use semver::{Version, VersionReq};

/// Registers `packages` packages with sane defaults and no edges.
fn workspace_with(packages: u32) -> (Workspace, Vec<PkgId>) {
    let mut ws = Workspace::new();
    let mut ids = Vec::new();
    for idx in 0..packages {
        let name = format!("pkg{idx:04}");
        let root = format!("crates/{name}");
        let pkg = Package {
            id: PkgId::FIRST,
            name: ws.intern(&name),
            ecosystem: Ecosystem::Cargo,
            root: RelPath::new(&root).unwrap_or_else(|| unreachable!("fixture root is valid")),
            manifest: RelPath::new(&format!("{root}/Cargo.toml"))
                .unwrap_or_else(|| unreachable!("fixture manifest is valid")),
            version: Some(Version::new(0, 1, 0)),
            version_source: VersionSource::Local,
            publishable: true,
            deps: Vec::new(),
            tag_template: TagTemplate::parse("{name}-v{version}")
                .unwrap_or_else(|e| unreachable!("fixture template is valid: {e}")),
            changelog: None,
            version_site: None,
            include: PathFilter::new(Vec::new(), Vec::new()),
            group: GroupId::new(0),
            facets: halyard_core::Facets::default(),
        };
        ids.push(ws.push_package(pkg));
    }
    (ws, ids)
}

/// Adds an edge `from → to` (the fixture requirement is never re-parsed
/// by the code under test).
fn add_edge(ws: &mut Workspace, from: PkgId, to: PkgId) {
    let req = match VersionReq::parse(">=0.1.0") {
        Ok(parsed) => ReqSpec::VersionReq(parsed),
        Err(e) => unreachable!("fixture requirement is valid: {e}"),
    };
    let site = RelPath::new("Cargo.toml").unwrap_or_else(|| unreachable!("fixture site is valid"));
    if let Some(pkg) = ws.package_mut(from) {
        pkg.deps.push(DepEdge {
            to,
            kind: DepKind::Normal,
            req,
            site: ManifestSite {
                file: site,
                spans: vec![Span { start: 0, end: 1 }],
            },
            raw: ">=0.1.0".into(),
        });
    }
}

fn bench_affected(c: &mut Criterion) {
    let count = 40u32;

    // The chain: pkg0000 ← pkg0001 ← … — one changed root closes all 40
    // rows (the closure's worst-case shape on a release train).
    let (mut chain_ws, chain_ids) = workspace_with(count);
    for pair in chain_ids.windows(2) {
        if let [dependency, dependent] = pair {
            add_edge(&mut chain_ws, *dependency, *dependent);
        }
    }
    let chain_graph = WorkspaceGraph::build(&chain_ws);
    let chain_root = chain_ids
        .first()
        .copied()
        .unwrap_or_else(|| unreachable!("the chain has a root"));
    drop(chain_ws);

    // The fan-out: pkg0000 ← everything else — 39 direct dependents, a
    // one-layer reverse walk (the shared-core shape).
    let (mut fan_ws, fan_ids) = workspace_with(count);
    let fan_root = fan_ids
        .first()
        .copied()
        .unwrap_or_else(|| unreachable!("the fan-out has a root"));
    for dependent in fan_ids.iter().skip(1) {
        add_edge(&mut fan_ws, *dependent, fan_root);
    }
    let fan_graph = WorkspaceGraph::build(&fan_ws);
    drop(fan_ws);

    let mut group = c.benchmark_group("affected");
    group.bench_function("chain_40", |biter| {
        biter.iter(|| affected_selection(black_box(&chain_graph), black_box(&[chain_root])));
    });
    group.bench_function("fanout_39", |biter| {
        biter.iter(|| affected_selection(black_box(&fan_graph), black_box(&[fan_root])));
    });
    group.bench_function("no_change_baseline", |biter| {
        biter.iter(|| affected_selection(black_box(&chain_graph), black_box(&[] as &[PkgId])));
    });
    group.finish();
}

fn main() {
    bench_affected(&mut Criterion::default().configure_from_args());
}
