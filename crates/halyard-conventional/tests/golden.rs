//! Golden tests against the Conventional Commits 1.0.0 spec examples
//! (<https://www.conventionalcommits.org/en/v1.0.0/#examples>, quoted
//! verbatim) plus the halyard-specific behaviours from §8.5 (PR-number
//! extraction, fixup/squash detection) and §10.1 (breaking detection).

use bstr::ByteSlice;
use halyard_conventional::{extract_pr_number, is_fixup_or_squash, parse};

#[test]
fn spec_example_type_only() {
    let cc = parse(b"feat: allow provided config to extend other configs").unwrap();
    assert_eq!(cc.kind.as_bytes(), b"feat".as_slice());
    assert_eq!(cc.scope, None);
    assert!(!cc.breaking);
    assert_eq!(
        cc.description.as_bytes(),
        b"allow provided config to extend other configs".as_slice()
    );
    assert_eq!(cc.body, None);
    assert_eq!(cc.footers.len(), 0);
}

#[test]
fn spec_example_breaking_bang_without_scope() {
    let cc = parse(b"feat!: send an email to the customer when a product is shipped").unwrap();
    assert_eq!(cc.kind.as_bytes(), b"feat".as_slice());
    assert!(cc.breaking, "`!` alone marks breaking");
    assert_eq!(cc.footers.len(), 0);
}

#[test]
fn spec_example_breaking_bang_with_scope() {
    let cc = parse(b"feat(api)!: send an email to the customer when a product is shipped").unwrap();
    assert_eq!(cc.kind.as_bytes(), b"feat".as_slice());
    assert_eq!(
        cc.scope.as_ref().map(|s| s.as_bytes()),
        Some(b"api".as_slice())
    );
    assert!(cc.breaking);
}

#[test]
fn spec_example_breaking_change_footer() {
    let message = b"feat: allow provided config to extend other configs\n\n\
BREAKING CHANGE: `extends` key in config file is now used for extending other config files";
    let cc = parse(message).unwrap();
    assert_eq!(cc.kind.as_bytes(), b"feat".as_slice());
    assert!(cc.breaking, "BREAKING CHANGE footer marks breaking");
    assert_eq!(cc.body, None);
    assert_eq!(cc.footers.len(), 1);
    let footer = cc.footers.first().unwrap();
    assert_eq!(footer.key.as_bytes(), b"BREAKING CHANGE".as_slice());
    assert_eq!(
        footer.value.as_bytes(),
        b"`extends` key in config file is now used for extending other config files".as_slice()
    );
}

#[test]
fn spec_example_body_and_footer() {
    let message = b"chore: drop support for Node 6\n\n\
it gets difficult to test in CI.\n\n\
BREAKING CHANGE: drop support for Node 6 which is no longer supported by the Node team.";
    let cc = parse(message).unwrap();
    assert_eq!(
        cc.body.as_ref().map(|b| b.as_bytes()),
        Some(b"it gets difficult to test in CI.".as_slice())
    );
    assert_eq!(cc.footers.len(), 1);
    assert!(cc.breaking);
}

#[test]
fn spec_example_multiple_footers() {
    let message = b"fix: correct minor typo for happy path\n\n\
Happy path now is correct\n\n\
Reviewed-by: Z\nRefs: #133";
    let cc = parse(message).unwrap();
    assert_eq!(cc.kind.as_bytes(), b"fix".as_slice());
    assert_eq!(
        cc.body.as_ref().map(|b| b.as_bytes()),
        Some(b"Happy path now is correct".as_slice())
    );
    assert_eq!(cc.footers.len(), 2);
    let first = cc.footers.first().unwrap();
    assert_eq!(first.key.as_bytes(), b"Reviewed-by".as_slice());
    assert_eq!(first.value.as_bytes(), b"Z".as_slice());
    let second = cc.footers.get(1).unwrap();
    assert_eq!(second.key.as_bytes(), b"Refs".as_slice());
    assert_eq!(second.value.as_bytes(), b"#133".as_slice());
    assert!(!cc.breaking);
}

