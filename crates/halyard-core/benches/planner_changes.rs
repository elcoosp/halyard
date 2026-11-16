//! Change-file planning throughput (§9.4, ADR-0028).
//!
//! The explicit bump rides `direct_bumps` as one overwrite + reason
//! push per package with a change file, and the note merge appends the
//! note bodies to the changelog preview — both O(files) over the
//! planning cost. The bench pins that cost against the same fixture
//! planned without change files, so the §14.1 planning budgets stay
//! untouched by the feature.

use std::hint::black_box;

use criterion::Criterion;
use halyard_core::change_file::ChangeFiles;
use halyard_core::history::{
    AuthorRef, BaseRef, FixedHistory, HistoryDigest, ParsedCommit, ReleaseRefs, RelevantCommit,
};
use halyard_core::package::{
    Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource, Workspace,
};
use halyard_core::plan::Bump;
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
fn fixture(version: &str, commits: Vec<RelevantCommit>, changes: ChangeFiles) -> Snapshot<'static> {
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
    let base = BaseRef {
        tag: format!("v{version}").into(),
        oid: Oid::from_hex(&[b'b'; 40]).unwrap_or_else(|| unreachable!("fixture hex is valid")),
    };
    let digest = HistoryDigest {
        commits,
        per_pkg,
        bases: vec![Some(base.clone())],
    };
    let head = Oid::from_hex(&[b'c'; 40]).unwrap_or_else(|| unreachable!("fixture hex is valid"));
    let history = Box::leak(Box::new(FixedHistory::new(Some(head), digest)));
    let refs = Box::leak(Box::new(ReleaseRefs::from_bases(vec![Some(base)])));
    let leaked_ws: &'static Workspace = Box::leak(Box::new(ws));
    let leaked_changes: &'static ChangeFiles = Box::leak(Box::new(changes));
    Snapshot {
        workspace: leaked_ws,
        history,
        refs,
        change_files: leaked_changes,
    }
}

/// One recorded change file: an explicit bump and a note body of
/// `note_lines` bullet lines (0 = bump-only file).
fn change_file(label: &str, bump: Bump, note_lines: usize) -> ChangeFiles {
    let mut changes = ChangeFiles::new();
    let note = "- upgrade step\n".repeat(note_lines);
    changes.record(
        &format!(".halyard/changes/{label}.md"),
        [("app".to_owned(), bump)],
        &note,
    );
    changes
}

/// Plans one release with the default config.
fn plan_once(snapshot: &Snapshot<'_>) {
    let cfg = PlanConfig::default_values();
    let _ = black_box(plan(snapshot, &cfg).ok());
}

fn bench_changes(c: &mut Criterion) {
    let mut group = c.benchmark_group("plan_changes");

    // The baseline: the same commits planned with no change files —
    // the feature's cost is measured against exactly this shape.
    let commits = vec![commit(1, "feat"), commit(2, "fix")];
    let snap = fixture("1.2.3", commits.clone(), ChangeFiles::new());
    group.bench_function("no_changes_baseline", |b| b.iter(|| plan_once(&snap)));

    // The bump path: one explicit bump replacing the commit-derived
    // level (overwrite + one reason push).
    let changes = change_file("bump", Bump::Minor, 0);
    let snap = fixture("1.2.3", commits.clone(), changes);
    group.bench_function("explicit_bump", |b| b.iter(|| plan_once(&snap)));

    // The merge path: the note bodies appended to the preview (a
    // realistic upgrade note, ~10 lines).
    let changes = change_file("noted", Bump::Minor, 10);
    let snap = fixture("1.2.3", commits, changes);
    group.bench_function("note_merge", |b| b.iter(|| plan_once(&snap)));

    group.finish();
}

/// Criterion entry point (hand-written instead of `criterion_group!` so the
/// generated function keeps its doc comment under `missing_docs`).
fn main() {
    let mut c = Criterion::default().configure_from_args();
    bench_changes(&mut c);
}
