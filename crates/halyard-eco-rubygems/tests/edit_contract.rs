//! The M8 `RubyGems` cell's edit contract (ADR-0061's release half): the
//! plan's own-version byte patches re-apply **semantically and in place**
//! — each patch span must land on the gemspec's `version` literal's
//! inner-byte span (*"Versions shall be represented by three
//! non-negative integers, separated by periods (e.g. 3.1.4)."* — the
//! value the probe pairs), the literal's inner bytes swap (the quote
//! characters and trailing comments never move, invariant I4), and a
//! re-parse read-back verifies the gemspec now carries the target
//! before the patch is accepted.
//!
//! A patch aimed at any other span refuses (`E-EDIT-PATCHES`); a
//! re-parse that does not carry the target refuses (`E-EDIT-VERIFY`
//! posture); a patch whose text already equals the current value is the
//! idempotent re-run (I1): matched, skipped, bytes verbatim.

use halyard_core::EcosystemAdapter;
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;
use halyard_eco_rubygems::RubyGemsAdapter;

const APP: &str = "# frozen_string_literal: true\n\nGem::Specification.new do |spec|\n  spec.name = 'app'\n  spec.version = '1.2.3' # the release row\n  spec.summary = 'the application'\nend\n";

fn span_of(haystack: &str, needle: &str) -> (u32, u32) {
    let start = usize_to_u32(haystack.find(needle), needle);
    let length = usize_to_u32(Some(needle.len()), needle);
    (start, start + length)
}

/// The fixture offset as a plan-contract span coordinate (`None` cannot
/// happen for a constant fixture, but the failure must stay loud and
/// panic-free — the bench posture).
fn usize_to_u32(value: Option<usize>, needle: &str) -> u32 {
    if let Some(value) = value.and_then(|value| u32::try_from(value).ok()) {
        return value;
    }
    eprintln!("the fixture must carry {needle}");
    std::process::exit(101);
}

fn patch(haystack: &str, needle: &str, text: &str) -> PlanPatch {
    let (start, end) = span_of(haystack, needle);
    PlanPatch {
        start,
        end,
        text: text.to_owned(),
    }
}

/// The version literal swaps in place and every other byte survives
/// verbatim (I4) — the comment, the quote characters, the name row, the
/// summary row.
#[test]
fn swaps_the_value_and_preserves_the_markup() {
    let file = RelPath::new("gems/app/app.gemspec").expect("fixture path");
    let patches = [patch(APP, "1.2.3", "1.3.0")];
    let out = RubyGemsAdapter::new()
        .edit_versions(&file, APP.as_bytes(), &patches)
        .expect("applies");
    assert_eq!((out.applied, out.skipped), (1, 0));
    assert_eq!(
        String::from_utf8(out.content).expect("utf-8"),
        APP.replace("spec.version = '1.2.3'", "spec.version = '1.3.0'"),
        "only the literal's inner bytes move"
    );
}

/// A double-quoted version keeps the quotes: the patch lands on the
/// inner bytes, the quote characters survive.
#[test]
fn a_double_quoted_version_edits_the_inner_bytes() {
    let quoted = "Gem::Specification.new do |spec|\n  spec.name = \"app\"\n  spec.version = \"1.2.3\"\nend\n";
    let file = RelPath::new("gems/app/app.gemspec").expect("fixture path");
    let patches = [patch(quoted, "1.2.3", "2.0.0")];
    let out = RubyGemsAdapter::new()
        .edit_versions(&file, quoted.as_bytes(), &patches)
        .expect("applies");
    assert_eq!(
        String::from_utf8(out.content).expect("utf-8"),
        quoted.replace("\"1.2.3\"", "\"2.0.0\""),
        "the quotes survive"
    );
}

