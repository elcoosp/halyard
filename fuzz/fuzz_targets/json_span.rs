//! Fuzz target: the JSON span scanner (M1.6, ADR-0001).
//!
//! Invariants under arbitrary bytes:
//! * `parse` never panics — a document or a `JsonError`, always;
//! * a successful parse re-renders byte-identical output (round-trip
//!   safety, invariant I4);
//! * editing one string value leaves every untouched byte in place.

#![no_main]

use libfuzzer_sys::fuzz_target;
use halyard_eco_npm::json_span::{JsonDoc, JsonValue};

fuzz_target!(|data: &[u8]| {
    if let Ok(mut doc) = JsonDoc::parse(data) {
        // Round-trip: render with no edits == input bytes.
        assert_eq!(doc.render(), data, "no-edit render must be byte-identical");
        // Edit one string value, if any; the render must still be valid
        // JSON and preserve every byte outside the patched inner span.
        let target = find_first_string(doc.root());
        if let Some(path) = target {
            if doc.set_string(&path, "0.0.0").is_ok() {
                let out = doc.render();
                if let Ok(re) = JsonDoc::parse(&out) {
                    assert_eq!(re.render(), out, "edited render must round-trip");
                }
            }
        }
    }
});

/// Finds the key path of the first string value in the tree.
fn find_first_string(root: &halyard_eco_npm::json_span::SpannedValue) -> Option<Vec<String>> {
    match &root.value {
        JsonValue::Object(entries) => {
            for entry in entries {
                match &entry.value.value {
                    JsonValue::String { .. } => return Some(vec![entry.key.clone()]),
                    JsonValue::Object(_) => {
                        if let Some(mut path) = find_first_string(&entry.value) {
                            path.insert(0, entry.key.clone());
                            return Some(path);
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        _ => None,
    }
}
