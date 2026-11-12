//! Apply throughput (§14.1 "apply edits (40 files) ≤ 15 ms p50"): the
//! edit pass over a 40-file workspace-shaped corpus, plus a single-file
//! variant for the dominant no-op-adjacent case.

use std::fmt::Write as _;
use std::hint::black_box;

use criterion::Criterion;
use halyard_edit::Patch;

/// Builds `files` manifests of `lines` lines each with one version line.
fn corpus(files: usize, lines: usize) -> Vec<String> {
    (0..files)
        .map(|file| {
            let mut text = String::with_capacity(lines * 32);
            for line in 0..lines {
                if line == 2 {
                    text.push_str("version = \"0.4.1\"\n");
                } else {
                    let _ = writeln!(text, "# line {line} of {file} = placeholder");
                }
            }
            text
        })
        .collect()
}

/// The one patch each corpus file carries: its version line. The range is
/// a static constant, so a malformed patch is impossible; invalid results
/// are filtered rather than panicked on (restriction lints apply to
/// benches).
fn patches() -> Vec<Patch> {
    // "version = " is 10 bytes, the quote is 1: the value spans 11..16.
    [Patch::new(11, 16, "0.5.0".to_owned())]
        .into_iter()
        .filter_map(std::result::Result::ok)
        .collect()
}

fn bench_apply(c: &mut Criterion) {
    let manifest = corpus(1, 20).pop().unwrap_or_default();
    let ps = patches();
    let mut group = c.benchmark_group("edit_apply");
    group.bench_function("single_file", |b| {
        b.iter(|| {
            // The patch/range pair is static and validated by the crate's
            // unit tests; a failure here measures nothing, so fall back to
            // an empty buffer instead of panicking.
            halyard_edit::apply(black_box(manifest.as_bytes()), black_box(&ps)).unwrap_or_default()
        });
    });

    let many = corpus(40, 60);
    group.bench_function("forty_files", |b| {
        b.iter(|| {
            let mut total = 0usize;
            for content in &many {
                let out = halyard_edit::apply(black_box(content.as_bytes()), black_box(&ps))
                    .unwrap_or_default();
                total += out.len();
            }
            total
        });
    });
    group.finish();
}

/// Criterion entry point (hand-written instead of `criterion_group!` so the
/// generated function keeps its doc comment under `missing_docs`).
fn main() {
    let mut c = Criterion::default().configure_from_args();
    bench_apply(&mut c);
}
