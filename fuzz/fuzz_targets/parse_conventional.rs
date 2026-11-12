//! Fuzz target for the conventional-commit parser (§15.1 fuzz layer).
//!
//! Invariants for any accepted input: kind and description are non-empty,
//! every footer key is non-empty, and a `BREAKING CHANGE` footer always
//! implies the breaking flag.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(cc) = halyard_conventional::parse(data) {
        assert!(!cc.kind.is_empty(), "kind must be non-empty");
        assert!(!cc.description.is_empty(), "description must be non-empty");
        for footer in &cc.footers {
            assert!(!footer.key.is_empty(), "footer key must be non-empty");
        }
        assert!(
            !cc.has_breaking_footer() || cc.breaking,
            "BREAKING footer must mark the commit breaking"
        );
        if let Some(pr) = cc.pr {
            assert!(pr > 0, "extracted PR numbers are positive");
        }
    }
});
