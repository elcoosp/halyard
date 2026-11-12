//! Property tests (§15.1): the conventional parser is *total* — it never
//! panics on arbitrary bytes — and every accepted commit satisfies the
//! parser's invariants.

use halyard_conventional::{extract_pr_number, parse};
use proptest::prelude::*;

proptest::proptest! {
    #[test]
    fn parse_never_panics(input in proptest::collection::vec(proptest::num::u8::ANY, 0..400)) {
        // Total function: no panic for any byte string.
        if let Some(cc) = parse(&input) {
            prop_assert!(!cc.kind.is_empty());
            prop_assert!(!cc.description.is_empty());
            for footer in &cc.footers {
                prop_assert!(!footer.key.is_empty());
            }
            // A BREAKING footer always implies the breaking flag.
            prop_assert!(
                !cc.has_breaking_footer() || cc.breaking,
                "BREAKING footer must mark the commit breaking"
            );
        }
    }

    #[test]
    fn pr_extraction_matches_trailing_parens(
        subject in "[a-z]+(\\([a-z]+\\))?!?: [^\n]{1,80}",
        pr in proptest::option::of(1u64..100_000),
    ) {
        let composed = match pr {
            Some(n) => format!("{subject} (#{n})"),
            None => subject,
        };
        let parsed = parse(composed.as_bytes());
        if parsed.is_some() {
            prop_assert_eq!(parsed.as_ref().unwrap().pr, extract_pr_number(composed.as_bytes()));
            prop_assert_eq!(parsed.as_ref().unwrap().pr, pr);
        }
    }
}
