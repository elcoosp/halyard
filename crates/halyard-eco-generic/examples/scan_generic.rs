//! Example: discover a config-declared `generic` repository and inspect
//! the rows (M8, the §17 **`generic`** cell — ADR-0062).
//!
//! Run: `cargo run -p halyard-eco-generic --example scan_generic`
//!
//! A `generic` package exists because `halyard.toml` declares it
//! (`[[generic]]`: `name`/`file`/`pattern`) — the escape hatch for
//! repositories no manifest grammar covers. The version is the declared
//! pattern's named `version` capture group (the VERIFIED prior-art
//! grammar in `docs/research/generic-version-files.md` §1.1: *"The
//! regex pattern must include a named capture group called version"*);
//! the capture's byte span is the edit site; only semver-parseable
//! captures are canonical per ADR-0056's discipline — every other
//! spelling is warned, unpublishable, never rewritten; a second match
//! is consumed unmodeled with a warning.

use halyard_core::EcosystemAdapter;
use halyard_core::discover::{DiscoverCtx, GenericFileSpec};
use halyard_eco_generic::GenericAdapter;

/// The `halyard.toml` shape this example mirrors (the loader validates
/// it with `E-CFG-GENERIC`; here the specs are built directly):
///
/// ```toml
/// [[generic]]
/// name = "squid-utils"
/// file = "VERSION"
/// pattern = '(?<version>\d+\.\d+\.\d+)'
/// ```
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("VERSION", b"1.4.2\n".to_vec());
    ctx.insert_file(
        "src/version.rs",
        b"pub const VERSION: &str = \"2.3.0\"; // the release row\n".to_vec(),
    );
    ctx.insert_file(
        "docs/loose.md",
        b"current version: 1.2 (two components - not canonical)\n".to_vec(),
    );

    let specs = [
        GenericFileSpec {
            name: "squid-utils".to_owned(),
            file: "VERSION".to_owned(),
            pattern: r"(?<version>\d+\.\d+\.\d+)".to_owned(),
        },
        GenericFileSpec {
            name: "app".to_owned(),
            file: "src/version.rs".to_owned(),
            pattern: r#"VERSION: &str = "(?<version>\d+\.\d+\.\d+)""#.to_owned(),
        },
        GenericFileSpec {
            name: "loose".to_owned(),
            file: "docs/loose.md".to_owned(),
            pattern: r"(?<version>\d+(?:\.\d+)*)".to_owned(),
        },
    ];

    let rows = GenericAdapter::new(&specs).discover(&ctx)?;
    println!("discovered {} generic rows:\n", rows.len());
    for row in &rows {
        println!(
            "  {} @ {} (version file {})",
            row.name,
            if row.root.is_empty() {
                "<root>"
            } else {
                &row.root
            },
            row.manifest
        );
        println!(
            "    version: {:?} (source {:?})",
            row.version.as_ref().map(std::string::ToString::to_string),
            row.version_source
        );
        if let Some(site) = &row.version_site {
            println!("    edit site: {} spans {:?}", site.file, site.spans);
        }
        println!(
            "    publishable: {} (no registry decision exists — the tag is the artifact, \
             ADR-0062)",
            row.publishable
        );
        for warning in &row.warnings {
            println!("    warning [{}]: {}", warning.code, warning.message);
        }
        println!();
    }
    Ok(())
}
