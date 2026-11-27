//! Bench: the `uv.lock` refresh verifier (the M8 `PyPI` cell's `uv.lock`
//! lockfile half, ADR-0056 surface).
//!
//! Measures the caller-side §9.2 effect verification over a realistic
//! snapshot pair — the parse-and-compare the native refresh runs after
//! every declined `uv.lock`. The verification is a pure parse over the
//! given bytes; the shapes pin the member-only move (the observed uv
//! fidelity) and the header-plus-sibling equality that refuses.

use criterion::Criterion;

use halyard_eco_pypi::uvlock::verify_native_refresh;

/// A 120-row lockfile: 40 editable member rows, 80 registry rows (the
/// uv 0.13.10 shape — header, `[manifest]`, `[[package]]` rows).
fn snapshot(member_version: &str) -> String {
    let mut text = String::from(
        "version = 1\nrevision = 5\nrequires-python = \">=3.9\"\n\n[manifest]\nmembers = [\n",
    );
    for i in 0..40 {
        let row = format!("    \"member-{i:02}\",\n");
        text.push_str(&row);
    }
    text.push_str("]\n\n");
    for i in 0..40 {
        let row = format!(
            "[[package]]\nname = \"member-{i:02}\"\nversion = \"{member_version}\"\nsource = \
             {{ editable = \"pkg{i:02}\" }}\n"
        );
        text.push_str(&row);
    }
    for i in 0..80 {
        let row = format!(
            "[[package]]\nname = \"registry-{i:02}\"\nversion = \"2.3.4\"\nsource = {{ registry \
             = \"https://pypi.org/simple\" }}\n"
        );
        text.push_str(&row);
    }
    text
}

fn bench_uvlock_verify(c: &mut Criterion) {
    let mut group = c.benchmark_group("eco-pypi");
    let members: Vec<String> = (0..40).map(|i| format!("member-{i:02}")).collect();
    group.bench_function("uvlock_verify_member_move", |b| {
        let before = snapshot("1.2.3");
        let after = snapshot("1.2.4");
        b.iter(|| {
            verify_native_refresh(before.as_bytes(), after.as_bytes(), &members)
                .unwrap_or_default();
        });
    });
    group.finish();
}

/// Criterion harness entry (the `criterion_group!` expansion, spelled out
/// so the generated `pub fn` does not trip `missing_docs`).
fn benches() {
    let mut criterion: Criterion = Criterion::default().configure_from_args();
    bench_uvlock_verify(&mut criterion);
}

fn main() {
    benches();
}
