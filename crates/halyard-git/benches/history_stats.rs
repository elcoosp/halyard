//! The walk-provenance baseline (the M2.6 stats wiring): the engine's
//! own counters — `tree_diff_stats`/`merge_stats` and the degraded /
//! truncated markers — as produced by one fixture walk. The shell
//! renders exactly these numbers ([`render`]-side); this bench pins
//! their production cost (they ride the walk itself, never a second
//! pass).
//!
//! Run: `cargo bench -p halyard-git`

use criterion::{BatchSize, Criterion};
use halyard_core::Workspace;
use halyard_git::{GitHistory, GitHistoryOptions};
use halyard_test_support::{CommitSpec, GitFixture};

/// A small fixture workspace: one member, three commits (scaffold, tag,
/// extend) — the shape a solo-package run walks.
fn build_fixture(label: &str) -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("halyard-git-stats-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|e| unreachable!("the temp fixture dir builds: {e}"));
    let mut fx =
        GitFixture::init_in(&dir).unwrap_or_else(|e| unreachable!("the fixture repo inits: {e}"));
    fx.commit_with(&CommitSpec::files(
        vec![
            (
                "Cargo.toml".to_owned(),
                "[package]\nname = \"solo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n".to_owned(),
            ),
            (
                "src/lib.rs".to_owned(),
                "pub const C: usize = 1;\n".to_owned(),
            ),
        ],
        "feat: scaffold the crate",
    ))
    .unwrap_or_else(|e| unreachable!("the scaffold commit lands: {e}"));
    fx.tag_annotated("v0.1.0", "solo v0.1.0")
        .unwrap_or_else(|e| unreachable!("the tag lands: {e}"));
    fx.commit_with(&CommitSpec::files(
        vec![(
            "src/lib.rs".to_owned(),
            "pub const C: usize = 2;\n".to_owned(),
        )],
        "feat: extend the crate",
    ))
    .unwrap_or_else(|e| unreachable!("the extend commit lands: {e}"));
    fx.root().to_path_buf()
}

fn workspace(root: &std::path::Path) -> Workspace {
    halyard_discover::discover_repo(root, &halyard_discover::DiscoverInput::default())
        .unwrap_or_else(|e| unreachable!("the fixture workspace discovers: {e}"))
        .workspace
}

/// The full walk with counters on the fixture (the cold path).
fn bench_walk_with_stats(c: &mut Criterion) {
    let root = build_fixture("cold");
    let ws = workspace(&root);
    c.bench_function("history_stats/walk_with_stats/3-commits", |b| {
        b.iter_batched(
            || (),
            |()| {
                GitHistory::build_with_options(&root, &ws, GitHistoryOptions::default())
                    .unwrap_or_else(|e| unreachable!("the fixture walk succeeds: {e}"));
            },
            BatchSize::SmallInput,
        );
    });
}

/// The counter read the shell renders (post-walk: struct field reads —
/// the point is that no second measurement pass exists).
fn bench_stats_read(c: &mut Criterion) {
    let root = build_fixture("read");
    let ws = workspace(&root);
    let history = GitHistory::build_with_options(&root, &ws, GitHistoryOptions::default())
        .unwrap_or_else(|e| unreachable!("the fixture walk succeeds: {e}"));
    c.bench_function("history_stats/stats_read", |b| {
        b.iter(|| {
            let diff = &history.tree_diff_stats;
            std::hint::black_box((
                diff.trees_read,
                diff.entries_read,
                diff.memo_hits,
                diff.bloom_skips,
                diff.bloom_unavailable,
                history.merge_stats.merges_dropped,
                history.merge_stats.merges_kept,
                history.degraded,
                history.truncated,
            ))
        })
    });
}

fn main() {
    let mut criterion = Criterion::default();
    bench_walk_with_stats(&mut criterion);
    bench_stats_read(&mut criterion);
    criterion.final_summary();
}
