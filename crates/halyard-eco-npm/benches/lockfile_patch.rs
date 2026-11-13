//! Bench: `pnpm-lock.yaml` patch pass (M3.4, §9.3 hot path of the apply
//! flow).
//!
//! Measures the full `patch_pnpm_lockfile` walk (scan + importer-row
//! matching + byte rewrite + read-back re-analysis) over a synthetic
//! lockfile whose importers depend on workspace members through plain
//! semver specifiers resolved as links. Formal apply budgets are §14.1
//! ("Apply edits (40 files)"); the lockfile pass is part of that stage.

use criterion::Criterion;
use halyard_core::lockfile::{LockfileOutcome, LockfileVersionChange};
use halyard_eco_npm::lockfile::{patch_pnpm_lockfile, verify_native_refresh};

/// Builds a deterministic synthetic lockfile: `importers` importer blocks
/// each depending on two members (plain semver specifier + link ref with
/// the resolved version) plus one foreign registry row under `packages:`.
fn synthetic_lock(importers: usize, members: usize) -> String {
    use std::fmt::Write as _;
    let mut out = String::from("lockfileVersion: '9.0'\n\nimporters:\n\n");
    for i in 0..importers {
        // Two deps per importer: one plain-semver link-resolved member
        // (its specifier and link ref both ride the move) and one
        // workspace-protocol member (no version in its ref).
        let semver_dep = (i + 1) % members.max(1);
        let linked_dep = (i + 2) % members.max(1);
        let _ = writeln!(
            out,
            "  packages/app-{i:04}:\n    dependencies:\n      member-{semver_dep:04}:\n        \
             specifier: ^0.1.{semver_dep}\n        version: \
             0.1.{semver_dep}(link:../member-{semver_dep:04})\n      member-{linked_dep:04}:\n        \
             specifier: workspace:*\n        version: link:../member-{linked_dep:04}\n"
        );
    }
    let _ = writeln!(out, "\npackages:\n");
    for m in 0..members {
        let _ = writeln!(
            out,
            "  /registry-{m:04}@1.0.{}:\n    resolution: {{integrity: sha512-{m:064x}}}\n",
            m % 10,
        );
    }
    out
}

/// The planned moves: member `m` sits at `0.1.m` (every importer
/// referencing it carries `^0.1.m`), so each move rewrites that
/// importer's specifier and link ref.
fn changes_for(members: usize) -> Vec<LockfileVersionChange> {
    (0..members)
        .map(|i| LockfileVersionChange {
            name: format!("member-{i:04}"),
            from: format!("0.1.{i}"),
            to: format!("0.2.{i}"),
        })
        .collect()
}

fn bench_lockfile_patch(c: &mut Criterion) {
    let lock = synthetic_lock(40, 10);
    let changes = changes_for(10);
    let mut group = c.benchmark_group("eco-npm");
    group.throughput(criterion::Throughput::Bytes(
        u64::try_from(lock.len()).unwrap_or(u64::MAX),
    ));
    group.bench_function("patch_pnpm_lockfile_40importers_10moves", |b| {
        b.iter(|| {
            match patch_pnpm_lockfile(std::hint::black_box(&lock), "pnpm-lock.yaml", &changes) {
                Ok(outcome) => outcome,
                Err(e) => unreachable!("fixture patches cleanly: {e}"),
            }
        });
    });
    // M3.4 companion: the post-refresh verification gate
    // (`verify_native_refresh`) walks the same row space over the
    // before/after pair — its cost is what the native fallback adds on
    // top of the spawn.
    let patched = match patch_pnpm_lockfile(&lock, "pnpm-lock.yaml", &changes) {
        Ok(LockfileOutcome::Patched { content, .. }) => content,
        other => unreachable!("fixture patches cleanly: {other:?}"),
    };
    let members: Vec<String> = (0..10usize).map(|i| format!("member-{i:04}")).collect();
    group.bench_function("verify_native_refresh_40importers_10moves", |b| {
        b.iter(|| {
            match verify_native_refresh(
                std::hint::black_box(lock.as_bytes()),
                std::hint::black_box(&patched),
                &members,
            ) {
                Ok(moved) => moved,
                Err(e) => unreachable!("fixture verifies cleanly: {e}"),
            }
        });
    });
    group.finish();
}

/// Criterion harness entry (the `criterion_group!` expansion, spelled out
/// so the generated `pub fn` does not trip `missing_docs`).
fn benches() {
    let mut criterion: Criterion = Criterion::default().configure_from_args();
    bench_lockfile_patch(&mut criterion);
}

fn main() {
    benches();
    Criterion::default().configure_from_args().final_summary();
}