#[test]
fn hash_separator_footer() {
    // git trailers also accept `key #value`; the parser must handle both.
    let message = b"fix: buffer overflow\n\nAcked-by: someone #2";
    let cc = parse(message).unwrap();
    assert_eq!(cc.footers.len(), 1);
    let footer = cc.footers.first().unwrap();
    assert_eq!(footer.key.as_bytes(), b"Acked-by".as_slice());
    assert_eq!(footer.value.as_bytes(), b"someone #2".as_slice());
}

#[test]
fn breaking_change_alias_and_multiline_value() {
    let message = b"feat: new API surface\n\n\
BREAKING-CHANGE: the signature changed.\n  continuation line stays in the value\n\
Signed-off-by: A. U. Thor <author@example.com>";
    let cc = parse(message).unwrap();
    assert!(cc.breaking);
    let breaking = cc
        .footers
        .iter()
        .find(|f| f.key.as_bytes() == b"BREAKING-CHANGE".as_slice())
        .unwrap();
    assert_eq!(
        breaking.value.as_bytes(),
        b"the signature changed.\ncontinuation line stays in the value".as_slice()
    );
    assert_eq!(cc.footers.len(), 2);
}

#[test]
fn scope_with_escaped_parenthesis() {
    // Spec FAQ: a scope may contain parentheses when escaped.
    let cc = parse(br"feat(scope\(with\)parens): description").unwrap();
    assert_eq!(
        cc.scope.as_ref().map(|s| s.as_bytes()),
        Some(br"scope\(with\)parens".as_slice())
    );
}

#[test]
fn non_conventional_subjects_return_none() {
    for subject in [
        &b"just some update"[..],
        b"Merge branch 'main' into feature",
        b"feat:no-space-after-colon",
        b"feat: ",
        b"feat:",
        b"feat(): empty scope",
        b": no type",
        b"feat : space before colon",
        b"",
        b"feat:\n\nBREAKING CHANGE: no description after the colon",
    ] {
        assert!(
            parse(subject).is_none(),
            "{subject:?} must not parse as conventional"
        );
    }
}

#[test]
fn fixup_and_squash_detection() {
    assert!(is_fixup_or_squash(b"fixup! the subject it fixes"));
    assert!(is_fixup_or_squash(b"squash! the subject it squashes"));
    assert!(!is_fixup_or_squash(b"fix: normal commit"));
    assert!(!is_fixup_or_squash(b"fixup-prefix without bang"));
    // A fixup commit itself is not a conventional commit.
    assert!(parse(b"fixup! feat: something").is_none());
}

#[test]
fn pr_number_extraction() {
    assert_eq!(
        extract_pr_number(b"feat: add beta language (#123)"),
        Some(123)
    );
    assert_eq!(
        extract_pr_number(b"feat: trailing spaces   (#42)  "),
        Some(42)
    );
    assert_eq!(extract_pr_number(b"feat: no pr"), None);
    assert_eq!(extract_pr_number(b"feat: parens but no hash (123)"), None);
    assert_eq!(extract_pr_number(b"feat: empty parens ()"), None);
    assert_eq!(extract_pr_number(b"feat: hash but no parens #7"), None);
    // Parsed commits carry the extracted number.
    let cc = parse(b"feat(parser): handle refs (#412)").unwrap();
    assert_eq!(cc.pr, Some(412));
}

#[test]
fn crlf_messages_parse() {
    let message = b"fix: windows-authored message\r\n\r\nbody line\r\n\r\nBREAKING CHANGE: yes\r\n";
    let cc = parse(message).unwrap();
    assert!(cc.breaking);
    assert_eq!(
        cc.body.as_ref().map(|b| b.as_bytes()),
        Some(b"body line".as_slice())
    );
}

#[test]
fn unicode_subjects_parse() {
    let cc = parse("feat: ünïcödé beschrijving ✓".as_bytes()).unwrap();
    assert_eq!(
        cc.description.as_bytes(),
        "ünïcödé beschrijving ✓".as_bytes()
    );
}

#[test]
fn parsed_output_snapshot() {
    // Deterministic Debug rendering pinned as a snapshot (I3).
    let parsed: Vec<_> = [
        &b"feat: allow provided config to extend other configs"[..],
        b"feat(api)!: send an email (#42)",
        b"fix: correct minor typo\n\nReviewed-by: Z\nRefs: #133",
    ]
    .iter()
    .map(|m| parse(m))
    .collect();
    insta::assert_debug_snapshot!("parsed_spec_samples", parsed);
}
