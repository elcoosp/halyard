//! The §11.6 check-comment path (M4.5 step 1): the in-memory cost of the
//! marker-based comment upsert the `check --comment` mode drives — the
//! create path (fresh forge, scan misses) and the rewrite path (the
//! marker hit, rewritten in place — the re-run shape, I1) over the
//! deterministic [`MemoryForge`] reference adapter, plus the §11.6
//! comment render the shell feeds it. The real §14.1 budget for the
//! forge remains the ≤2-call update path (§11.5); this bench documents
//! the seam's in-memory overhead, not a network budget.
//!
//! Run: `cargo bench -p halyard-forge --bench pr_comment`

use std::hint::black_box;

use criterion::Criterion;
use halyard_changelog::{
    CHECK_MARKER, CheckComment, CheckPrediction, CheckVerdict, render_check_comment,
};
use halyard_core::Bump;
use halyard_forge::{BranchName, CommentSpec, Forge, MemoryForge, PrSpec};

/// PR spec fixture: opens the comment target on the head branch the
/// comment mode probes.
fn pr_spec() -> PrSpec {
    PrSpec {
        branch: BranchName::new("feature/topic")
            .unwrap_or_else(|e| unreachable!("valid branch: {e}")),
        title: "feat: the change".to_owned(),
        body: "the change body".to_owned(),
    }
}

/// The §11.6 comment fixture: a plausible verdict over 6 pending
/// packages — the size a workspace release prediction renders.
fn check_comment() -> CheckComment {
    let predictions = (0..6)
        .map(|index| CheckPrediction {
            package: format!("pkg-{index}"),
            from: format!("0.1.{index}"),
            bump: if index % 2 == 0 {
                Bump::Patch
            } else {
                Bump::Minor
            },
            remapped: index % 2 == 0,
        })
        .collect();
    CheckComment {
        title: "feat(parser): accept spans".to_owned(),
        verdict: CheckVerdict::Accepted {
            kind: "feat".to_owned(),
            breaking: false,
        },
        predictions,
    }
}

/// The upsert spec over the rendered body (the marker is the find key).
fn comment_spec(body: &str) -> CommentSpec {
    CommentSpec {
        pr_number: 1,
        marker: CHECK_MARKER.to_owned(),
        body: body.to_owned(),
    }
}

/// The §11.6 comment render (the CLI's input to the seam).
fn bench_render(c: &mut Criterion) {
    c.bench_function("check_comment/render_6_pkgs", |b| {
        b.iter(|| render_check_comment(black_box(&check_comment())));
    });
}

/// The create path on a fresh forge: the scan misses, the comment is
/// created (the first CI run on a PR).
fn bench_upsert_create(c: &mut Criterion) {
    c.bench_function("check_comment/upsert_create", |b| {
        b.iter(|| {
            let forge = MemoryForge::new();
            let _ = forge.upsert_pr(&pr_spec());
            let body = render_check_comment(&check_comment());
            forge
                .upsert_pr_comment(&comment_spec(black_box(&body)))
                .unwrap_or_else(|e| unreachable!("valid spec: {e}"))
        });
    });
}

/// The rewrite path: the marker hit rewrites in place — the steady-state
/// re-run (I1; the comment count stays flat).
fn bench_upsert_rewrite(c: &mut Criterion) {
    c.bench_function("check_comment/upsert_rewrite", |b| {
        let forge = MemoryForge::new();
        let _ = forge.upsert_pr(&pr_spec());
        let body = render_check_comment(&check_comment());
        let _ = forge.upsert_pr_comment(&comment_spec(&body));
        b.iter(|| {
            forge
                .upsert_pr_comment(&comment_spec(black_box(&body)))
                .unwrap_or_else(|e| unreachable!("valid spec: {e}"))
        });
    });
}

/// Criterion entry point (hand-written instead of `criterion_group!` so the
/// generated function keeps its doc comment under `missing_docs`).
fn main() {
    let mut c = Criterion::default().configure_from_args();
    bench_render(&mut c);
    bench_upsert_create(&mut c);
    bench_upsert_rewrite(&mut c);
}
