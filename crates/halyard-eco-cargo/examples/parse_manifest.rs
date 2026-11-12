//! Example: parse a Cargo manifest with spans (M1.5).
//!
//! Run: `cargo run -p halyard-eco-cargo --example parse_manifest`
//!
//! Demonstrates the pure adapter surface: a [`DiscoverCtx`], the
//! [`CargoAdapter`], and the dependency rows with their edit sites (§9.1).

use halyard_core::discover::{DiscoverCtx, EcosystemAdapter, RawVersionSource};
use halyard_eco_cargo::CargoAdapter;

const ROOT: &str = r#"
[workspace]
members = ["crates/*"]

[workspace.package]
version = "0.4.1"

[workspace.dependencies]
engine = { path = "crates/engine", version = "0.4.1" }
"#;

const MEMBER: &str = r#"
[package]
name = "engine-cli"
version.workspace = true
publish = false

[dependencies]
engine = { workspace = true }
serde = "1.0"

[dev-dependencies]
tempfile = "3"
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("Cargo.toml", ROOT.as_bytes().to_vec());
    ctx.insert_file("crates/cli/Cargo.toml", MEMBER.as_bytes().to_vec());

    let packages = CargoAdapter::new().discover(&ctx)?;
    for pkg in packages {
        println!("package {} (root: {})", pkg.name, if pkg.root.is_empty() { "<repo root>" } else { &pkg.root });
        println!("  version: {}", pkg.version.as_ref().map_or("?".into(), ToString::to_string));
        match &pkg.version_source {
            RawVersionSource::Local => println!("  version source: local"),
            RawVersionSource::WorkspaceInherited { file, key_path } => {
                println!("  version source: inherited from {file}#{key_path}");
            }
            RawVersionSource::Dynamic => println!("  version source: dynamic"),
        }
        println!("  publishable: {}", pkg.publishable);
        for dep in &pkg.deps {
            let spans = dep
                .site
                .spans
                .iter()
                .map(|s| format!("{}..{}", s.start, s.end))
                .collect::<Vec<_>>()
                .join(", ");
            println!(
                "  dep {:?} kind={:?} req={:?} site={}:{spans} internal={}",
                dep.name, dep.kind, dep.req, dep.site.file, dep.internal
            );
        }
    }
    Ok(())
}
