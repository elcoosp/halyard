//! Example: discover a small polyglot workspace end-to-end (M1.5/M1.6/
//! M1.7).
//!
//! Run: `cargo run -p halyard-discover --example discover_workspace`
//!
//! Prints the merged `Workspace { packages, deps, links }`: a Cargo
//! crate + its napi binding + an app that consumes the binding through
//! `workspace:^` — the §6.5 link rules connect all three.

use halyard_core::discover::DiscoverCtx;
use halyard_discover::{discover_from_ctx, DiscoverInput};

const FILES: &[(&str, &str)] = &[
    (
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.package]\nversion = \"0.1.0\"\n",
    ),
    (
        "crates/native/Cargo.toml",
        "[package]\nname = \"native\"\nversion.workspace = true\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\nserde = \"1.0\"\n",
    ),
    (
        "crates/native/package.json",
        "{\n  \"name\": \"@org/native\",\n  \"version\": \"0.1.0\",\n  \"napi\": {\"binaryName\": \"native\"},\n  \"dependencies\": {}\n}\n",
    ),
    (
        "crates/native/npm/linux-x64-gnu/package.json",
        "{\n  \"name\": \"@org/native-linux-x64-gnu\",\n  \"version\": \"0.1.0\",\n  \"private\": true\n}\n",
    ),
    (
        "packages/app/package.json",
        "{\n  \"name\": \"app\",\n  \"version\": \"0.2.0\",\n  \"private\": true,\n  \"dependencies\": {\n    \"@org/native\": \"workspace:^\"\n  }\n}\n",
    ),
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut ctx = DiscoverCtx::new();
    for (path, body) in FILES {
        ctx.insert_file(path, body.as_bytes().to_vec());
    }
    let discovery = discover_from_ctx(&ctx, &DiscoverInput::default())?;
    let ws = &discovery.workspace;

    println!("packages:");
    for pkg in ws.packages() {
        let name = ws.interner().get(pkg.name).unwrap_or("?");
        let deps: Vec<String> = pkg
            .deps
            .iter()
            .filter_map(|d| ws.package(d.to).and_then(|t| ws.interner().get(t.name)))
            .map(|n| n.to_owned())
            .collect();
        println!(
            "  {name:<28} v{} publishable={} deps=[{}]",
            pkg.version.as_ref().map_or("?".into(), ToString::to_string),
            pkg.publishable,
            deps.join(", ")
        );
    }
    println!("links (from -> to, mode, rule):");
    for link in ws.links() {
        let from = ws.package(link.from).and_then(|p| ws.interner().get(p.name)).unwrap_or("?");
        let to = ws.package(link.to).and_then(|p| ws.interner().get(p.name)).unwrap_or("?");
        println!("  {from} -> {to} (mode: {}, rule: {})", link.mode.wire_key(), link.rule.wire_key());
    }
    match discovery.graph.topo_order() {
        Ok(order) => {
            let names: Vec<&str> = order
                .iter()
                .filter_map(|id| ws.package(*id).and_then(|p| ws.interner().get(p.name)))
                .collect();
            println!("topo (dependencies first): {}", names.join(" -> "));
        }
        Err(e) => println!("graph error: {e}"),
    }
    if !discovery.warnings.is_empty() {
        println!("warnings:");
        for warning in &discovery.warnings {
            println!("  [{}] {}: {}", warning.code, warning.subject, warning.message);
        }
    }
    Ok(())
}
