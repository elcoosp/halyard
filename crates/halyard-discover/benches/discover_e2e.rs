//! Bench: end-to-end discovery over a synthetic 40-crate workspace
//! (§6.2, the §14.1 "40 crates" front half; formal budgets arm at M2.6).

use std::fmt::Write as _;
use std::path::Path;

use criterion::Criterion;
use halyard_discover::DiscoverInput;
use tempfile::TempDir;

/// Writes a deterministic synthetic Cargo workspace with `n` crates
/// (linear dependency chain plus workspace-inherited versions).
fn write_workspace(root: &Path, n: usize) {
    let mut members = String::from(
        "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.package]\nversion = \"0.1.0\"\n\n[workspace.dependencies]\nhalyard-core = { path = \"crates/c0\", version = \"0.1.0\" }\n",
    );
    std::fs::write(root.join("Cargo.toml"), members.as_bytes())
        .unwrap_or_else(|e| unreachable!("root manifest: {e}"));
    for i in 0..n {
        let dir = root.join("crates").join(format!("c{i}"));
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("crate dir: {e}"));
        let mut manifest = format!(
            "[package]\nname = \"c{i}\"\nversion.workspace = true\nedition = \"2021\"\n\n[dependencies]\n"
        );
        if i > 0 {
            manifest
                .write_fmt(format_args!(
                    "c{} = {{ path = \"../c{}\" }}\n",
                    i - 1,
                    i - 1
                ))
                .unwrap_or_else(|e| unreachable!("member manifest: {e}"));
        } else {
            manifest.push_str("serde = \"1.0\"\n");
        }
        std::fs::write(dir.join("Cargo.toml"), manifest.as_bytes())
            .unwrap_or_else(|e| unreachable!("member manifest: {e}"));
    }
    let _ = &mut members;
}

fn bench_discover_e2e(c: &mut Criterion) {
    let dir = TempDir::new().unwrap_or_else(|e| unreachable!("bench tempdir: {e}"));
    write_workspace(dir.path(), 40);
    let root = dir.path().to_path_buf();
    let mut group = c.benchmark_group("halyard-discover");
    group.bench_function("workspace_40_crates", |b| {
        b.iter(|| {
            let discovery = halyard_discover::discover_repo(
                std::hint::black_box(&root),
                &DiscoverInput::default(),
            )
            .unwrap_or_else(|e| unreachable!("discovery succeeds: {e}"));
            std::hint::black_box(discovery.workspace.packages().len())
        });
    });
    group.finish();
}

/// Criterion harness entry (the `criterion_group!` expansion, spelled out
/// so the generated `pub fn` does not trip `missing_docs`).
fn benches() {
    let mut criterion: Criterion = Criterion::default().configure_from_args();
    bench_discover_e2e(&mut criterion);
}

fn main() {
    benches();
    Criterion::default().configure_from_args().final_summary();
}
