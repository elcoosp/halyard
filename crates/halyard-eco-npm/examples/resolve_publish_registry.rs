//! Example: resolve an npm package's publish registry (M5.2 continued,
//! ADR-0021).
//!
//! Run: `cargo run -p halyard-eco-npm --example resolve_publish_registry`
//!
//! Demonstrates the npm resolution surface the CLI shell's per-target
//! routing consumes: the `publishConfig.registry` publish-time
//! override, the `NPM_CONFIG_REGISTRY` environment override, the
//! `.npmrc` `registry` / `@scope:registry` keys, and the documented
//! default — with the honest decline for a non-HTTP value.

use halyard_eco_npm::publish_registry::{PublishRegistryBase, resolve_publish_registry};

const PKG_WITH_OVERRIDE: &[u8] =
    br#"{ "name": "p1", "version": "0.1.0", "publishConfig": { "registry": "https://corp.example/registry" } }"#;
const PKG_PLAIN: &[u8] = br#"{ "name": "p2", "version": "0.1.0" }"#;
const NPMRC: &str = "@myorg:registry=https://myorg.example/\nregistry=https://npmrc.example/\n";

fn main() {
    println!("p1 (publishConfig.registry pinned):");
    match resolve_publish_registry("p1", Some(PKG_WITH_OVERRIDE), &[(".npmrc", NPMRC)], None) {
        PublishRegistryBase::Custom(base) => println!(
            "  custom base {base}\n  → probe with Npm::with_registry_base(transport, \"{base}\") \
             as a per-target override"
        ),
        other => println!("  {other}"),
    }

    println!("@myorg/pkg (scoped @scope:registry key):");
    match resolve_publish_registry("@myorg/pkg", Some(PKG_PLAIN), &[(".npmrc", NPMRC)], None) {
        PublishRegistryBase::Custom(base) => println!("  custom base {base} (the scoped key wins)"),
        other => println!("  {other}"),
    }

    println!("p2 (npmrc registry key):");
    match resolve_publish_registry("p2", Some(PKG_PLAIN), &[(".npmrc", NPMRC)], None) {
        PublishRegistryBase::Custom(base) => println!("  custom base {base}"),
        other => println!("  {other}"),
    }

    println!("p3 (nothing configured):");
    match resolve_publish_registry("p3", Some(PKG_PLAIN), &[], None) {
        PublishRegistryBase::Default => {
            println!("  the default npm registry (the by-name route probes it)");
        }
        other => println!("  {other}"),
    }

    println!("p4 (a non-HTTP override):");
    match resolve_publish_registry(
        "p4",
        Some(br#"{ "name": "p4", "publishConfig": { "registry": "corp.example" } }"#),
        &[],
        None,
    ) {
        PublishRegistryBase::Unsupported(message) => println!("  declined: {message}"),
        other => println!("  {other}"),
    }
}
