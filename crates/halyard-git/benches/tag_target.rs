//! Tag-target resolution benchmark (§11.1 precision rule): the two
//! steady-state shapes the release forecast exercises — a pinpoint hit
//! (the introducer sits `PIN_DEPTH` commits below HEAD; the walk reads
//! two trees per examined commit and one manifest blob per change event)
//! and the fallback miss (the version was never committed; the walk
//! exhausts the whole window before degrading to HEAD).

use std::path::Path;
use std::sync::OnceLock;

use criterion::Criterion;
use halyard_git::tag_target::{
    GixTagTargetResolver, ManifestVersionProbe, TagTargetRequest, TagTargetResolver,
};
use halyard_test_support::fixture::{CommitSpec, GitFixture};

/// Commits between HEAD and the bump commit (and the window a miss walks).
const PIN_DEPTH: usize = 500;

/// The cargo probe the benches share: the minimal `version = "X"` scan
/// (the fixtures' manifests are single-table).
struct BenchProbe;

impl ManifestVersionProbe for BenchProbe {
    fn manifest_version(
        &self,
        _ecosystem: &str,
        _manifest_path: &str,
        manifest: &[u8],
    ) -> Option<String> {
        let text = std::str::from_utf8(manifest).ok()?;
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("version = \"") {
                let value = rest.strip_suffix('"')?;
                if !value.is_empty() {
                    return Some(value.to_owned());
                }
            }
            if line.starts_with('[') && !line.starts_with("[package]") {
                return None;
            }
        }
        None
    }
}

fn manifest(version: &str) -> String {
    format!("[package]\nname = \"app\"\nversion = \"{version}\"\n")
}

/// Builds (once) a repo whose manifest was bumped `PIN_DEPTH` commits
/// below HEAD (the bump is followed by source-only commits).
fn pinned_fixture() -> &'static GitFixture {
    static FIXTURE: OnceLock<GitFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let mut fx = GitFixture::new("bench-tag-target-pinned")
            .unwrap_or_else(|e| unreachable!("fixture dir: {e}"));
        fx.commit_with(&CommitSpec::files(
            vec![("Cargo.toml".to_owned(), manifest("0.1.0"))],
            "feat: scaffold app",
        ))
        .unwrap_or_else(|e| unreachable!("base commit: {e}"));
        fx.tag_annotated("v0.1.0", "app 0.1.0")
            .unwrap_or_else(|e| unreachable!("base tag: {e}"));
        fx.commit_with(&CommitSpec::files(
            vec![("Cargo.toml".to_owned(), manifest("0.2.0"))],
            "chore: release 0.2.0",
        ))
        .unwrap_or_else(|e| unreachable!("bump commit: {e}"));
        for i in 0..PIN_DEPTH {
            fx.commit_with(&CommitSpec::files(
                vec![(
                    "src/lib.rs".to_owned(),
                    format!("pub const N: usize = {i};\n"),
                )],
                "feat: grow app",
            ))
            .unwrap_or_else(|e| unreachable!("growth commit {i}: {e}"));
        }
        fx
    })
}

/// Builds (once) a repo whose manifest never carried the requested
/// version (the fallback miss walks the full window to the base).
fn missing_fixture() -> &'static GitFixture {
    static FIXTURE: OnceLock<GitFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let mut fx = GitFixture::new("bench-tag-target-missing")
            .unwrap_or_else(|e| unreachable!("fixture dir: {e}"));
        fx.commit_with(&CommitSpec::files(
            vec![("Cargo.toml".to_owned(), manifest("0.1.0"))],
            "feat: scaffold app",
        ))
        .unwrap_or_else(|e| unreachable!("base commit: {e}"));
        fx.tag_annotated("v0.1.0", "app 0.1.0")
            .unwrap_or_else(|e| unreachable!("base tag: {e}"));
        for i in 0..PIN_DEPTH {
            fx.commit_with(&CommitSpec::files(
                vec![(
                    "src/lib.rs".to_owned(),
                    format!("pub const N: usize = {i};\n"),
                )],
                "feat: grow app",
            ))
            .unwrap_or_else(|e| unreachable!("growth commit {i}: {e}"));
        }
        fx
    })
}

fn head_of(fx: &GitFixture) -> halyard_core::Oid {
    let head = fx
        .head_oid()
        .unwrap_or_else(|e| unreachable!("HEAD: {e}"))
        .unwrap_or_else(|| unreachable!("fixture without HEAD"));
    halyard_core::Oid::from_hex(head.as_bytes())
        .unwrap_or_else(|| unreachable!("fixture oids are valid hex"))
}

fn base_of(fx: &GitFixture) -> halyard_core::Oid {
    let base = fx
        .resolve("v0.1.0^{}")
        .unwrap_or_else(|e| unreachable!("base tag: {e}"));
    halyard_core::Oid::from_hex(base.trim().as_bytes())
        .unwrap_or_else(|| unreachable!("fixture oids are valid hex"))
}

fn bench_tag_target(c: &mut Criterion) {
    let mut group = c.benchmark_group("halyard-git");
    group.sample_size(30);
    group.warm_up_time(std::time::Duration::from_millis(300));
    group.measurement_time(std::time::Duration::from_secs(3));

    let pinned = pinned_fixture();
    let pinned_root: &Path = pinned.root();
    let resolver = GixTagTargetResolver::open(pinned_root, Box::new(BenchProbe))
        .unwrap_or_else(|e| unreachable!("resolver open: {e}"));
    let hit = TagTargetRequest {
        ecosystem: "cargo".to_owned(),
        manifest: "Cargo.toml".to_owned(),
        version: "0.2.0".to_owned(),
        head: head_of(pinned),
        base: Some(base_of(pinned)),
    };
    group.bench_function("tag_target_pinpoint_500", |b| {
        b.iter(|| {
            let target = resolver.resolve(&hit);
            debug_assert!(
                matches!(
                    &target,
                    Ok(t) if t.source == halyard_git::tag_target::TagTargetSource::ManifestPinpoint
                ),
                "the pinned bench must pinpoint, got {target:?}"
            );
        });
    });

    let missing = missing_fixture();
    let missing_root: &Path = missing.root();
    let resolver = GixTagTargetResolver::open(missing_root, Box::new(BenchProbe))
        .unwrap_or_else(|e| unreachable!("resolver open: {e}"));
    let miss = TagTargetRequest {
        ecosystem: "cargo".to_owned(),
        manifest: "Cargo.toml".to_owned(),
        version: "9.9.9".to_owned(),
        head: head_of(missing),
        base: Some(base_of(missing)),
    };
    group.bench_function("tag_target_fallback_500", |b| {
        b.iter(|| {
            let target = resolver.resolve(&miss);
            debug_assert!(
                matches!(
                    &target,
                    Ok(t) if t.source == halyard_git::tag_target::TagTargetSource::HeadFallback
                ),
                "the missing bench must fall back, got {target:?}"
            );
        });
    });

    group.finish();
}

fn main() {
    bench_tag_target(&mut Criterion::default().configure_from_args());
}
