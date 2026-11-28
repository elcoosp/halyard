//! Example: the Gradle half's proven internal dependency edges (M8,
//! the Maven/Gradle cell, ADR-0058 continued — the recorded-open
//! `project(":sub")` remainder's landing).
//!
//! Run: `cargo run -p halyard-eco-maven --example gradle_deps`
//!
//! A top-level `dependencies { … }` block's `<configuration>
//! project(':path')` rows resolve against the settings-governed member
//! set (the VERIFIED Settings grammar's include provenance): a leading-
//! `:` path naming a proven member is an internal, path-only edge
//! (`ReqSpec::PathOnly` — a project dependency carries no version
//! constraint, so there is no requirement text to rewrite and the
//! cascade policy stays the planner's own; the explicit `[[link]]`
//! config surface remains the forced-cascade shape). A path nothing
//! proves warns and stays external — fail-closed, nothing guessed.

use halyard_core::EcosystemAdapter;
use halyard_core::discover::DiscoverCtx;
use halyard_eco_maven::MavenAdapter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file(
        "settings.gradle",
        concat!(
            "rootProject.name = 'polyglot-parent'\n",
            "\n",
            "include 'library'\n",
            "include ':services:api'\n",
        )
        .as_bytes()
        .to_vec(),
    );
    ctx.insert_file(
        "build.gradle",
        concat!(
            "version = '1.0.0'\n",
            "\n",
            "dependencies {\n",
            "    implementation project(':library')\n",
            "    testImplementation project(':services:api')\n",
            "    implementation project(':ghost')\n",
            "}\n",
        )
        .as_bytes()
        .to_vec(),
    );
    ctx.insert_file(
        "library/build.gradle",
        "version = '2.0.0'\n".as_bytes().to_vec(),
    );
    ctx.insert_file(
        "services/api/build.gradle.kts",
        "version = \"0.3.0\"\n".as_bytes().to_vec(),
    );

    let rows = MavenAdapter::new().discover(&ctx)?;
    println!(
        "discovered {} maven/gradle rows with their edges:\n",
        rows.len()
    );
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
        if row.deps.is_empty() {
            println!("    deps: (none)");
        }
        for dep in &row.deps {
            println!(
                "    -> {} ({}, {:?}, raw {:?}, internal)",
                dep.name,
                if dep.kind == halyard_core::package::DepKind::Dev {
                    "dev"
                } else {
                    "normal"
                },
                dep.req,
                dep.raw
            );
        }
        for warning in &row.warnings {
            println!("    warning [{}]: {}", warning.code, warning.message);
        }
        println!();
    }
    Ok(())
}
