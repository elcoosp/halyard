//! The §11.1 step-3 content-commit pass shapes (the M4.2 remainder) over
//! the [`MemoryForge`] reference adapter: the divergent first commit,
//! the matched re-run (I1's zero-mutating-calls path) and the PROPAGATE
//! gate. The compare is an OID equality — the pass cost is the forge
//! call flow, not the tree hash (the tree builder has its own bench in
//! `halyard-git`).

use criterion::{BenchmarkId, Criterion};
use halyard_core::reason::Oid;
use halyard_forge::{
    ApiFile, BranchName, ContentCommit, MachineInput, MemoryForge, PrSpec, ReleaseSpec,
    run_release_machine,
};

/// A 40-hex tree fixture (deterministic; only equality matters to the
/// machine — the OID is opaque data at this seam).
fn tree_oid(seed: u8) -> Oid {
    let digits: String = std::iter::repeat_n(format!("{seed:02x}"), 20)
        .collect::<Vec<_>>()
        .join("");
    Oid::from_hex(digits.as_bytes()).expect("hex fixture is 40 digits")
}

/// A content payload of `count` files (the applied EditSet's shape).
fn payload(count: usize, seed: u8) -> Vec<ApiFile> {
    (0..count)
        .map(|index| ApiFile {
            path: format!("crates/pkg-{index}/Cargo.toml"),
            content: format!("[package]\nname = \"pkg-{index}\"\nversion = \"0.{seed}.0\"\n")
                .into_bytes(),
        })
        .collect()
}

fn divergent_input(count: usize) -> MachineInput {
    MachineInput {
        branch: BranchName::new("halyard/release/main").expect("branch fixture is valid"),
        pending_releases: Vec::new(),
        release_pr: Some(PrSpec {
            branch: BranchName::new("halyard/release/main").expect("branch fixture is valid"),
            title: "chore: release v0.1.0".to_owned(),
            body: "## 🚀 Release plan".to_owned(),
        }),
        content_commit: Some(ContentCommit {
            message: "chore: release v0.1.0".to_owned(),
            files: payload(count, 1),
            tree_oid: tree_oid(0xaa),
            remote_tree_oid: None,
        }),
    }
}

fn matched_input(count: usize) -> MachineInput {
    MachineInput {
        branch: BranchName::new("halyard/release/main").expect("branch fixture is valid"),
        pending_releases: Vec::new(),
        release_pr: Some(PrSpec {
            branch: BranchName::new("halyard/release/main").expect("branch fixture is valid"),
            title: "chore: release v0.1.0".to_owned(),
            body: "## 🚀 Release plan".to_owned(),
        }),
        content_commit: Some(ContentCommit {
            message: "chore: release v0.1.0".to_owned(),
            files: payload(count, 1),
            tree_oid: tree_oid(0xbb),
            remote_tree_oid: Some(tree_oid(0xbb)),
        }),
    }
}

fn propagated_input() -> MachineInput {
    MachineInput {
        branch: BranchName::new("halyard/release/main").expect("branch fixture is valid"),
        pending_releases: vec![ReleaseSpec {
            tag: "pkg-0-v0.1.0".to_owned(),
            notes: "## Features".to_owned(),
        }],
        release_pr: None,
        content_commit: None,
    }
}

fn bench_content_commit(c: &mut Criterion) {
    let mut group = c.benchmark_group("forge_content_commit");
    for count in [1, 40] {
        group.bench_with_input(
            BenchmarkId::new("divergent_pass", count),
            &count,
            |b, count| {
                b.iter_batched(
                    || (MemoryForge::new(), divergent_input(*count)),
                    |(forge, input)| {
                        let _ = run_release_machine(&forge, &input)
                            .expect("the reference forge commits");
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
        group.bench_with_input(
            BenchmarkId::new("matched_pass", count),
            &count,
            |b, count| {
                b.iter_batched(
                    || (MemoryForge::new(), matched_input(*count)),
                    |(forge, input)| {
                        let _ = run_release_machine(&forge, &input)
                            .expect("the matched pass is a no-op commit");
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }
    group.bench_function("propagate_gate", |b| {
        b.iter_batched(
            || (MemoryForge::new(), propagated_input()),
            |(forge, input)| {
                let _ = run_release_machine(&forge, &input).expect("the tag phase owns the pass");
            },
            criterion::BatchSize::SmallInput,
        );
    });
    group.finish();
}

/// Criterion entry point (hand-written instead of `criterion_group!` so
/// the generated function keeps its doc comment under `missing_docs`).
fn main() {
    let mut c = Criterion::default().configure_from_args();
    bench_content_commit(&mut c);
}
