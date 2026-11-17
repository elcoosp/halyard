//! The CI-selector adapters' render cost (§16.3/M10 step 2, ADR-0034
//! + the ADR-0035/0036 adapters).
//!
//! Measures the seven adapters over the affected bench's shape (the
//! 40-package chain): the render must stay free — it runs on every CI
//! step, and a nanosecond-scale projection must never become the step's
//! cost.

use std::hint::black_box;

use criterion::Criterion;
use halyard_core::PkgId;
use halyard_core::affected::affected_selection;
use halyard_core::ci_selector::{SelectedPackage, ci_selector_adapter};
use halyard_core::graph::WorkspaceGraph;
use halyard_core::package::{
    DepEdge, DepKind, Ecosystem, GroupId, ManifestSite, Package, PathFilter, RelPath, ReqSpec,
    Span, TagTemplate, VersionSource, Workspace,
};
use semver::{Version, VersionReq};

/// The fixture: `packages` packages chained `pkgN depends on pkgN-1`
/// (one changed root reaches every dependent). The fixed literals make
/// every parse succeed; a failure at registration is skipped honestly.
fn chain_workspace(packages: u32) -> Option<(Workspace, Vec<PkgId>)> {
    let mut ws = Workspace::new();
    let mut ids = Vec::new();
    let site = RelPath::new("crates/x/Cargo.toml")?;
    for idx in 0..packages {
        let name = format!("pkg{idx:04}");
        let root = format!("crates/{name}");
        let pkg = Package {
            id: PkgId::FIRST,
            name: ws.intern(&name),
            ecosystem: Ecosystem::Cargo,
            root: RelPath::new(&root)?,
            manifest: RelPath::new(&format!("{root}/Cargo.toml"))?,
            version: Some(Version::parse("0.1.0").ok()?),
            version_source: VersionSource::Local,
            publishable: true,
            deps: Vec::new(),
            tag_template: TagTemplate::parse("{name}-v{version}").ok()?,
            changelog: None,
            version_site: None,
            include: PathFilter::new(Vec::new(), Vec::new()),
            group: GroupId::new(0),
            facets: halyard_core::Facets::default(),
        };
        ids.push(ws.push_package(pkg));
    }
    // Chain the edges over iterator pairs — no indexing (§15.3).
    for (dependent, dependency) in ids.iter().copied().zip(ids.iter().copied().skip(1)) {
        let req = ReqSpec::VersionReq(VersionReq::parse(">=0.1.0").ok()?);
        if let Some(pkg) = ws.package_mut(dependent) {
            pkg.deps.push(DepEdge {
                to: dependency,
                kind: DepKind::Normal,
                req,
                site: ManifestSite {
                    file: site.clone(),
                    spans: vec![Span { start: 0, end: 1 }],
                },
                raw: ">=0.1.0".into(),
            });
        }
    }
    Some((ws, ids))
}

fn bench_affected_ci(c: &mut Criterion) {
    let Some((ws, ids)) = chain_workspace(40) else {
        return;
    };
    let graph = WorkspaceGraph::build(&ws);
    let Some(root_id) = ids.first().copied() else {
        return;
    };
    let selection = affected_selection(&graph, &[root_id]);
    let resolve = |pkg: PkgId| -> Option<SelectedPackage> {
        let package = ws.package(pkg)?;
        let name = ws.interner().get(package.name)?;
        Some(SelectedPackage {
            name: name.to_owned(),
            root: package.root.as_str().to_owned(),
        })
    };
    let changed: Vec<SelectedPackage> = selection
        .rows()
        .iter()
        .filter(|row| row.reason == halyard_core::affected::AffectedReason::Changed)
        .filter_map(|row| resolve(row.package))
        .collect();
    let affected: Vec<SelectedPackage> = selection
        .rows()
        .iter()
        .filter_map(|row| resolve(row.package))
        .collect();
    let Some(turbo) = ci_selector_adapter("turbo") else {
        return;
    };
    let Some(nx) = ci_selector_adapter("nx") else {
        return;
    };
    let Some(bazel) = ci_selector_adapter("bazel") else {
        return;
    };
    let Some(gha) = ci_selector_adapter("gha") else {
        return;
    };
    let Some(gitlab) = ci_selector_adapter("gitlab") else {
        return;
    };
    let Some(azure) = ci_selector_adapter("azure") else {
        return;
    };
    let Some(teamcity) = ci_selector_adapter("teamcity") else {
        return;
    };
    let Some(circleci) = ci_selector_adapter("circleci") else {
        return;
    };
    let mut group = c.benchmark_group("core");
    group.bench_function("ci_selector_turbo_40chain", |b| {
        b.iter(|| black_box(turbo.render(black_box(&changed), black_box(&affected))));
    });
    group.bench_function("ci_selector_nx_40chain", |b| {
        b.iter(|| black_box(nx.render(black_box(&changed), black_box(&affected))));
    });
    group.bench_function("ci_selector_bazel_40chain", |b| {
        b.iter(|| black_box(bazel.render(black_box(&changed), black_box(&affected))));
    });
    group.bench_function("ci_selector_gha_40chain", |b| {
        b.iter(|| black_box(gha.render(black_box(&changed), black_box(&affected))));
    });
    group.bench_function("ci_selector_gitlab_40chain", |b| {
        b.iter(|| black_box(gitlab.render(black_box(&changed), black_box(&affected))));
    });
    group.bench_function("ci_selector_azure_40chain", |b| {
        b.iter(|| black_box(azure.render(black_box(&changed), black_box(&affected))));
    });
    group.bench_function("ci_selector_teamcity_40chain", |b| {
        b.iter(|| black_box(teamcity.render(black_box(&changed), black_box(&affected))));
    });
    group.bench_function("ci_selector_circleci_40chain", |b| {
        b.iter(|| black_box(circleci.render(black_box(&changed), black_box(&affected))));
    });
    group.finish();
}

/// Criterion harness entry (the `criterion_group!` expansion, spelled out
/// so the generated `pub fn` does not trip `missing_docs`).
fn benches() {
    let mut criterion: Criterion = Criterion::default().configure_from_args();
    bench_affected_ci(&mut criterion);
}

fn main() {
    benches();
    Criterion::default().configure_from_args().final_summary();
}
