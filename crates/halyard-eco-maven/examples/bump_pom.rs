//! Example: the Maven release half — bump a pom's version through the
//! adapter's edit seam (M2 of the §17 **Maven/Gradle** cell, ADR-0058).
//!
//! Run: `cargo run -p halyard-eco-maven --example bump_pom`
//!
//! The plan's own-version patch re-applies semantically and in place
//! (invariant I4): the patch span lands on the `<project>`-owned
//! `<version>` element's value text, the inner bytes swap for the plan's
//! target, and every other byte of the pom survives verbatim. The
//! read-back verify re-parses the pom before the patch is accepted, and
//! a patch aimed off the version row refuses fail-closed
//! (`E-EDIT-PATCHES`).

use halyard_core::EcosystemAdapter;
use halyard_core::discover::DiscoverCtx;
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;
use halyard_eco_maven::MavenAdapter;

const POM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!-- a real-shaped pom: attributes, comments, indentation -->
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>demo-api</artifactId>
  <version>1.2.3</version>
</project>
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Discover the single-package workspace; the canonical version row
    // carries the edit site the plan's patch aims at.
    let mut ctx = DiscoverCtx::new();
    ctx.insert_file("pom.xml", POM.as_bytes().to_vec());
    let rows = MavenAdapter::new().discover(&ctx)?;
    let row = rows.first().ok_or("the fixture discovers one row")?;
    let site = row
        .version_site
        .as_ref()
        .ok_or("the version is canonical")?;
    println!(
        "discovered {} @ {:?} — edit site {} spans {:?}",
        row.name,
        row.version.as_ref().map(std::string::ToString::to_string),
        site.file,
        site.spans
    );

    // The plan's own-version patch: swap 1.2.3 → 1.3.0 (a feat bump).
    let file = RelPath::new("pom.xml").ok_or("fixture path")?;
    let Some(span) = site.spans.first().copied() else {
        return Err("the version site carries a span".into());
    };
    let patches = [PlanPatch {
        start: span.start,
        end: span.end,
        text: "1.3.0".to_owned(),
    }];
    let outcome = MavenAdapter::new().edit_versions(&file, POM.as_bytes(), &patches)?;
    let (applied, skipped) = (outcome.applied, outcome.skipped);
    println!("\nthe patch applied: {applied} value swapped, {skipped} already carried");
    let bumped = String::from_utf8(outcome.content)?;
    println!("\n--- the bumped pom (only the value changed) ---\n{bumped}");

    // The fail-closed posture: a patch aimed off the version row
    // refuses, and the input bytes are untouched.
    let off_row = [PlanPatch {
        start: span.start,
        end: span.end + 1,
        text: "1.3.1".to_owned(),
    }];
    match MavenAdapter::new().edit_versions(&file, POM.as_bytes(), &off_row) {
        Err(failure) => {
            println!("\nthe off-row patch fails closed as expected: {failure} (E-EDIT-PATCHES)");
        }
        Ok(_) => println!("\nunreachable: an off-row span cannot match"),
    }
    Ok(())
}
