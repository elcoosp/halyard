//! Publish-engine benchmark (M5.2, §13): the orchestration cost the
//! engine itself adds on top of the registry round trips it saves —
//! layer planning over the workspace graph (pure, once per run) and one
//! full engine pass over scripted fakes (the resume probe + the
//! visibility wait + the report, 40 targets in 4 layers). The network
//! round trip dominates a real publish; the engine must stay trivially
//! cheap against it.

use std::sync::Arc;
use std::time::Duration;

use criterion::{BatchSize, Criterion};
use halyard_core::package::{
    DepEdge, DepKind, Ecosystem, GroupId, ManifestSite, Package, PathFilter, RelPath, ReqSpec,
    Span, TagTemplate, VersionSource, Workspace,
};
use halyard_core::plan::{PLAN_SCHEMA_V1, Plan, PlanPublish, PlanRelease};
use halyard_core::{PkgId, plan_hash};
use halyard_registry::VisibilityPolicy;

use halyard_publish::{
    MapRouter, MemoryPublishEffect, MemoryRegistry, PublishPolicy, plan_publication,
    run_publication,
};

/// A 40-package workspace: 4 layers of 10, each package depends on one
/// package of the previous layer (`p1..p10` = layer 0).
fn ws_40() -> Workspace {
    let mut ws = Workspace::new();
    let mut ids: Vec<PkgId> = Vec::new();
    for i in 0..40 {
        let name = format!("p{i}");
        let root = format!("crates/p{i}");
        let manifest = format!("crates/p{i}/Cargo.toml");
        let pkg = Package {
            id: PkgId::FIRST,
            name: ws.intern(&name),
            ecosystem: Ecosystem::Cargo,
            root: RelPath::new(&root).unwrap_or_else(|| unreachable!("fixed root")),
            manifest: RelPath::new(&manifest).unwrap_or_else(|| unreachable!("fixed manifest")),
            version: Some(
                semver::Version::parse("0.1.0")
                    .unwrap_or_else(|e| unreachable!("fixed version: {e}")),
            ),
            version_source: VersionSource::Local,
            publishable: true,
            deps: Vec::new(),
            tag_template: TagTemplate::parse("{name}-v{version}")
                .unwrap_or_else(|e| unreachable!("fixed template: {e}")),
            changelog: None,
            version_site: Some(ManifestSite {
                file: RelPath::new(&manifest).unwrap_or_else(|| unreachable!("fixed manifest")),
                spans: vec![Span { start: 40, end: 45 }],
            }),
            include: PathFilter::new(Vec::new(), Vec::new()),
            group: GroupId::new(0),
            facets: halyard_core::Facets::default(),
        };
        ids.push(ws.push_package(pkg));
    }
    for (i, id) in ids.iter().enumerate() {
        if i >= 10 {
            let Some(dep) = ids.get(i - 10) else {
                unreachable!("the fixed chain indexes in range")
            };
            let dep = *dep;
            let manifest = format!("crates/p{i}/Cargo.toml");
            if let Some(pkg) = ws.package_mut(*id) {
                pkg.deps.push(DepEdge {
                    to: dep,
                    kind: DepKind::Normal,
                    req: ReqSpec::VersionReq(
                        semver::VersionReq::parse(">=0.1.0")
                            .unwrap_or_else(|e| unreachable!("fixed req: {e}")),
                    ),
                    site: ManifestSite {
                        file: RelPath::new(&manifest)
                            .unwrap_or_else(|| unreachable!("fixed manifest")),
                        spans: vec![Span { start: 10, end: 17 }],
                    },
                    raw: ">=0.1.0".into(),
                });
            }
        }
    }
    ws
}

/// The 40-release plan over `ws_40`, self-consistent hash (I8).
fn plan_40() -> Plan {
    let releases = (0..40)
        .map(|i| {
            let name = format!("p{i}");
            PlanRelease {
                package: name.clone(),
                ecosystem: "cargo".to_owned(),
                from: "0.1.0".to_owned(),
                to: "0.2.0".to_owned(),
                bump: halyard_core::plan::Bump::Minor,
                tag: format!("{name}-v0.2.0"),
                reasons: Vec::new(),
                changelog: None,
                publish: Some(PlanPublish {
                    registry: "crates.io".to_owned(),
                    enabled: true,
                    access: None,
                    provenance: None,
                }),
            }
        })
        .collect::<Vec<_>>();
    let order = (0..40).map(|i| format!("p{i}")).collect::<Vec<_>>();
    let mut plan = Plan {
        schema: PLAN_SCHEMA_V1.to_owned(),
        base_branch: "main".to_owned(),
        head: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
        plan_hash: String::new(),
        releases,
        edits: Vec::new(),
        order,
        skipped: Vec::new(),
    };
    plan.plan_hash = plan_hash(&plan);
    plan
}

fn bench_publish_engine(c: &mut Criterion) {
    c.bench_function("publish_plan_layering_40", |b| {
        let ws = ws_40();
        let plan = plan_40();
        b.iter(|| plan_publication(&plan, &ws));
    });

    c.bench_function("publish_engine_pass_40", |b| {
        b.iter_batched(
            || {
                let ws = ws_40();
                let plan = plan_40();
                let publication =
                    plan_publication(&plan, &ws).unwrap_or_else(|e| unreachable!("plans: {e}"));
                // The coupled fake: uploads stage into the registry with
                // the one-probe index lag, so the bench exercises the
                // full resume-probe + visibility-wait path per target.
                let registry = MemoryRegistry::new();
                let effect = MemoryPublishEffect::new();
                effect.publishes_into(&registry);
                let router = MapRouter::new().with("crates.io", Arc::new(registry));
                (publication, router, effect)
            },
            |(publication, router, effect)| {
                let policy = PublishPolicy::new(
                    VisibilityPolicy {
                        max_polls: 3,
                        base_delay: Duration::from_millis(1),
                        max_delay: Duration::from_millis(4),
                    },
                    1,
                );
                run_publication(&publication, &router, &effect, &policy, &NoSleep)
                    .unwrap_or_else(|e| unreachable!("run: {e}"))
            },
            BatchSize::SmallInput,
        );
    });
}

/// The bench sleeper: no wall clock at all (I3 — the bench measures the
/// engine, never the backoff).
struct NoSleep;

impl halyard_registry::RegistrySleeper for NoSleep {
    fn sleep(&self, _delay: Duration) {}
}

fn main() {
    let mut criterion = Criterion::default();
    bench_publish_engine(&mut criterion);
}
