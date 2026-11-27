//! Example: discover a Dockerfile repository and inspect the rows (M8,
//! the §17 **`Docker image tags`** cell — ADR-0063).
//!
//! Run: `cargo run -p halyard-eco-docker --example scan_docker`
//!
//! A `Dockerfile` discovers as one package row: the version is the
//! OCI-documented version annotation
//! (`org.opencontainers.image.version`, *"version of the packaged
//! software"*, VERIFIED in `docs/research/docker-image-tags.md` §1)
//! carried by the Dockerfile reference's `LABEL` instruction; the
//! label's value span is the edit site; only semver-parseable values
//! are canonical per ADR-0056's discipline — every other spelling is
//! warned, unpublishable, never rewritten; a `FROM` row *"sets the base
//! image"* — its tag is the base image's version, never this package's;
//! Docker's own override rule (*"the most-recently-applied value
//! overrides any previously-set value"*) makes the LAST version label
//! the row's site.

use halyard_core::EcosystemAdapter;
use halyard_core::discover::DiscoverCtx;
use halyard_eco_docker::DockerAdapter;

/// The `halyard.toml`-less fixture: the Dockerfile IS the manifest —
/// no config surface exists for the cell (tree-scanned like `RubyGems`).
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file(
        "Dockerfile",
        concat!(
            "# syntax=docker/dockerfile:1\n",
            "FROM alpine:3.20\n",
            "LABEL org.opencontainers.image.title=\"squid-image\"\n",
            "LABEL org.opencontainers.image.version=\"1.4.2\"\n",
            "CMD [\"/app/squid\"]\n",
        )
        .as_bytes()
        .to_vec(),
    );
    ctx.insert_file(
        "services/api/Dockerfile",
        concat!(
            "FROM alpine:3.20\n",
            "LABEL org.opencontainers.image.version=\"2.0\"\n",
        )
        .as_bytes()
        .to_vec(),
    );
    ctx.insert_file(
        "services/worker/Dockerfile",
        concat!(
            "FROM alpine:3.20\n",
            "LABEL org.opencontainers.image.title=\"worker\"\n",
        )
        .as_bytes()
        .to_vec(),
    );

    let rows = DockerAdapter::new().discover(&ctx)?;
    println!("discovered {} docker rows:\n", rows.len());
    for row in &rows {
        println!(
            "  {} @ {} (manifest {})",
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
            "    publishable: {} (no docker build/push argv arm exists — the disabled \
             shape, ADR-0063)",
            row.publishable
        );
        for warning in &row.warnings {
            println!("    warning [{}]: {}", warning.code, warning.message);
        }
        println!();
    }
    Ok(())
}
