//! Example: apply a version bump to a gemspec and verify the bytes (M8,
//! the §17 **`RubyGems`** cell — ADR-0061, the release half).
//!
//! Run: `cargo run -p halyard-eco-rubygems --example bump_gemspec`
//!
//! The plan's own-version patch re-applies **semantically and in place**:
//! the patch span must land on the version literal's inner bytes, the
//! quotes and the trailing comment never move (I4), the idempotent
//! re-run skips the already-carried target (I1), and a slotless gemspec
//! (a computed version row — the *"Specifications must be
//! deterministic"* rule) refuses instead of guessing.

use halyard_core::edit::EditOutcome;
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;
use halyard_eco_rubygems::edit::edit_versions;

const APP_GEMSPEC: &str = "# frozen_string_literal: true

Gem::Specification.new do |spec|
  spec.name = 'app'
  spec.version = '1.4.0' # the release row
  spec.summary = 'the application'
  spec.add_dependency 'json', '>= 2.0'
end
";

const COMPUTED_GEMSPEC: &str = "# frozen_string_literal: true

Gem::Specification.new do |spec|
  spec.name = 'computed'
  spec.version = App::VERSION
end
";

/// The fixture offset as a plan-contract span coordinate (a constant
/// fixture must carry the version row — the failure stays loud).
fn span_of(haystack: &str, needle: &str) -> Result<u32, Box<dyn std::error::Error>> {
    let start = haystack
        .find(needle)
        .ok_or_else(|| format!("the fixture must carry {needle}"))?;
    Ok(u32::try_from(start)?)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = RelPath::new("gems/app/app.gemspec").ok_or("the fixture path is normalized")?;
    let start = span_of(APP_GEMSPEC, "1.4.0")?;
    let patches = [PlanPatch {
        start,
        end: start + 5,
        text: "1.5.0".to_owned(),
    }];

    let out: EditOutcome = edit_versions(&file, APP_GEMSPEC.as_bytes(), &patches)?;
    println!(
        "applied {} patch(es), skipped {} — the gemspec now carries 1.5.0:\n",
        out.applied, out.skipped
    );
    println!("{}", String::from_utf8(out.content)?);

    // The idempotent re-run (I1): the same patch matched, skipped,
    // bytes verbatim.
    let again = edit_versions(
        &file,
        APP_GEMSPEC.replace("1.4.0", "1.5.0").as_bytes(),
        &patches,
    )?;
    println!(
        "the re-run applies {} and skips {} (already carried)\n",
        again.applied, again.skipped
    );

    // A slotless gemspec (a computed version row) refuses fail-closed.
    let computed =
        RelPath::new("gems/computed/computed.gemspec").ok_or("the fixture path is normalized")?;
    match edit_versions(&computed, COMPUTED_GEMSPEC.as_bytes(), &patches) {
        Ok(_) => println!("the computed row applied (unexpected)"),
        Err(error) => println!("the computed version row refuses: {error}"),
    }
    Ok(())
}
