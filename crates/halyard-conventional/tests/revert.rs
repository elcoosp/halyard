//! Revert / autosquash / PR-number scanner tests (M2.4, §8.5): golden
//! vectors for `revert_target`, the fixup/squash detector and the
//! trailing-PR-number byte scan, plus a totality proptest (the scanners
//! are hand-written byte scans — arbitrary input must never panic).

use halyard_conventional::{
    extract_pr_number, is_fixup_or_squash, parse, revert_target, trailing_pr_number,
};

fn message(subject: &str, body: &str) -> Vec<u8> {
    let mut out = subject.as_bytes().to_vec();
    if !body.is_empty() {
        out.push(b'\n');
        out.push(b'\n');
        out.extend_from_slice(body.as_bytes());
    }
    out
}

#[test]
fn revert_target_matches_git_canonical_body() {
    let oid = "9f32036f7215bb0fb590f120442648feca5a6104";
    let msg = message(
        &format!(r#"Revert "feat: grow parser""#),
        &format!("This reverts commit {oid}."),
    );
    assert_eq!(revert_target(&msg).map(|t| t.to_string()), Some(oid.to_owned()));
}

#[test]
fn revert_target_without_trailing_period() {
    let oid = "0123456789abcdef0123456789abcdef01234567";
    let msg = message("revert: undo", &format!("This reverts commit {oid}"));
    assert_eq!(revert_target(&msg).map(|t| t.to_string()), Some(oid.to_owned()));
}

#[test]
fn revert_target_tolerates_surrounding_lines_and_indent() {
    let oid = "ffffffffffffffffffffffffffffffffffffffff";
    let msg = message(
        "fix: something else",
        &format!("why:\n\n  This reverts commit {oid}.\n\nbecause reasons"),
    );
    assert_eq!(revert_target(&msg).map(|t| t.to_string()), Some(oid.to_owned()));
}

#[test]
fn revert_target_rejects_bad_shapes() {
    let truncated = message("revert: x", "This reverts commit 9f32036 (short).");
    assert_eq!(revert_target(&truncated), None);
    let nonhex = message("revert: x", "This reverts commit zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz.");
    assert_eq!(revert_target(&nonhex), None);
    let absent = message("revert: x", "This reverts commit .");
    assert_eq!(revert_target(&absent), None);
    // Lowercase variant is git's *subject* shape only; no OID, no match.
    let lowercase_key = message("revert: x", "this reverts commit 9f32036f7215bb0fb590f120442648feca5a6104");
    assert_eq!(revert_target(&lowercase_key), None);
    assert_eq!(revert_target(b""), None);
}

#[test]
fn revert_target_accepts_sha256_len() {
    let oid = "a".repeat(64);
    let msg = message("revert: x", &format!("This reverts commit {oid}."));
    assert_eq!(revert_target(&msg).map(|t| t.len()), Some(64));
}

#[test]
fn fixup_and_squash_are_detected_and_unparsed() {
    assert!(is_fixup_or_squash(b"fixup! feat: grow"));
    assert!(is_fixup_or_squash(b"squash! fix(parser): edge"));
    assert!(!is_fixup_or_squash(b"fixup"));
    assert!(!is_fixup_or_squash(b"feat: fixup! later"));
    // Autosquash subjects never parse (§8.5: no RelevantCommit, no reason).
    assert!(parse(b"fixup! feat: grow").is_none());
    assert!(parse(b"squash! feat: grow").is_none());
}

#[test]
fn trailing_pr_number_golden() {
    assert_eq!(trailing_pr_number(b"feat: grow (#42)"), Some(42));
    assert_eq!(extract_pr_number(b"fix: mend  (#1234)\r\n"), Some(1234));
    assert_eq!(trailing_pr_number(b"feat: grow #42"), None);
    assert_eq!(trailing_pr_number(b"feat: grow (#x)"), None);
    assert_eq!(trailing_pr_number(b"feat: grow ( )"), None);
    assert_eq!(trailing_pr_number(b"feat: grow (#)"), None);
    assert_eq!(trailing_pr_number(b"feat: grow (#00000)"), Some(0));
    assert_eq!(trailing_pr_number(b""), None);
}

/// Totality: none of the three scanners panics on arbitrary bytes
/// (§15.1 property layer for hand-written scanners).
#[test]
fn scanners_are_total_on_arbitrary_bytes() {
    proptest::proptest!(|(data in proptest::collection::vec(0u8..255, 0..400))| {
        let _ = revert_target(&data);
        let _ = is_fixup_or_squash(&data);
        let _ = trailing_pr_number(&data);
    });
}
