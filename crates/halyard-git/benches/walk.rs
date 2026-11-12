//! Painter-walk benchmark (M2.2): a synthetic 1 000-commit linear
//! history, built once via `GitFixture`, with base resolution + paint
//! measured together (the planner's per-run history path).

use std::path::Path;
use std::sync::OnceLock;

use criterion::{criterion_group, criterion_main, Criterion};
use halyard_core::package::{
    Ecosystem, GroupId, Package, PathFilter, RelPath, TagTemplate, VersionSource, Workspace,
};
use halyard_core::PkgId;
use halyard_git::GitHistory;
use halyard_test_support::fixture::{CommitSpec, GitFixture};

/// Builds (once) a deterministic 1 000-commit single-package repo.
fn fixture() -> &'static GitFixture {
    static FIXTURE: OnceLock<GitFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let mut fx = GitFixture::new("bench-walk").expect("fixture dir");
        fx.commit_with(&CommitSpec::files(
            vec![(
                "Cargo.toml".to_owned(),
                "[package]\nname = \"app\"\nversion = \"0.1.0\"\n".to_owned(),
            )],
            "feat: base",
        ))
        .expect("base commit");
        fx.tag_light("v0.1.0").expect("base tag");
        for i in 0..1_000usize {
            fx.commit_with(&CommitSpec::files(
                vec![("src/lib.rs".to_owned(), format!("pub const V{i}: u32 = {i};\n"))],
                if i % 3 == 0 {
                    "feat: extend"
                } else if i % 3 == 1 {
                    "fix: mend"
                } else {
                    "perf: tighten"
                },
            ))
            .expect("commit");
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

fn bench_history_engine(c: &mut Criterion) {
    let root: &Path = fixture().root();
    let ws = single_pkg_workspace();
    let mut group = c.benchmark_group("halyard-git");
    group.throughput(criterion::Throughput::Elements(1_000));
    group.bench_function("build_history_1000_commits", |b| {
        b.iter(|| {
            let history = GitHistory::build(root, &ws, 10_000).expect("history");
            std::hint::black_box(history.digest().commits.len())
        });
    });
    group.finish();
}

criterion_group!(benches, bench_history_engine);
criterion_main!(benches);
