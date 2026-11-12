//! Parser throughput.
//!
//! Conventional parsing runs once per commit in every `plan` (§14.1: a
//! plan over 200 commits must stay within 25 ms p50), so the bench covers
//! the three shapes the walk emits: plain subjects, breaking subjects and
//! body+footer messages.

use std::hint::black_box;

use criterion::Criterion;
use halyard_conventional::parse;

/// The subject corpus: plain, scoped, breaking, PR-suffixed, trailers,
/// multi-line breaking, non-conventional, fixup.
const SUBJECTS: &[&[u8]] = &[
    b"feat: allow provided config to extend other configs",
    b"feat(api)!: send an email to the customer when a product is shipped",
    b"fix(parser): handle refs (#412)",
    b"chore: drop support for Node 6\n\nBREAKING CHANGE: drop support for Node 6.",
    b"fix: correct minor typo\n\nHappy path now is correct\n\nReviewed-by: Z\nRefs: #133",
    b"just a plain merge commit subject without colons",
    b"fixup! feat: something earlier",
];

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("conventional_parse");
    group.bench_function("corpus_of_7", |b| {
        b.iter(|| SUBJECTS.iter().filter_map(|s| parse(black_box(s))).count());
    });
    group.finish();
}

/// Criterion entry point (hand-written instead of `criterion_group!` so the
/// generated function keeps its doc comment under `missing_docs`).
fn main() {
    let mut c = Criterion::default().configure_from_args();
    bench_parse(&mut c);
}
