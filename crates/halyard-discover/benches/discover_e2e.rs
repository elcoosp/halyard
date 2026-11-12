//! Bench: end-to-end discovery over a synthetic 40-crate workspace
//! (§6.2, the §14.1 "40 crates" front half; formal budgets arm at M2.6).

use std::path::Path;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tempfile::TempDir;

/// Writes a deterministic synthetic Cargo workspace with `n` crates
/// (linear dependency chain plus workspace-inherited versions).
fn write_workspace(root: &Path, n: usize) {
    let mut members = String::from("[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.package]\nversion = \"0.1.0\"\n\n[workspace.dependencies]\nhalyard-core = { path = \"crates/c0\", version = \"0.1.0\" }\n");
    std::fs::write(root.join("Cargo.toml"), members.as_bytes()).expect("root manifest");
    for i in 0..n {
        let dir = root.join("crates").join(format!("c{i}"));
        std::fs::create_dir_all(&dir).expect("crate dir");
        let mut manifest = format!(
            "[package]\nname = \"c{i}\"\nversion.workspace = true\nedition = \"2021\"\n\n[dependencies]\n"
        );
        if i > 0 {
            manifest.push_str(&format!("c{} = {{ path = \"../c{}\" }}\n", i - 1, i - 1));
        } else {
            manifest.push_str("serde = \"1.0\"\n");
        }
        std::fs::write(dir.join("Cargo.toml"), manifest.as_bytes()).expect("member manifest");
    }
    let _ = &mut members;
}

fn bench_discover_e2e(c: &mut Criterion) {
    let dir = TempDir::new().expect("bench tempdir");
    write_workspace(dir.path(), 40);
    let root = dir.path().to_path_buf();
    let mut group = c.benchmark_group("halyard-discover");
    group.bench_function("workspace_40_crates", |b| {
        b.iter(|| {
            let discovery = halyard_discover::discover_repo(black_box(&root), &Default::default())
                .expect("discovery succeeds");
            black_box(discovery.workspace.packages().len())
        });
    });
    group.finish();
}

criterion_group!(benches, bench_discover_e2e);
criterion_main!(benches);
