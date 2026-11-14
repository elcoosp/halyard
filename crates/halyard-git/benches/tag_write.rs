//! Tag-phase write-path benchmark (§11.1 step 2): the two steady-state
//! shapes the release shell exercises — a fresh tag creation (one object
//! write + one create-only ref update) and the idempotent re-run over an
//! already-tagged set (the CI no-op path, I1: validation + ref probe +
//! peel per directive).

use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use criterion::Criterion;
use halyard_git::tag::{
    GixTagWriter, PreviewTagWriter, TagDirective, TagWriter as _, TaggerIdentity, run_tag_phase,
};
use halyard_test_support::fixture::{CommitSpec, GitFixture};

/// Upper bound on distinct bench tags. The create bench rotates through
/// these names; with the criterion budget below the pool is never
/// exhausted mid-run (an exhausted pool would silently degrade into the
/// already-tagged shape and misreport the create cost).
const TAG_POOL: usize = 100_000;
/// Directives per idempotent re-run iteration.
const ALREADY_SET: usize = 64;

/// Builds (once) a deterministic one-commit repo.
fn fixture() -> &'static GitFixture {
    static FIXTURE: OnceLock<GitFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let mut fx =
            GitFixture::new("bench-tag-write").unwrap_or_else(|e| unreachable!("fixture dir: {e}"));
        fx.commit_with(&CommitSpec::files(
            vec![(
                "Cargo.toml".to_owned(),
                "[package]\nname = \"app\"\nversion = \"0.1.0\"\n".to_owned(),
            )],
            "feat: base",
        ))
        .unwrap_or_else(|e| unreachable!("base commit: {e}"));
        fx
    })
}

fn identity() -> TaggerIdentity {
    TaggerIdentity::new("halyard", "release@halyard.test", 1_700_000_000)
}

fn bench_tag_write(c: &mut Criterion) {
    let fx = fixture();
    let root: &Path = fx.root();
    let head = fx
        .resolve("HEAD")
        .unwrap_or_else(|e| unreachable!("HEAD: {e}"));
    let target = halyard_git::gix::hash::ObjectId::from_hex(head.as_bytes())
        .unwrap_or_else(|e| unreachable!("fixture oid: {e}"));
    let writer =
        GixTagWriter::open(root, identity()).unwrap_or_else(|e| unreachable!("writer open: {e}"));

    let mut group = c.benchmark_group("halyard-git");
    group.sample_size(30);
    group.warm_up_time(std::time::Duration::from_millis(300));
    group.measurement_time(std::time::Duration::from_secs(2));

    // Fresh creation: rotating name pool, bounded by TAG_POOL (see the
    // constant's doc).
    let next = AtomicUsize::new(0);
    group.bench_function("tag_write_create", |b| {
        b.iter(|| {
            let index = next.fetch_add(1, Ordering::Relaxed);
            let directive = TagDirective {
                name: format!("v0.0.{index}"),
                target,
                message: "app 0.1.0".to_owned(),
            };
            let outcome = writer
                .create_annotated_tag(&directive)
                .unwrap_or_else(|e| unreachable!("tag create: {e}"));
            std::hint::black_box(outcome)
        });
    });
    assert!(
        next.load(Ordering::Relaxed) < TAG_POOL,
        "the create bench exhausted its name pool ({} tags); raise TAG_POOL or shorten \
         the criterion budget so the measured shape stays 'fresh creation'",
        next.load(Ordering::Relaxed)
    );

    // Idempotent re-run (I1): ALREADY_SET pre-created tags, the phase
    // re-run reports AlreadyTagged for every directive (validation +
    // ref probe + peel per directive — the CI no-op path).
    let already: Vec<TagDirective> = (0..ALREADY_SET)
        .map(|i| TagDirective {
            name: format!("v0.0.pre.{i}"),
            target,
            message: "app 0.1.0".to_owned(),
        })
        .collect();
    {
        let writer = GixTagWriter::open(root, identity())
            .unwrap_or_else(|e| unreachable!("writer open: {e}"));
        let report = run_tag_phase(&writer, &already);
        assert_eq!(
            report.created, ALREADY_SET,
            "the setup pass must create the whole set (the bench measures the re-runs)"
        );
    }
    group.bench_function("tag_phase_already_64", |b| {
        b.iter(|| {
            let report = run_tag_phase(&writer, &already);
            std::hint::black_box(report.created)
        });
    });

    // The offline forecast (the `release --dry` path): probes are real
    // (ref reads against the already-tagged set), mutations are forecast
    // hashes — re-runnable forever, no writes.
    let preview =
        PreviewTagWriter::open(root).unwrap_or_else(|e| unreachable!("preview open: {e}"));
    group.bench_function("tag_phase_forecast_64", |b| {
        b.iter(|| {
            let report = run_tag_phase(&preview, &already);
            std::hint::black_box(report.already_tagged)
        });
    });
    group.finish();
}

/// Criterion harness entry (the `criterion_group!` expansion, spelled out
/// so the generated `pub fn` does not trip `missing_docs`).
fn benches() {
    let mut criterion: Criterion = Criterion::default().configure_from_args();
    bench_tag_write(&mut criterion);
}

fn main() {
    benches();
    Criterion::default().configure_from_args().final_summary();
}
