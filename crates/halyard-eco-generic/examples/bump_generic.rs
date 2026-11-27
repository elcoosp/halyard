//! Example: apply the generic adapter's semantic edit seam (M2 of the
//! §17 **`generic`** cell — ADR-0062).
//!
//! Run: `cargo run -p halyard-eco-generic --example bump_generic`
//!
//! The plan's own-version byte patches re-apply **semantically and in
//! place**: the patch span must land on the declared pattern's named
//! `version` capture group's byte span (a drifted file refuses
//! `E-EDIT-PATCHES`), the captured bytes swap and every other byte
//! survives verbatim (invariant I4), and a re-match read-back verifies
//! the file now carries the target before the patch is accepted. A
//! patch whose text already equals the current capture is the
//! idempotent re-run (I1).

use halyard_core::EcosystemAdapter;
use halyard_core::discover::GenericFileSpec;
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;
use halyard_eco_generic::GenericAdapter;

/// The declared-file fixture: a version row inside surrounding markup —
/// the `const` declaration, the quotes and the trailing comment are the
/// bytes the swap must preserve.
const MARKED_UP: &str = "pub const VERSION: &str = \"1.4.2\"; // the release row\n";

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
    let length = u32::try_from(needle.len()).unwrap_or_else(|error| {
        eprintln!("the fixture needle fits u32: {error}");
        std::process::exit(101);
    });
    (start, start + length)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = RelPath::new("src/version.rs").ok_or("fixture path")?;
    let specs = [GenericFileSpec {
        name: "app".to_owned(),
        file: "src/version.rs".to_owned(),
        pattern: "VERSION: &str = \"(?<version>\\d+\\.\\d+\\.\\d+)\";".to_owned(),
    }];
    let adapter = GenericAdapter::new(&specs);
    let (start, end) = span_of(MARKED_UP, "1.4.2");
    let patches = [PlanPatch {
        start,
        end,
        text: "1.5.0".to_owned(),
    }];

    let out = adapter.edit_versions(&file, MARKED_UP.as_bytes(), &patches)?;
    println!("applied {} patch(es), skipped {}", out.applied, out.skipped);
    println!("before: {MARKED_UP}");
    println!("after:  {}", String::from_utf8(out.content.clone())?);

    // The idempotent re-run: the same patch (already carried) skips —
    // matched, bytes verbatim (I1).
    let rerun = adapter.edit_versions(&file, &out.content, &patches)?;
    println!(
        "idempotent re-run: applied {}, skipped {} (I1)",
        rerun.applied, rerun.skipped
    );
    Ok(())
}
