//! `dhat` allocation audit for the painter walk (M2.2, §14.3): allocations
//! during `GitHistory::build` on a 100-commit fixture, reported per
//! popped commit. The bound below pins the audited ceiling (actual number
//! recorded in the PR body); regressions beyond it fail the suite.
//!
//! This binary deliberately runs with the dhat global allocator — it is a
//! dedicated integration test, so the allocator swap affects nothing
//! else.

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

use std::sync::OnceLock;

use halyard_core::package::{
    Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource, Workspace,
};
use halyard_core::PkgId;
use halyard_git::GitHistory;
use halyard_test_support::fixture::{CommitSpec, GitFixture};

/// Audited per-commit ceilings (§14.3 tracks the metric; M2.6 owns the
/// drive toward ≤ 1). Pinned so regressions fail; the measured values are
/// recorded in the PR body. The commit-graph path is the §14.1 shape; the
/// no-graph (degraded) path pays one ODB read per commit.
const MAX_ALLOCATIONS_PER_COMMIT_NO_GRAPH: u64 = 14;
const MAX_ALLOCATIONS_PER_COMMIT_GRAPH: u64 = 8;

const COMMITS: usize = 100;

fn fixture(with_graph: bool) -> &'static GitFixture {
    static NO_GRAPH: OnceLock<GitFixture> = OnceLock::new();
    static WITH_GRAPH: OnceLock<GitFixture> = OnceLock::new();
    let cell = if with_graph { &WITH_GRAPH } else { &NO_GRAPH };
    cell.get_or_init(|| {
        let mut fx = GitFixture::new(if with_graph { "dhat-walk-graph" } else { "dhat-walk" })
            .expect("fixture dir");
        fx.commit_with(&CommitSpec::files(
            vec![(
                "Cargo.toml".to_owned(),
                "[package]\nname = \"app\"\nversion = \"0.1.0\"\n".to_owned(),
            )],
            "feat: base",
        ))
        .expect("base commit");
        fx.tag_light("v0.1.0").expect("base tag");
        for i in 0..COMMITS {
            fx.commit_with(&CommitSpec::files(
                vec![("src/lib.rs".to_owned(), format!("pub const V{i}: u32 = {i};\n"))],
                "feat: extend",
            ))
            .expect("commit");
        }
        if with_graph {
            fx.commit_graph_write(true).expect("commit-graph");
        }
        fx
    })
}

fn single_pkg_workspace() -> Workspace {
    let mut ws = Workspace::new();
    let pkg = Package {
        id: PkgId::FIRST,
        name: ws.intern("app"),
        ecosystem: Ecosystem::Cargo,
        root: RelPath::new("").unwrap(),
        manifest: RelPath::new("Cargo.toml").unwrap(),
        version: Some(semver::Version::new(0, 1, 0)),
        version_source: VersionSource::Local,
        publishable: true,
        deps: Vec::new(),
        tag_template: TagTemplate::parse("v{version}").unwrap(),
        changelog: None,
        version_site: None,
        include: PathFilter::new(Vec::new(), Vec::new()),
        group: GroupId::new(0),
        facets: halyard_core::Facets::default(),
    };
    ws.push_package(pkg);
    ws
}

#[test]
fn walk_allocation_budget() {
    // The profiler must outlive every stats query.
    let _profiler = dhat::Profiler::builder().testing().build();
    let ws = single_pkg_workspace();
    let degraded = audit(&ws, false, MAX_ALLOCATIONS_PER_COMMIT_NO_GRAPH);
    let with_graph = audit(&ws, true, MAX_ALLOCATIONS_PER_COMMIT_GRAPH);
    eprintln!("dhat per-commit: no-graph={degraded}, commit-graph={with_graph}");
    assert!(with_graph <= degraded, "the commit-graph path must not be slower on allocs");
}

/// Measures allocations for one `paint` over `fixture(with_graph)`.
fn audit(ws: &Workspace, with_graph: bool, ceiling: u64) -> u64 {
    let fx = fixture(with_graph);
    // Warm every lazy pool outside the measured window: gix caches, the
    // ODB pack index, the ref snapshot and the generation cache all come
    // from `GitHistory::build`; the audited window is the WALK itself
    // (§14.3: allocations per popped commit in the painter walk).
    let warm = GitHistory::build(fx.root(), ws, 10_000).expect("warm-up");

    // Re-open the walk inputs (bases + head) so the measured section is
    // exactly one `paint` over the same graph.
    let repo = halyard_git::gix::open(fx.root()).expect("repo");
    let head = repo.head_id().expect("head").detach();
    let base_oids: Vec<Option<gix::hash::ObjectId>> = warm
        .bases()
        .iter()
        .map(|b| {
            b.as_ref()
                .and_then(|base| gix::hash::ObjectId::from_hex(base.oid.as_bytes()).ok())
        })
        .collect();

    let before = dhat::HeapStats::get();
    let mut generations = halyard_git::refs::Generations::new(&repo);
    let painted = halyard_git::walk::paint(&repo, head, &base_oids, 10_000, &mut generations)
        .expect("paint");
    let after = dhat::HeapStats::get();

    let blocks = after.total_blocks - before.total_blocks;
    let popped = painted.commits.len().max(1) as u64;
    let per_commit = blocks / popped;
    assert_eq!(painted.commits.len(), COMMITS);
    assert!(
        per_commit <= ceiling,
        "walk allocated {blocks} blocks for {COMMITS} popped commits \
         ({per_commit}/commit; ceiling {ceiling})"
    );
    per_commit
}
