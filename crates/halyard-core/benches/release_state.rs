//! Reconciliation throughput (§11.1 step 2).
//!
//! The manifest-vs-tags reconciliation runs once per `pr`/`release` pass
//! over every package × every tag; the bench pins the sweep's cost
//! across workspace and tag-table sizes so the tag-phase forecast stays
//! negligible against the §14.1 planning budgets.

use std::hint::black_box;

use criterion::Criterion;
use halyard_core::package::{
    Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource, Workspace,
};
use halyard_core::planner::PlanConfig;
use halyard_core::release_state::{pending_releases, reconcile_tags};
use halyard_core::{BString, PkgId};

/// Builds a synthetic workspace of `packages` members with per-package
/// `{name}-v{version}` templates.
fn workspace(packages: u32) -> Workspace {
    let mut ws = Workspace::new();
    let mut idx = 0;
    while idx < packages {
        let name = format!("pkg{idx:04}");
        let pkg = Package {
            id: PkgId::new(idx),
            name: ws.intern(&name),
            ecosystem: Ecosystem::Cargo,
            root: RelPath::new(&format!("crates/{name}"))
                .unwrap_or_else(|| unreachable!("fixture root is valid")),
            manifest: RelPath::new(&format!("crates/{name}/Cargo.toml"))
                .unwrap_or_else(|| unreachable!("fixture manifest is valid")),
            version: Some(semver::Version::new(
                1,
                u64::from(idx % 90 + 10),
                u64::from(idx % 9),
            )),
            version_source: VersionSource::Local,
            publishable: true,
            deps: Vec::new(),
            tag_template: TagTemplate::parse("{name}-v{version}")
                .unwrap_or_else(|e| unreachable!("fixture template is valid: {e}")),
            changelog: None,
            version_site: None,
            include: PathFilter::new(Vec::new(), Vec::new()),
            group: GroupId::new(0),
            facets: halyard_core::discover::Facets::default(),
        };
        ws.push_package(pkg);
        idx += 1;
    }
    ws
}

/// Builds the tag table: every package's latest release tag plus some
/// unrelated noise tags (real repos carry foreign tags too).
fn tags(ws: &Workspace, per_package: u32) -> Vec<BString> {
    let mut out = Vec::new();
    let mut idx = 0;
    while idx < per_package {
        for pkg in ws.packages() {
            let name = ws.interner().get(pkg.name).unwrap_or_default();
            let version = semver::Version::new(1, u64::from(idx), 0);
            out.push(BString::from(
                pkg.tag_template.render(name, &version, "cargo"),
            ));
        }
        idx += 1;
    }
    out.push(BString::from("v0.0.0-noise"));
    out.push(BString::from("nightly-build-noise"));
    out
}

fn bench_reconcile(c: &mut Criterion) {
    let mut group = c.benchmark_group("release_state");
    for (packages, per_package) in [(10u32, 5u32), (40, 5), (200, 5), (40, 50)] {
        let ws = workspace(packages);
        let tag_table = tags(&ws, per_package);
        let cfg = PlanConfig::default_values();
        group.bench_function(format!("reconcile_{packages}x{per_package}"), |biter| {
            biter.iter(|| reconcile_tags(black_box(&ws), black_box(&tag_table), &cfg));
        });
        group.bench_function(format!("pending_{packages}x{per_package}"), |biter| {
            biter.iter(|| pending_releases(black_box(&ws), black_box(&tag_table), &cfg));
        });
    }
    group.finish();
}

/// Criterion entry point (hand-written instead of `criterion_group!` so the
/// generated function keeps its doc comment under `missing_docs`).
fn main() {
    let mut c = Criterion::default().configure_from_args();
    bench_reconcile(&mut c);
}
