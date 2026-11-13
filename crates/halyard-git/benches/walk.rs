//! Painter-walk benchmark (M2.2): a synthetic 1 000-commit linear
//! history, built once via `GitFixture`, with base resolution + paint
//! measured together (the planner's per-run history path).

use std::path::Path;
use std::sync::OnceLock;

use criterion::Criterion;
use halyard_core::HistoryProvider;
use halyard_core::PkgId;
use halyard_core::package::{
    Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource, Workspace,
};
use halyard_git::GitHistory;
use halyard_test_support::fixture::{CommitSpec, GitFixture};

/// Builds (once) a deterministic 1 000-commit single-package repo.
fn fixture() -> &'static GitFixture {
    static FIXTURE: OnceLock<GitFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        // Bench fixture setup failures abort the bench loudly (bench fns
        // are not `#[test]`, so the expect/unwrap lint allowance does not
        // reach them).
        let mut fx =
            GitFixture::new("bench-walk").unwrap_or_else(|e| unreachable!("fixture dir: {e}"));
        fx.commit_with(&CommitSpec::files(
            vec![(
                "Cargo.toml".to_owned(),
                "[package]\nname = \"app\"\nversion = \"0.1.0\"\n".to_owned(),
            )],
            "feat: base",
        ))
        .unwrap_or_else(|e| unreachable!("base commit: {e}"));
        fx.tag_light("v0.1.0")
            .unwrap_or_else(|e| unreachable!("base tag: {e}"));
        for i in 0..1_000usize {
            fx.commit_with(&CommitSpec::files(
                vec![(
                    "src/lib.rs".to_owned(),
                    format!("pub const V{i}: u32 = {i};\n"),
                )],
                if i % 3 == 0 {
                    "feat: extend"
                } else if i % 3 == 1 {
                    "fix: mend"
                } else {
                    "perf: tighten"
                },
            ))
            .unwrap_or_else(|e| unreachable!("commit {i}: {e}"));
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
        root: RelPath::new("")
            .unwrap_or_else(|| unreachable!("empty root is always a valid RelPath")),
        manifest: RelPath::new("Cargo.toml")
            .unwrap_or_else(|| unreachable!("root manifest path is always a valid RelPath")),
        version: Some(semver::Version::new(0, 1, 0)),
        version_source: VersionSource::Local,
        publishable: true,
        deps: Vec::new(),
        tag_template: TagTemplate::parse("v{version}")
            .unwrap_or_else(|_| unreachable!("'v{{version}}' is always a valid template")),
        changelog: None,
        version_site: None,
        include: PathFilter::new(Vec::new(), Vec::new()),
        group: GroupId::new(0),
        facets: halyard_core::Facets::default(),
    };
    ws.push_package(pkg);
    ws
}

fn bench_history_engine(c: &mut Criterion) {
    let root: &Path = fixture().root();
    let ws = single_pkg_workspace();
    let mut group = c.benchmark_group("halyard-git");
    group.throughput(criterion::Throughput::Elements(1_000));
    group.bench_function("build_history_1000_commits", |b| {
        b.iter(|| {
            let history = GitHistory::build(root, &ws, 10_000)
                .unwrap_or_else(|e| unreachable!("history build: {e}"));
            std::hint::black_box(history.digest().commits.len())
        });
    });
    group.finish();
}

/// Criterion harness entry (the `criterion_group!` expansion, spelled out
/// so the generated `pub fn` does not trip `missing_docs`).
fn benches() {
    let mut criterion: Criterion = Criterion::default().configure_from_args();
    bench_history_engine(&mut criterion);
}

fn main() {
    benches();
    Criterion::default().configure_from_args().final_summary();
}
