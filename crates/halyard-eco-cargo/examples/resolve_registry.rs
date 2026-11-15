//! Example: resolve a cargo registry name to its probeable index
//! (M5.2 continued, ADR-0021).
//!
//! Run: `cargo run -p halyard-eco-cargo --example resolve_registry`
//!
//! Demonstrates the config-resolution surface the CLI shell's routing
//! map consumes: a `[registries.<name>]` configuration, the
//! `CARGO_REGISTRIES_<NAME>_INDEX` override, and the three lookup
//! outcomes — a sparse index base (probeable with
//! `halyard_registry::CratesIo::with_index_base`), an honest decline
//! naming the fix, and the unknown-name miss.

use halyard_eco_cargo::registry_config::{
    RegistryIndexLookup, registry_env_fragment, resolve_registry_index,
};

fn main() {
    // A realistic project-level configuration: a sparse custom
    // registry, a legacy git-index registry, and a token-only entry.
    let config = "\
[registries.corp]
index = \"sparse+https://corp.example/index/\"

[registries.corp-legacy]
index = \"https://corp.example/git/index\"

[registries.token-only]
token = \"redacted\"
";

    for name in ["corp", "corp-legacy", "token-only", "absent"] {
        let lookup = resolve_registry_index(name, &[(".cargo/config.toml", config)], None);
        println!("{name}:");
        match &lookup {
            RegistryIndexLookup::Sparse(base) => println!(
                "  sparse index base {base}\n  → probe with CratesIo::with_index_base(transport, \
                 \"{base}\")"
            ),
            RegistryIndexLookup::Unsupported(message) => println!("  declined: {message}"),
            RegistryIndexLookup::Unknown => println!(
                "  unknown registry (no [registries.{name}] entry; its env override is \
                 CARGO_REGISTRIES_{}_INDEX)",
                registry_env_fragment(name)
            ),
        }
    }
}
