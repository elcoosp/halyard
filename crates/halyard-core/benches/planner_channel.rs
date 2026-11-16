//! Prerelease-channel planning throughput (§10.3, ADR-0025).
//!
//! The channel work rides the pure planner (`channelize_targets` +
//! the materialize train rule): a base-tag parse, a version witness
//! list and the counter advance per releasing package. The bench pins
//! that cost against the same fixture planned without a channel, so the
//! §14.1 planning budgets stay untouched by the feature.

use std::hint::black_box;

use criterion::Criterion;
use halyard_core::history::{
    AuthorRef, BaseRef, FixedHistory, HistoryDigest, ParsedCommit, ReleaseRefs, RelevantCommit,
};
use halyard_core::package::{
    Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource, Workspace,
};
use halyard_core::planner::{PlanConfig, Snapshot, plan};
use halyard_core::{Oid, PkgId};

/// One conventional commit row (planner-test fixture shape).
fn commit(suffix: u8, kind: &str) -> RelevantCommit {
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
            breaking: false,
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
    Snapshot {
        workspace: leaked_ws,
        history,
        refs,
    }
}

/// Plans one release with (or without) the configured channel.
fn plan_once(snapshot: &Snapshot<'_>, channel: Option<&str>) {
    let mut cfg = PlanConfig::default_values();
    cfg.prerelease_channel = channel.and_then(halyard_core::bump::PrereleaseChannel::new);
    let _ = black_box(plan(snapshot, &cfg).ok());
}

fn bench_channels(c: &mut Criterion) {
    let mut group = c.benchmark_group("plan_channel");

    // Fresh train: stable base tag, stable manifest, one feat.
    let snap = fixture("1.2.3", Some("v1.2.3"), vec![commit(1, "feat")]);
    group.bench_function("fresh_train_feat", |b| {
        b.iter(|| plan_once(&snap, Some("rc")));
    });

    // Active train: prerelease base tag, prerelease manifest, one fix —
    // the absorbed-triple path plus the counter advance.
    let snap = fixture("1.3.0-rc.1", Some("v1.3.0-rc.1"), vec![commit(2, "fix")]);
    group.bench_function("active_train_counter", |b| {
        b.iter(|| plan_once(&snap, Some("rc")));
    });

    // The same fixture without a channel: the stable baseline the
    // channel overhead is measured against.
    let snap = fixture("1.3.0-rc.1", Some("v1.3.0-rc.1"), vec![commit(2, "fix")]);
    group.bench_function("stable_baseline", |b| {
        b.iter(|| plan_once(&snap, None));
    });

    group.finish();
}

/// Criterion entry point (hand-written instead of `criterion_group!` so the
/// generated function keeps its doc comment under `missing_docs`).
fn main() {
    let mut c = Criterion::default().configure_from_args();
    bench_channels(&mut c);
}
