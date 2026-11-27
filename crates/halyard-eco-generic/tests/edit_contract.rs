//! The M8 `generic` cell's edit contract (ADR-0062's release half): the
//! plan's own-version byte patches re-apply **semantically and in place**
//! — each patch span must land on the declared pattern's named `version`
//! capture group's byte span (the locator grammar's own
//! *"This capture group identifies the version number to replace"* —
//! the value the probe pairs), the captured bytes swap and every other
//! byte survives verbatim (invariant I4), and a re-match read-back
//! verifies the file now carries the target before the patch is
//! accepted.
//!
//! A patch aimed at any other span refuses (`E-EDIT-PATCHES`); a
//! re-match that does not carry the target refuses (`E-EDIT-VERIFY`
//! posture); a patch whose text already equals the current capture is
//! the idempotent re-run (I1): matched, skipped, bytes verbatim.

use halyard_core::EcosystemAdapter;
use halyard_core::discover::GenericFileSpec;
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;
use halyard_eco_generic::GenericAdapter;

/// The declared-file fixture: a version row inside surrounding markup —
/// the `const` declaration, the quotes and the trailing comment are the
/// bytes the swap must preserve.
const MARKED_UP: &str = "pub const VERSION: &str = \"1.4.2\"; // the release row\n";

fn spec(pattern: &str) -> GenericFileSpec {
    GenericFileSpec {
        name: "app".to_owned(),
        file: "src/version.rs".to_owned(),
        pattern: pattern.to_owned(),
    }
}

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

/// The capture swaps in place and every other byte survives verbatim
/// (I4) — the `const` declaration, the quotes, the trailing comment.
#[test]
fn swaps_the_capture_and_preserves_the_markup() {
    let file = RelPath::new("src/version.rs").expect("fixture path");
    let adapter =
        GenericAdapter::new(&[spec("VERSION: &str = \"(?<version>\\d+\\.\\d+\\.\\d+)\";")]);
    let patches = [patch(MARKED_UP, "1.4.2", "1.5.0")];
    let out = adapter
        .edit_versions(&file, MARKED_UP.as_bytes(), &patches)
        .expect("applies");
    assert_eq!((out.applied, out.skipped), (1, 0));
    assert_eq!(
        String::from_utf8(out.content).expect("valid UTF-8"),
        "pub const VERSION: &str = \"1.5.0\"; // the release row\n",
        "the markup around the capture survives verbatim (I4)"
    );
}

/// A patch whose span is not the declared pattern's capture span
/// refuses (`E-EDIT-PATCHES`) — the working tree drifted from the file
/// the plan was computed against; nothing is mutated.
#[test]
fn a_patch_off_the_capture_span_refuses() {
    let file = RelPath::new("src/version.rs").expect("fixture path");
    let adapter =
        GenericAdapter::new(&[spec("VERSION: &str = \"(?<version>\\d+\\.\\d+\\.\\d+)\";")]);
    // One byte right of the capture: no editable row there.
    let (start, end) = span_of(MARKED_UP, "1.4.2");
    let patches = [PlanPatch {
        start: start + 1,
        end: end + 1,
        text: "1.5.0".to_owned(),
    }];
    let error = adapter
        .edit_versions(&file, MARKED_UP.as_bytes(), &patches)
        .expect_err("the drifted span refuses");
    assert_eq!(error.code(), "E-EDIT-PATCHES");
    assert!(
        MARKED_UP.contains("1.4.2"),
        "nothing was mutated on refusal"
    );
}

/// A patch whose text already equals the current capture is the
/// idempotent re-run (I1): matched, skipped, bytes verbatim.
#[test]
fn the_idempotent_rerun_skips_the_carried_target() {
    let file = RelPath::new("src/version.rs").expect("fixture path");
    let adapter =
        GenericAdapter::new(&[spec("VERSION: &str = \"(?<version>\\d+\\.\\d+\\.\\d+)\";")]);
    let patches = [patch(MARKED_UP, "1.4.2", "1.4.2")];
    let out = adapter
        .edit_versions(&file, MARKED_UP.as_bytes(), &patches)
        .expect("applies");
    assert_eq!((out.applied, out.skipped), (0, 1));
    assert_eq!(
        String::from_utf8(out.content).expect("valid UTF-8"),
        MARKED_UP,
        "bytes verbatim on the idempotent re-run (I1)"
    );
}