/// A dependency requirement is NOT the slot: a patch aimed at the
/// `~> 2.3` requirement span refuses — `RubyGems` requirements ride
/// verbatim (ADR-0006), only the gem's own `version` row is the editable
/// site.
#[test]
fn a_patch_at_a_dependency_requirement_refuses() {
    let with_dep = "Gem::Specification.new do |spec|\n  spec.name = 'app'\n  spec.version = '1.2.3'\n  spec.add_dependency 'shared', '~> 2.3'\nend\n";
    let file = RelPath::new("gems/app/app.gemspec").expect("fixture path");
    let patches = [patch(with_dep, "~> 2.3", "~> 2.4")];
    let err = RubyGemsAdapter::new()
        .edit_versions(&file, with_dep.as_bytes(), &patches)
        .expect_err("the requirement span is not the slot");
    assert_eq!(err.code(), "E-EDIT-PATCHES");
}

/// A patch whose span does not land on the slot refuses
/// (`E-EDIT-PATCHES`) — the manifest drifted or the plan is stale.
#[test]
fn a_span_off_the_slot_refuses() {
    let file = RelPath::new("gems/app/app.gemspec").expect("fixture path");
    let patches = [patch(APP, "'app'", "'other'")];
    let err = RubyGemsAdapter::new()
        .edit_versions(&file, APP.as_bytes(), &patches)
        .expect_err("refuses");
    assert_eq!(err.code(), "E-EDIT-PATCHES");
}

/// The idempotent re-run (I1): the value already carries the target —
/// matched, counted as skipped, bytes verbatim.
#[test]
fn the_idempotent_rerun_skips_the_matched_value() {
    let file = RelPath::new("gems/app/app.gemspec").expect("fixture path");
    let patches = [patch(APP, "1.2.3", "1.2.3")];
    let out = RubyGemsAdapter::new()
        .edit_versions(&file, APP.as_bytes(), &patches)
        .expect("applies");
    assert_eq!((out.applied, out.skipped), (0, 1), "already carried");
    assert_eq!(out.content, APP.as_bytes(), "the bytes are verbatim");
}

/// Empty passes return the verbatim bytes (nothing to apply, nothing
/// changed).
#[test]
fn empty_passes_return_the_verbatim_bytes() {
    let file = RelPath::new("gems/app/app.gemspec").expect("fixture path");
    let out = RubyGemsAdapter::new()
        .edit_versions(&file, APP.as_bytes(), &[])
        .expect("an empty pass");
    assert_eq!(out.content, APP.as_bytes());
    assert_eq!((out.applied, out.skipped), (0, 0));
}

/// A gemspec without a literal `version` row (absent, computed, or
/// conditional) has no slot: any patch refuses.
#[test]
fn a_slotless_gemspec_refuses_any_patch() {
    let plain = "Gem::Specification.new do |spec|\n  spec.name = 'plain'\n  spec.summary = 'no version row'\nend\n";
    let file = RelPath::new("gems/plain/plain.gemspec").expect("fixture path");
    let patches = [PlanPatch {
        start: 4,
        end: 9,
        text: "1.0.0".to_owned(),
    }];
    let err = RubyGemsAdapter::new()
        .edit_versions(&file, plain.as_bytes(), &patches)
        .expect_err("refuses");
    assert_eq!(err.code(), "E-EDIT-PATCHES");
}

/// The `.freeze` idiom on the version literal keeps the row editable:
/// the slot is still the literal's inner bytes.
#[test]
fn a_frozen_literal_keeps_its_slot() {
    let frozen = "Gem::Specification.new do |spec|\n  spec.name = 'app'\n  spec.version = '1.2.3'.freeze\nend\n";
    let file = RelPath::new("gems/app/app.gemspec").expect("fixture path");
    let patches = [patch(frozen, "1.2.3", "1.3.0")];
    let out = RubyGemsAdapter::new()
        .edit_versions(&file, frozen.as_bytes(), &patches)
        .expect("applies");
    assert_eq!(
        String::from_utf8(out.content).expect("utf-8"),
        frozen.replace("'1.2.3'.freeze", "'1.3.0'.freeze"),
        "the freeze call survives"
    );
}
