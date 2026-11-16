//! Bench: the `halyard init` scaffold planner and its write step
//! (§16.1/§16.2, ADR-0029).
//!
//! `init` runs once per invocation — millisecond scale at worst — but
//! the draft's strict-loader validation is the part the gate must keep
//! honest: the skeleton must stay a load-able v1 document, and the plan
//! must stay a trivial scan over the survey (the shell's discovery is
//! the expensive half, and it stays outside this bench). The write
//! decision is pinned because it runs on the command path.

use criterion::Criterion;
use halyard_config::{InitSurvey, MemoryDraftSink, plan_init, run_init};

/// A single-package Cargo repo (§16.1's opening shape).
fn single_cargo() -> InitSurvey {
    InitSurvey {
        packages: 1,
        ecosystems: vec!["cargo".to_owned()],
        single_package: true,
    }
}

/// A 40-crate workspace (the same shape the planner benches use).
fn workspace_40() -> InitSurvey {
    InitSurvey {
        packages: 40,
        ecosystems: std::iter::repeat_n("cargo".to_owned(), 40).collect(),
        single_package: false,
    }
}

fn bench_plan_init(c: &mut Criterion) {
    let single = single_cargo();
    let mixed = workspace_40();
    let mut group = c.benchmark_group("init");
    group.throughput(criterion::Throughput::Elements(1));
    group.bench_function("plan_single_package", |b| {
        b.iter(|| {
            let plan = plan_init(&single, false);
            assert!(
                matches!(&plan, Ok(plan) if plan.files.len() == 1),
                "the single-package skeleton always plans: {plan:?}"
            );
        });
    });
    group.bench_function("plan_workspace_40", |b| {
        b.iter(|| {
            let plan = plan_init(&mixed, true);
            assert!(
                matches!(&plan, Ok(plan) if plan.files.len() == 2),
                "the workspace scaffold plans both files: {plan:?}"
            );
        });
    });
    group.bench_function("plan_and_run_github_over_memory_sink", |b| {
        b.iter(|| {
            let plan = plan_init(&single, true).unwrap_or_else(|error| {
                eprintln!("the single-package scaffold always plans: {error}");
                std::process::exit(1);
            });
            let report = run_init(plan, &MemoryDraftSink::new()).unwrap_or_else(|error| {
                eprintln!("a fresh sink never refuses: {error}");
                std::process::exit(1);
            });
            assert!(
                report.files.len() == 2 && report.files.iter().all(|file| file.bytes > 0),
                "both scaffold files land: {report:?}"
            );
        });
    });
}

fn main() {
    let mut criterion = Criterion::default();
    bench_plan_init(&mut criterion);
}
