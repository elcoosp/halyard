//! Property tests (§15.1): batched patch application equals sequential
//! application, and `apply` matches the byte-range definition exactly
//! (invariant I4: untouched bytes never change).

use halyard_edit::{Patch, apply};
use proptest::prelude::*;

/// Lowercase-letters content, deterministic and shrinkable.
fn content_strategy() -> impl Strategy<Value = String> {
    proptest::collection::vec(proptest::char::range('a', 'z'), 1..60)
        .prop_map(|chars| chars.into_iter().collect())
}

/// A `(start, len, text)` triple within the content bounds.
fn patch_spec_strategy(max_len: usize) -> impl Strategy<Value = (usize, usize, String)> {
    (0usize..max_len, 0usize..6, "[A-Z0-9]{1,4}")
        .prop_map(move |(start, len, text)| (start.min(max_len.saturating_sub(1)), len, text))
}

/// Turns specs into non-overlapping ascending patches.
fn real_patches(
    content: &str,
    specs: &[(usize, usize, String)],
) -> Vec<(Patch, usize, usize, String)> {
    let bytes_len = content.len();
    let mut specs: Vec<(u32, u32, String)> = specs
        .iter()
        .map(|(start, len, text)| {
            let start = u32::try_from(*start)
                .unwrap_or(0)
                .min(u32::try_from(bytes_len).unwrap_or(u32::MAX));
            let end = start
                .saturating_add(u32::try_from(*len).unwrap_or(0))
                .min(u32::try_from(bytes_len).unwrap_or(u32::MAX));
            let end = end.max(start);
            (start, end, text.clone())
        })
        .collect();
    specs.sort_by_key(|(start, end, _)| (*start, *end));
    let mut kept = Vec::new();
    let mut last_end = 0u32;
    for (start, end, text) in specs {
        if start >= last_end {
            let patch = Patch::new(start, end, text.clone())
                .unwrap_or_else(|_| unreachable!("specs are pre-validated ascending"));
            kept.push((patch, start as usize, end as usize, text));
            last_end = end;
        }
    }
    kept
}

proptest::proptest! {
    #[test]
    fn apply_matches_the_byte_range_definition(
        content in content_strategy(),
        specs in proptest::collection::vec(patch_spec_strategy(60), 0..6),
    ) {
        let patches = real_patches(&content, &specs);
        let real: Vec<Patch> = patches.iter().map(|(p, ..)| p.clone()).collect();
        let out = apply(content.as_bytes(), &real).unwrap_or_else(|e| panic!("apply: {e}"));

        // Definition: out == content[..p1.start] + p1.text + content[p1.end..p2.start] + …
        let mut expected: Vec<u8> = Vec::with_capacity(content.len());
        let mut pos = 0usize;
        for (patch, start, end, text) in &patches {
            let _ = patch;
            expected.extend_from_slice(content.as_bytes().get(pos..*start).unwrap_or(&[]));
            expected.extend_from_slice(text.as_bytes());
            pos = *end;
        }
        expected.extend_from_slice(content.as_bytes().get(pos..).unwrap_or(&[]));
        prop_assert_eq!(out, expected);
    }

    #[test]
    fn batched_application_equals_sequential_application(
        content in content_strategy(),
        specs in proptest::collection::vec(patch_spec_strategy(60), 0..6),
    ) {
        // Length-preserving replacements keep every patch's offsets valid
        // against the progressively edited content, so one-at-a-time
        // application must equal the batched result.
        let patches = real_patches(&content, &specs);
        let real: Vec<Patch> = patches
            .iter()
            .map(|(patch, _, _, _)| {
                let width = (patch.end() - patch.start()) as usize;
                let text: String = patch
                    .text()
                    .chars()
                    .cycle()
                    .take(width)
                    .collect();
                Patch::new(patch.start(), patch.end(), text)
                    .unwrap_or_else(|_| unreachable!("adjusted patch stays valid"))
            })
            .collect();
        // Insertions sharing one offset reorder between batched and
        // sequential application (each insert lands at the same byte), so
        // the equivalence property covers strictly increasing starts.
        let mut strictly: Vec<Patch> = Vec::new();
        let mut last_end: Option<u32> = None;
        for patch in &real {
            let ok = last_end.is_none_or(|prev| patch.start() > prev);
            if ok {
                strictly.push(patch.clone());
                last_end = Some(patch.end());
            }
        }
        let batched =
            apply(content.as_bytes(), &strictly).unwrap_or_else(|e| panic!("batched: {e}"));
        let mut sequential = content.as_bytes().to_vec();
        for patch in &strictly {
            sequential = apply(&sequential, std::slice::from_ref(patch))
                .unwrap_or_else(|e| panic!("sequential at {}: {e}", patch.start()));
        }
        prop_assert_eq!(batched, sequential);
    }

    #[test]
    fn identity_patch_is_a_noop(content in content_strategy()) {
        let out = apply(content.as_bytes(), &[]).unwrap_or_else(|e| panic!("empty: {e}"));
        prop_assert_eq!(out, content.as_bytes());
        let len = u32::try_from(content.len()).unwrap_or(u32::MAX);
        let identity = Patch::new(0, len, content.clone())
            .unwrap_or_else(|_| unreachable!("identity patch is valid"));
        let out = apply(content.as_bytes(), &[identity]).unwrap_or_else(|e| panic!("identity: {e}"));
        prop_assert_eq!(out, content.as_bytes());
    }
}
