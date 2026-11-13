//! The mock-forge path baseline (M4.1a): the in-memory cost of the
//! Release-PR lifecycle steps the M4.2 state machine drives. The real
//! §14.1 budget for the forge is the ≤2-call update path (§11.5), so this
//! bench documents the seam's in-memory overhead, not a network budget.
//!
//! Run: cargo bench -p halyard-forge

use criterion::Criterion;
use halyard_forge::{BranchName, Forge, MemoryForge, PrSpec, ReleaseSpec};

/// Branch fixture shared by the bench groups.
fn main_branch() -> BranchName {
    BranchName::new("halyard/release/main").unwrap_or_else(|e| unreachable!("valid branch: {e}"))
}

/// PR spec fixture: a plausible Release-PR body size (~1 KiB).
fn pr_spec() -> PrSpec {
    let mut body = String::with_capacity(1024);
    body.push_str("## 🚀 Release plan\n\n| Package | Current | Next | Bump | Why |\n");
    for i in 0..12 {
        body.push_str(&format!(
            "| `pkg-{i}` | 0.1.{i} | 0.2.0 | minor | `feat: …` |\n"
        ));
    }
    PrSpec {
        branch: main_branch(),
        title: "chore: release v0.2.0".to_owned(),
        body,
    }
}

/// Release spec fixture: a plausible changelog payload (~512 B).
fn release_spec(tag: &str) -> ReleaseSpec {
    let mut notes = String::with_capacity(512);
    notes.push_str("## Features\n\n");
    for i in 0..8 {
        notes.push_str(&format!("- feat(pkg-{i}): add the thing ({i})\n"));
    }
    ReleaseSpec {
        tag: tag.to_owned(),
        notes,
    }
}

/// `find_release_pr` over a forge that already carries the Release PR
/// (the hit path; the §11.5 update path's first call).
fn bench_find_hit(c: &mut Criterion) {
    c.bench_function("find_release_pr/hit", |b| {
        let forge = MemoryForge::new();
        let spec = pr_spec();
        let _ = forge.upsert_pr(&spec);
        let branch = main_branch();
        b.iter(|| forge.find_release_pr(&branch));
    });
}

/// The full idempotent upsert: hit → in-place rewrite (the M4.2 prepare
/// phase's steady state).
fn bench_upsert_idempotent(c: &mut Criterion) {
    c.bench_function("upsert_pr/idempotent_rewrite", |b| {
        let forge = MemoryForge::new();
        let spec = pr_spec();
        let _ = forge.upsert_pr(&spec);
        b.iter(|| forge.upsert_pr(&spec));
    });
}

/// `create_release` on an already-released tag (GET-by-tag-first, the
/// §11.1 tag-phase rerun path).
fn bench_create_release_existing(c: &mut Criterion) {
    c.bench_function("create_release/already_exists", |b| {
        let forge = MemoryForge::new();
        let spec = release_spec("core-v0.5.0");
        let _ = forge.create_release(&spec);
        b.iter(|| forge.create_release(&spec));
    });
}

/// Criterion entry point (hand-written instead of `criterion_group!` so the
/// generated function keeps its doc comment under `missing_docs`).
fn main() {
    let mut c = Criterion::default().configure_from_args();
    bench_find_hit(&mut c);
    bench_upsert_idempotent(&mut c);
    bench_create_release_existing(&mut c);
}
