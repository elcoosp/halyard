//! Title-lint throughput (§11.6, M4.3 `check`): one lint per `check`
//! invocation, so a single call must stay far below any perceptible
//! budget. The corpus covers the verdict shapes: mapped kinds, breaking,
//! unmapped, typo'd (the did-you-mean scan), non-conventional and
//! autosquash. The `commits_lint` group (M2, §16.2's commits half)
//! measures the same machinery over the pending history's own subjects —
//! one lint per (package, commit) row.

use std::hint::black_box;

use criterion::Criterion;
use halyard_conventional::{Level, TitleBumpRule, lint_title};

/// The §10.1 default mapping as a bench rule (same shape the CLI's
/// `PlanConfig` adapter uses).
struct Defaults;

impl TitleBumpRule for Defaults {
    fn bump_for_kind(&self, kind: &str) -> Level {
        match kind {
            "feat" => Level::Minor,
            "fix" | "perf" => Level::Patch,
            _ => Level::None,
        }
    }

    fn known_kinds(&self) -> Vec<String> {
        [
            "feat", "fix", "perf", "chore", "docs", "test", "refactor", "ci", "style", "revert",
        ]
        .iter()
        .map(|k| (*k).to_owned())
        .collect()
    }
}

/// The verdict-shape corpus.
const TITLES: &[&str] = &[
    "feat(parser): accept spans",
    "feat(api)!: send an email to the customer when a product ships",
    "fix: correct minor typo (#412)",
    "perf(walk): paint with generation numbers",
    "chore: tidy the bench fixtures",
    "feet: adjust the parser fixtures",
    "just a plain PR title without the conventional grammar",
    "fixup! feat: something earlier",
];

fn bench_title_lint(c: &mut Criterion) {
    let mut group = c.benchmark_group("conventional_title_lint");
    group.bench_function("corpus_of_8", |b| {
        b.iter(|| {
            let accepted = TITLES
                .iter()
                .filter(|t| {
                    matches!(
                        lint_title(black_box(t), &Defaults),
                        halyard_conventional::TitleVerdict::Accepted(_)
                    )
                })
                .count();
            black_box(accepted)
        });
    });
    group.finish();
}

/// Criterion entry point (a hand-written `main` instead of the macros:
/// the generated function keeps its doc comment under `missing_docs`).
/// The commits lint (M2, §16.2's commits half): the same machinery over
/// the pending history's own subjects — one lint per (package, commit)
/// row, so a 200-commit pending history must stay trivially cheap.
fn bench_commits_lint(c: &mut Criterion) {
    let mut group = c.benchmark_group("commits_lint");
    let subjects: Vec<String> = (0..200)
        .map(|index| match index % 5 {
            0 => format!("feat(core): accept spans {index}"),
            1 => format!("fix(parser): handle crlf {index}"),
            2 => format!("docs: note the grammar {index}"),
            3 => format!("chore: tidy the bench {index}"),
            _ => format!("feet: a near-miss kind {index}"),
        })
        .collect();
    group.bench_function("subjects_200", |b| {
        b.iter(|| {
            for subject in &subjects {
                let _linted = lint_title(black_box(subject), &Defaults);
            }
        });
    });
    group.finish();
}

fn main() {
    let mut c = Criterion::default().configure_from_args();
    bench_title_lint(&mut c);
    bench_commits_lint(&mut c);
}
