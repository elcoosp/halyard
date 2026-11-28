//! Example: apply the maven adapter's Gradle semantic edit seam (M2 of
//! the §17 **Maven/Gradle** cell's Gradle half — ADR-0058 continued).
//!
//! Run: `cargo run -p halyard-eco-maven --example bump_gradle`
//!
//! The plan's own-version byte patches re-apply **semantically and in
//! place**: the patch span must land on the LAST literal
//! `version = …` assignment's inner-byte span (Gradle's Project setter
//! overwrites on each assignment — the last literal in document order is
//! the row's source; a drifted file refuses `E-EDIT-PATCHES`), the
//! literal's inner bytes swap and every other byte survives verbatim
//! (invariant I4), and a re-scan read-back verifies the file now carries
//! the target before the patch is accepted. A patch whose text already
//! equals the current value is the idempotent re-run (I1).

use halyard_core::EcosystemAdapter;
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;
use halyard_eco_maven::MavenAdapter;

/// The Groovy build-file fixture: the literal `version = '1.4.2'`
/// assignment inside the realistic instruction mix — the `plugins`
/// block, the line comment, the `repositories` block and the sibling
/// string literal are the bytes the swap must preserve.
const BUILD_GRADLE: &str = concat!(
    "plugins {\n",
    "    id 'java'\n",
    "}\n",
    "\n",
    "// the project version (a literal assignment — the documented form)\n",
    "version = '1.4.2'\n",
    "\n",
    "repositories {\n",
    "    mavenCentral()\n",
    "}\n",
    "\n",
    "description = \"the version = '9.9.9' string is skipped in place\"\n",
);

/// The fixture offset as a plan-contract span coordinate (loud,
/// panic-free failure for a constant fixture that cannot miss).
fn span_of(haystack: &str, needle: &str) -> (u32, u32) {
    let start = haystack
        .find(needle)
        .and_then(|at| u32::try_from(at).ok())
        .unwrap_or_else(|| {
            eprintln!("the fixture must carry {needle}");
            std::process::exit(101);
        });
    let length = u32::try_from(needle.len()).unwrap_or_else(|_| {
        eprintln!("the fixture needle must fit the span coordinate range");
        std::process::exit(101);
    });
    (start, start + length)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = RelPath::new("build.gradle").ok_or("the fixture path is normalized")?;
    let adapter = MavenAdapter::new();

    // 1. The plan's patch: the discovery scan's span (the last literal's
    //    inner bytes), the target from the plan row.
    let (start, end) = span_of(BUILD_GRADLE, "1.4.2");
    let patches = [PlanPatch {
        start,
        end,
        text: "1.5.0".to_owned(),
    }];
    let outcome = adapter.edit_versions(&file, BUILD_GRADLE.as_bytes(), &patches)?;
    println!(
        "applied {} patch (skipped {}); the build file now carries:",
        outcome.applied, outcome.skipped
    );
    let bumped = String::from_utf8(outcome.content)?;
    for line in bumped.lines().filter(|line| line.contains("version = ")) {
        println!("  {line}");
    }

    // 2. The idempotent re-run (I1): the same patch aimed at the file
    //    that already carries the target — matched, skipped, verbatim.
    let (start, end) = span_of(&bumped, "1.5.0");
    let rerun = [PlanPatch {
        start,
        end,
        text: "1.5.0".to_owned(),
    }];
    let again = adapter.edit_versions(&file, bumped.as_bytes(), &rerun)?;
    println!(
        "idempotent re-run: applied {}, skipped {} — bytes verbatim: {}",
        again.applied,
        again.skipped,
        again.content == bumped.as_bytes()
    );

    // 3. The drifted span refuses (`E-EDIT-PATCHES`) — one byte right of
    //    the value is no editable row; the bytes survive untouched.
    let drifted = [PlanPatch {
        start: start + 1,
        end: end + 1,
        text: "1.6.0".to_owned(),
    }];
    let refusal = adapter.edit_versions(&file, bumped.as_bytes(), &drifted);
    match refusal {
        Err(failure) => println!("drifted span refuses: {failure}"),
        Ok(_) => println!("unexpected: the drifted span applied"),
    }
    Ok(())
}