/// The read-back verify: a target the declared pattern's capture cannot
/// carry (the group's own grammar pins a literal prefix) refuses after
/// the would-be swap — the file would no longer match the declared
/// locator, so nothing is accepted (`E-EDIT-VERIFY` posture; the
/// planner targets a pattern-carriable spelling).
#[test]
fn a_target_the_pattern_cannot_carry_refuses_verify() {
    let file = RelPath::new("src/version.rs").expect("fixture path");
    let adapter = GenericAdapter::new(&[spec("VERSION: &str = \"(?<version>1\\.\\d+\\.\\d+)\";")]);
    let patches = [patch(MARKED_UP, "1.4.2", "2.0.0")];
    let error = adapter
        .edit_versions(&file, MARKED_UP.as_bytes(), &patches)
        .expect_err("the uncarrable target refuses");
    assert_eq!(error.code(), "E-EDIT-VERIFY");
    assert!(
        MARKED_UP.contains("1.4.2"),
        "nothing was mutated on the verify refusal"
    );
}

/// A file that matches nothing (the declared row vanished — the working
/// tree drifted) refuses (`E-EDIT-PATCHES`); nothing is mutated.
#[test]
fn a_non_matching_file_refuses() {
    let file = RelPath::new("src/version.rs").expect("fixture path");
    let adapter =
        GenericAdapter::new(&[spec("VERSION: &str = \"(?<version>\\d+\\.\\d+\\.\\d+)\";")]);
    let drifted = "pub const VERSION: &str = \"removed\";\n";
    let patches = [patch(MARKED_UP, "1.4.2", "1.5.0")];
    let error = adapter
        .edit_versions(&file, drifted.as_bytes(), &patches)
        .expect_err("the drifted file refuses");
    assert_eq!(error.code(), "E-EDIT-PATCHES");
}

/// A file the adapter holds no spec for (the apply shell routed a
/// foreign file here) refuses fail-closed — the generic adapter edits
/// only the declared version files, never a guess.
#[test]
fn a_file_without_a_spec_refuses() {
    let file = RelPath::new("OTHER").expect("fixture path");
    let adapter =
        GenericAdapter::new(&[spec("VERSION: &str = \"(?<version>\\d+\\.\\d+\\.\\d+)\";")]);
    let patches = [patch(MARKED_UP, "1.4.2", "1.5.0")];
    let error = adapter
        .edit_versions(&file, MARKED_UP.as_bytes(), &patches)
        .expect_err("the undeclared file refuses");
    assert_eq!(error.code(), "E-EDIT-PATCHES");
}

/// The legitimate empty pass (nothing to apply): bytes verbatim,
/// applied 0, skipped 0 — the sibling adapters' posture.
#[test]
fn an_empty_pass_returns_the_bytes_verbatim() {
    let file = RelPath::new("src/version.rs").expect("fixture path");
    let adapter =
        GenericAdapter::new(&[spec("VERSION: &str = \"(?<version>\\d+\\.\\d+\\.\\d+)\";")]);
    let out = adapter
        .edit_versions(&file, MARKED_UP.as_bytes(), &[])
        .expect("applies");
    assert_eq!((out.applied, out.skipped), (0, 0));
    assert_eq!(out.content, MARKED_UP.as_bytes());
}

/// A second patch that already carries the post-swap capture is the
/// idempotent re-run within one pass: applied 1, skipped 1 (the swap
/// shifts nothing — same-length target — and the re-located capture
/// equals the second patch's text).
#[test]
fn a_second_identical_patch_is_skipped_within_one_pass() {
    let file = RelPath::new("src/version.rs").expect("fixture path");
    let adapter =
        GenericAdapter::new(&[spec("VERSION: &str = \"(?<version>\\d+\\.\\d+\\.\\d+)\";")]);
    let patches = [
        patch(MARKED_UP, "1.4.2", "1.5.0"),
        patch(MARKED_UP, "1.4.2", "1.5.0"),
    ];
    let out = adapter
        .edit_versions(&file, MARKED_UP.as_bytes(), &patches)
        .expect("applies");
    assert_eq!((out.applied, out.skipped), (1, 1));
    assert_eq!(
        String::from_utf8(out.content).expect("valid UTF-8"),
        "pub const VERSION: &str = \"1.5.0\"; // the release row\n"
    );
}
