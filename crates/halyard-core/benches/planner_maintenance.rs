//! Maintenance-line planning throughput (§10.3, ADR-0027).
//!
//! The guard rides the pure planner as one final pass over the plan's
//! targets: a `MaintenanceLine::matches` check per releasing package
//! (the refusal path constructs the error once). The bench pins that
//! cost against the same fixture planned without a pin, so the §14.1
//! planning budgets stay untouched by the feature.

use std::hint::black_box;

use criterion::Criterion;
use halyard_core::change_file::ChangeFiles;
use halyard_core::history::{
    AuthorRef, BaseRef, FixedHistory, HistoryDigest, ParsedCommit, ReleaseRefs, RelevantCommit,
};
use halyard_core::package::{
    Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource, Workspace,
};
use halyard_core::planner::{MaintenanceLine, PlanConfig, Snapshot, plan};
use halyard_core::{Oid, PkgId};

/// One conventional commit row (planner-test fixture shape).
fn commit(suffix: u8, kind: &str, breaking: bool) -> RelevantCommit {
    let mut hex = [b'a'; 40];
    hex[39] = b'0' + (suffix % 10);
    let oid = Oid::from_hex(&hex).unwrap_or_else(|| unreachable!("fixture hex is valid"));
    RelevantCommit {
        oid,
        subject: format!("{kind}: fixture").into(),
        body_range: 0..0,
        parsed: ParsedCommit {
            kind: kind.into(),
            scope: None,
            breaking,
            description: "fixture".into(),
        },
        pr: None,
        author: AuthorRef {
            name: "Bench".into(),
            email: "bench@example.com".into(),
        },
        trailers: Vec::new().into(),
        touched: halyard_core::SmallBitset::new(),
    }
}

/// One package at `version` with a `v{version}` template and its
/// attributed commits; the snapshot is `'static` through deliberate
/// leaks (each bench fixture lives for the process).
fn fixture(
    version: &str,
    base_tag: Option<&str>,
    commits: Vec<RelevantCommit>,
) -> Snapshot<'static> {
    let mut ws = Workspace::new();
    let pkg = Package {
        id: PkgId::FIRST,
        name: ws.intern("app"),
        ecosystem: Ecosystem::Cargo,
        root: RelPath::new("crates/app").unwrap_or_else(|| unreachable!("fixture root is valid")),
        manifest: RelPath::new("crates/app/Cargo.toml")
            .unwrap_or_else(|| unreachable!("fixture manifest is valid")),
        version: Some(
            semver::Version::parse(version).unwrap_or_else(|_| unreachable!("fixture version")),
        ),
        version_source: VersionSource::Local,
        publishable: true,
        deps: Vec::new(),
        tag_template: TagTemplate::parse("v{version}")
            .unwrap_or_else(|e| unreachable!("fixture template is valid: {e}")),
        changelog: None,
        version_site: None,
        include: PathFilter::new(Vec::new(), Vec::new()),
        group: GroupId::new(0),
        facets: halyard_core::discover::Facets::default(),
    };
    ws.push_package(pkg);
    let per_pkg = vec![(0..commits.len()).collect::<Vec<_>>()];
    let base = base_tag.map(|tag| BaseRef {
        tag: tag.into(),
        oid: Oid::from_hex(&[b'b'; 40]).unwrap_or_else(|| unreachable!("fixture hex is valid")),
    });
    let digest = HistoryDigest {
        commits,
        per_pkg,
        bases: vec![base.clone()],
    };
    let head = Oid::from_hex(&[b'c'; 40]).unwrap_or_else(|| unreachable!("fixture hex is valid"));
    let history = Box::leak(Box::new(FixedHistory::new(Some(head), digest)));
    let refs = Box::leak(Box::new(ReleaseRefs::from_bases(vec![base])));
    let leaked_ws: &'static Workspace = Box::leak(Box::new(ws));
    let changes: &'static ChangeFiles = Box::leak(Box::new(ChangeFiles::new()));
    Snapshot {
        workspace: leaked_ws,
        history,
        refs,
        change_files: changes,
    }
}

/// Plans one release with (or without) the branch-resolved line.
fn plan_once(snapshot: &Snapshot<'_>, line: Option<&str>) {
    let mut cfg = PlanConfig::default_values();
    cfg.maintenance_line = line.and_then(MaintenanceLine::parse);
    let _ = black_box(plan(snapshot, &cfg).ok());
}

fn bench_maintenance(c: &mut Criterion) {
    let mut group = c.benchmark_group("plan_maintenance");

    // The in-line path: a backport `fix` on a `1.x` branch plans through
    // the guard unchanged.
    let snap = fixture("1.2.3", Some("v1.2.3"), vec![commit(1, "fix", false)]);
    group.bench_function("in_line_patch", |b| {
        b.iter(|| plan_once(&snap, Some("1.x")));
    });

    // The refusal path: a breaking commit would cross the line; the
    // guard constructs the `E-PLAN-MAINTENANCE` error.
    let snap = fixture("1.2.3", Some("v1.2.3"), vec![commit(2, "feat", true)]);
    group.bench_function("cross_line_major_refusal", |b| {
        b.iter(|| plan_once(&snap, Some("1.x")));
    });

    // The same fixtures without a pin: the stable baseline the guard
    // overhead is measured against.
    let snap = fixture("1.2.3", Some("v1.2.3"), vec![commit(1, "fix", false)]);
    group.bench_function("unpinned_baseline", |b| {
        b.iter(|| plan_once(&snap, None));
    });

    group.finish();
}

/// Criterion entry point (hand-written instead of `criterion_group!` so the
/// generated function keeps its doc comment under `missing_docs`).
fn main() {
    let mut c = Criterion::default().configure_from_args();
    bench_maintenance(&mut c);
}
