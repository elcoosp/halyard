//! Example: discover a Gradle repository and inspect the rows (M8, the
//! §17 **Maven/Gradle** cell's Gradle half — ADR-0058 continued).
//!
//! Run: `cargo run -p halyard-eco-maven --example scan_gradle`
//!
//! Every `build.gradle` / `build.gradle.kts` in the committed tree is a
//! standalone Maven row (the Maven/Gradle cell's single ecosystem —
//! both publish to Maven Central; the per-file shape the Dockerfile and
//! gemspec cells established). The version is the documented literal
//! `version = '…'` (Groovy) / `version = "…"` (Kotlin) assignment
//! (Gradle's Project API: *"The version of this project. Gradle always
//! uses the `toString()` value of the version. The version defaults to
//! `unspecified`."*, VERIFIED in `docs/research/gradle-grammars.md` §1);
//! the last assignment in document order wins (the Project setter's
//! ordinary overwrite); only semver-parseable literals are canonical
//! per ADR-0056's discipline — every other right-hand side is warned,
//! unpublishable, never rewritten.

use halyard_core::EcosystemAdapter;
use halyard_core::discover::DiscoverCtx;
use halyard_eco_maven::MavenAdapter;

/// The `halyard.toml`-less fixture: the build files ARE the manifests —
/// no config surface exists for the Gradle half (tree-scanned like
/// `RubyGems` and `Docker image tags`).
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file(
        "build.gradle",
        concat!(
            "plugins {\n",
            "    id 'java'\n",
            "}\n",
            "\n",
            "version = '1.4.2'\n",
            "\n",
            "repositories {\n",
            "    mavenCentral()\n",
            "}\n",
        )
        .as_bytes()
        .to_vec(),
    );
    ctx.insert_file(
        "services/api/build.gradle.kts",
        concat!(
            "plugins {\n",
            "    java\n",
            "}\n",
            "\n",
            "version = \"2.0.0\"\n",
        )
        .as_bytes()
        .to_vec(),
    );
    // A build file whose version is a property reference — recognized
    // but non-canonical (the fail-closed discipline, ADR-0056).
    ctx.insert_file(
        "services/worker/build.gradle",
        "version = findProperty('appVersion')\n".as_bytes().to_vec(),
    );

    let rows = MavenAdapter::new().discover(&ctx)?;
    println!("discovered {} maven/gradle rows:\n", rows.len());
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
            "    publishable: {} (no gradle publish argv arm exists — the disabled \
             shape, ADR-0058 D5)",
            row.publishable
        );
        for warning in &row.warnings {
            println!("    warning [{}]: {}", warning.code, warning.message);
        }
        println!();
    }
    Ok(())
}
