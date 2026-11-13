//! The state-machine pass baseline (M4.2a): the in-memory cost of one
//! §11.1 machine pass per scenario — tag-phase creation, the idempotent
//! re-run, the §11.5 no-op pass (find only), the ≤2-call update path and
//! the empty-plan close probe. The forge budget is call-count based
//! (§11.5), so these numbers document the seam's in-memory overhead; a
//! REST adapter inherits the same pass shapes over real HTTP.
//!
//! Run: `cargo bench -p halyard-forge --bench state_machine`

use std::fmt::Write as _;

use criterion::Criterion;
use halyard_forge::{
    BranchName, Forge, MachineInput, MemoryForge, PrSpec, ReleaseSpec, run_release_machine,
};

/// Branch fixture shared by the bench groups.
fn main_branch() -> BranchName {
    BranchName::new("halyard/release/main").unwrap_or_else(|e| unreachable!("valid branch: {e}"))
}

/// Two pending-release fixtures (the tag phase's input).
fn two_pending_releases() -> Vec<ReleaseSpec> {
    vec![
        ReleaseSpec {
            tag: "core-v0.5.0".to_owned(),
            notes: "## Features\n\n- the painter walk".to_owned(),
        },
        ReleaseSpec {
            tag: "cli-v0.3.0".to_owned(),
            notes: "## Bug Fixes\n\n- apply exit codes".to_owned(),
        },
    ]
}

/// A Release-PR spec fixture with a plausible body size (~1 KiB).
fn pr_spec(body: &str) -> PrSpec {
    let mut full = String::with_capacity(1024);
    full.push_str("## 🚀 Release plan\n\n| Package | Current | Next | Bump | Why |\n");
    for i in 0..12 {
        let _ = writeln!(full, "| `pkg-{i}` | 0.1.{i} | 0.2.0 | minor | `feat: …` |");
    }
    full.push_str(body);
    PrSpec {
        branch: main_branch(),
        title: "chore: release v0.2.0".to_owned(),
        body: full,
    }
}

/// A forge with the Release PR already open at `body` (the prepare
/// phase's steady state).
fn forge_with_open_pr(body: &str) -> MemoryForge {
    let forge = MemoryForge::new();
    let _ = forge.upsert_pr(&pr_spec(body));
    forge
}

/// Tag phase on a fresh forge: two `create_release` calls, then PROPAGATE.
fn bench_tag_phase_create(c: &mut Criterion) {
    c.bench_function("machine/tag_phase_create_two", |b| {
        b.iter(|| {
            let forge = MemoryForge::new();
            let input = MachineInput {
                branch: main_branch(),
                pending_releases: two_pending_releases(),
                release_pr: None,
            };
            let _ = run_release_machine(&forge, &input);
        });
    });
}

/// Tag-phase re-run: both releases already exist (GET-by-tag-first, I1).
fn bench_tag_phase_rerun(c: &mut Criterion) {
    c.bench_function("machine/tag_phase_rerun_existing", |b| {
        let forge = MemoryForge::new();
        let input = MachineInput {
            branch: main_branch(),
            pending_releases: two_pending_releases(),
            release_pr: None,
        };
        let _ = run_release_machine(&forge, &input);
        b.iter(|| {
            let _ = run_release_machine(&forge, &input);
        });
    });
}

/// The §11.5 no-op pass: the PR already carries this exact body — find
/// only, zero mutating calls.
fn bench_prepare_noop(c: &mut Criterion) {
    c.bench_function("machine/prepare_noop_find_only", |b| {
        let forge = forge_with_open_pr("");
        let input = MachineInput {
            branch: main_branch(),
            pending_releases: Vec::new(),
            release_pr: Some(pr_spec("")),
        };
        b.iter(|| {
            let _ = run_release_machine(&forge, &input);
        });
    });
}

/// The update path: a changed body — find + upsert (the ≤2-call budget).
fn bench_prepare_update(c: &mut Criterion) {
    c.bench_function("machine/prepare_update_find_upsert", |b| {
        let forge = forge_with_open_pr("-stale");
        let input = MachineInput {
            branch: main_branch(),
            pending_releases: Vec::new(),
            release_pr: Some(pr_spec("")),
        };
        b.iter(|| {
            let _ = run_release_machine(&forge, &input);
        });
    });
}

/// The empty-plan close probe: no PR exists — the one find, nothing else.
fn bench_empty_plan_no_pr(c: &mut Criterion) {
    c.bench_function("machine/empty_plan_no_pr", |b| {
        let forge = MemoryForge::new();
        let input = MachineInput {
            branch: main_branch(),
            pending_releases: Vec::new(),
            release_pr: None,
        };
        b.iter(|| {
            let _ = run_release_machine(&forge, &input);
        });
    });
}

/// Criterion entry point (hand-written instead of `criterion_group!` so the
/// generated function keeps its doc comment under `missing_docs`).
fn main() {
    let mut c = Criterion::default().configure_from_args();
    bench_tag_phase_create(&mut c);
    bench_tag_phase_rerun(&mut c);
    bench_prepare_noop(&mut c);
    bench_prepare_update(&mut c);
    bench_empty_plan_no_pr(&mut c);
}
