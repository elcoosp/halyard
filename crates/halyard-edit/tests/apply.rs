//! Unit/golden tests for byte-range patch application (M1.4, §9.1).

use halyard_core::RelPath;
use halyard_edit::{EditError, EditSet, LineEndings, Patch, apply, detect_line_endings};

fn patch(start: u32, end: u32, text: &str) -> Patch {
    Patch::new(start, end, text.to_owned())
        .unwrap_or_else(|_| unreachable!("static patch specs are valid"))
}

fn rel(raw: &str) -> RelPath {
    RelPath::new(raw).unwrap_or_else(|| unreachable!("static paths are valid"))
}

#[test]
fn single_patch_replaces_byte_range() {
    let content = b"version = \"0.4.1\"\nname = \"core\"\n";
    let out =
        apply(content, &[patch(11, 16, "0.5.0")]).unwrap_or_else(|e| panic!("apply failed: {e}"));
    let text = String::from_utf8(out).unwrap_or_default();
    assert_eq!(text, "version = \"0.5.0\"\nname = \"core\"\n");
}

#[test]
fn zero_width_patch_inserts() {
    let out = apply(b"abc", &[patch(1, 1, "X")]).unwrap_or_else(|e| panic!("apply failed: {e}"));
    assert_eq!(out, b"aXbc");
}

#[test]
fn multiple_patches_apply_in_ascending_order() {
    let content = b"alpha beta gamma";
    let out = apply(content, &[patch(0, 5, "ALPHA"), patch(6, 10, "BETA")])
        .unwrap_or_else(|e| panic!("apply failed: {e}"));
    assert_eq!(out, b"ALPHA BETA gamma");
}

#[test]
fn out_of_bounds_patch_is_rejected() {
    let err = apply(b"short", &[patch(0, 10, "x")])
        .err()
        .unwrap_or_else(|| panic!("must fail"));
    assert_eq!(err.code(), EditError::PATCHES_CODE);
    assert_eq!(err.code(), "E-EDIT-PATCHES");
}

#[test]
fn overlapping_patches_are_rejected_at_construction() {
    let mut set = EditSet::new();
    let file = rel("a.toml");
    set.add(file.clone(), patch(0, 4, "one"))
        .unwrap_or_else(|e| panic!("first patch must validate: {e}"));
    let err = set
        .add(file, patch(2, 6, "two"))
        .err()
        .unwrap_or_else(|| panic!("overlap must fail"));
    assert_eq!(err.code(), "E-EDIT-PATCHES");
}

#[test]
fn edit_set_applies_files_in_sorted_order() {
    let mut set = EditSet::new();
    for name in ["c.toml", "a.toml", "b.toml"] {
        set.add(rel(name), patch(0, 0, name))
            .unwrap_or_else(|e| panic!("add failed: {e}"));
    }
    let outputs = set
        .apply_all(|file| Ok(file.as_str().as_bytes().to_vec()))
        .unwrap_or_else(|e| panic!("apply_all failed: {e}"));
    let names: Vec<&str> = outputs.keys().map(RelPath::as_str).collect();
    assert_eq!(names, vec!["a.toml", "b.toml", "c.toml"]);
}

#[test]
fn eol_and_bom_untouched_bytes_are_preserved() {
    // BOM + CRLF manifest; `version = "0.4.1"` has its opening quote at byte
    // 24 (3-byte BOM + `[package]\r\n` + `version = `), so the value spans
    // 25..30.
    let content: &[u8] = b"\xEF\xBB\xBF[package]\r\nversion = \"0.4.1\"\r\npublish = false\r\n";
    let out =
        apply(content, &[patch(25, 30, "0.5.0")]).unwrap_or_else(|e| panic!("apply failed: {e}"));
    assert_eq!(
        String::from_utf8_lossy(&out),
        "\u{FEFF}[package]\r\nversion = \"0.5.0\"\r\npublish = false\r\n"
    );
    assert_eq!(detect_line_endings(&out), LineEndings::Crlf);
    assert_eq!(detect_line_endings(b"a\nb\n"), LineEndings::Lf);
    assert_eq!(detect_line_endings(b"a\r\nb\n"), LineEndings::Mixed);
    assert_eq!(detect_line_endings(b""), LineEndings::Lf);
}
