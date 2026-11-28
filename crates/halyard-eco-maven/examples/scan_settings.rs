//! Example: discover a settings-governed Gradle repository — the
//! multi-project aggregator shape (M8, the Maven/Gradle cell's Gradle
//! half, ADR-0058 continued).
//!
//! Run: `cargo run -p halyard-eco-maven --example scan_settings`
//!
//! A `settings.gradle` / `settings.gradle.kts` governs its directory
//! subtree at the VERIFIED Settings grammar (`docs/research/
//! gradle-grammars.md` §6): the root build file is named by
//! `rootProject.name` (*"The root project's name defaults to the name
//! of the directory containing the settings file"* when no literal row
//! applies), each include-proven member is named by the include path's
//! last element (*"The last element of the supplied path is used as the
//! project name"*, *"the supplied path is converted to a project
//! directory relative to the root project directory"*, implicit
//! parents *"added if they don't already exist"*), an unincluded build
//! file keeps the standalone row (warned), and a non-literal name row
//! or include argument is recognized but non-canonical (warned — the
//! fail-closed discipline never guesses a name).

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
            // A proven member without a build file: Gradle permits
            // build-less projects — there is no row to plan, and the
            // root row carries the honest warning.
            "include ':tools'\n",
        )
        .as_bytes()
        .to_vec(),
    );
    ctx.insert_file("build.gradle", "version = '1.4.2'\n".as_bytes().to_vec());
    ctx.insert_file(
        "library/build.gradle",
        "version = '2.0.0'\n".as_bytes().to_vec(),
    );
    ctx.insert_file(
        "services/api/build.gradle.kts",
        "version = \"0.3.0\"\n".as_bytes().to_vec(),
    );
    // A build file the settings file does not govern: the standalone
    // shape keeps its row (warned — the per-file shape never silently
    // shrinks).
    ctx.insert_file(
        "extra/build.gradle",
        "version = '3.0.0'\n".as_bytes().to_vec(),
    );

    let rows = MavenAdapter::new().discover(&ctx)?;
    println!("discovered {} governed maven/gradle rows:\n", rows.len());
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
            "    version: {:?}",
            row.version.as_ref().map(std::string::ToString::to_string)
        );
        for warning in &row.warnings {
            println!("    warning [{}]: {}", warning.code, warning.message);
        }
        println!();
    }
    Ok(())
}
