//! Dry-run machine pass shapes (M4.2b step 1): `halyard pr --dry` runs
//! exactly one of these passes per invocation — the prepare-phase open
//! forecast, the empty-plan close probe, and the tag-phase release
//! forecast — so each shape's overhead is pinned here.

use std::hint::black_box;

use criterion::Criterion;
use halyard_forge::{
    BranchName, MachineInput, PrSpec, PreviewForge, ReleaseSpec, run_release_machine,
};

/// The release branch shared by the shapes.
fn branch() -> BranchName {
    BranchName::new("halyard/release/main")
        .unwrap_or_else(|e| unreachable!("branch fixture must be valid: {e}"))
}

/// Builds a prepare-phase input shaped like the §11.3 body the CLI
/// renders (a one-release plan).
fn open_input() -> MachineInput {
    MachineInput {
        branch: branch(),
        pending_releases: Vec::new(),
        release_pr: Some(PrSpec {
            branch: branch(),
            title: "chore: release p1-v0.1.1".to_owned(),
            body: "## 🚀 Release plan\n\n| Package | Current | Next | Bump | Why |\n|---|---|---|---|---|\n| p1 | 0.1.0 | **0.1.1** | patch | `fix: …` |\n".to_owned(),
        }),
    }
}

/// The empty-plan input (close probe path).
fn close_probe_input() -> MachineInput {
    MachineInput {
        branch: branch(),
        pending_releases: Vec::new(),
        release_pr: None,
    }
}

/// The tag-phase input (PROPAGATE path, §11.1 step 2).
fn tag_phase_input() -> MachineInput {
    MachineInput {
        branch: branch(),
        pending_releases: vec![ReleaseSpec {
            tag: "p1-v0.1.1".to_owned(),
            notes: "## Features\n\n- fix the thing\n".to_owned(),
        }],
        release_pr: None,
    }
}

fn bench_pr_preview(c: &mut Criterion) {
    let mut group = c.benchmark_group("forge_pr_preview");
    group.bench_function("prepare_open_pass", |b| {
        b.iter(|| {
            let report = run_release_machine(&PreviewForge::new(), black_box(&open_input()))
                .unwrap_or_else(|e| unreachable!("preview passes never fail: {e}"));
            report.effects.len()
        });
    });
    group.bench_function("empty_plan_probe_pass", |b| {
        b.iter(|| {
            let report = run_release_machine(&PreviewForge::new(), black_box(&close_probe_input()))
                .unwrap_or_else(|e| unreachable!("preview passes never fail: {e}"));
            report.calls
        });
    });
    group.bench_function("tag_phase_pass", |b| {
        b.iter(|| {
            let report = run_release_machine(&PreviewForge::new(), black_box(&tag_phase_input()))
                .unwrap_or_else(|e| unreachable!("preview passes never fail: {e}"));
            report.effects.len()
        });
    });
    group.finish();
}

/// Criterion entry point (a hand-written `main` instead of the macros:
/// the generated function keeps its doc comment under `missing_docs`).
fn main() {
    let mut c = Criterion::default().configure_from_args();
    bench_pr_preview(&mut c);
}
